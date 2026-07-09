use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter, Manager, Window};
use tokio::process::Child;
use tokio::sync::Mutex;

const OPENVINO_SERVER_PORT: u16 = 8091;
const OPENVINO_SERVER_HOST: &str = "127.0.0.1";
const OPENVINO_DEVICE: &str = "NPU";

static OPENVINO_SERVER: OnceLock<Mutex<Option<Child>>> = OnceLock::new();

fn server_state() -> &'static Mutex<Option<Child>> {
    OPENVINO_SERVER.get_or_init(|| Mutex::new(None))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenvinoRuntimeStatus {
    pub installed: bool,
    pub server_running: bool,
    pub install_dir: String,
    pub python_path: String,
    pub requirements_path: String,
    pub server_log_path: String,
    pub default_model_dir: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenvinoModelProfile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub target_dir: String,
    pub repo_id: Option<String>,
    pub source_url: Option<String>,
    pub auto_downloadable: bool,
    pub size_hint: String,
    pub quality_tier: String,
}

fn openvino_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Nepodarilo se zjistit app data slozku: {e}"))?;
    Ok(data_dir.join("openvino"))
}

fn venv_dir(root: &Path) -> PathBuf {
    root.join("venv")
}

fn venv_python(root: &Path) -> PathBuf {
    if cfg!(windows) {
        venv_dir(root).join("Scripts").join("python.exe")
    } else {
        venv_dir(root).join("bin").join("python")
    }
}

fn requirements_path(root: &Path) -> PathBuf {
    root.join("requirements-openvino.txt")
}

fn marker_path(root: &Path) -> PathBuf {
    root.join(".weave-openvino-installed")
}

fn server_script_path(root: &Path) -> PathBuf {
    root.join("weave_openvino_server.py")
}

fn model_download_script_path(root: &Path) -> PathBuf {
    root.join("download_recommended_openvino_model.py")
}

fn server_log_path(root: &Path) -> PathBuf {
    root.join("weave_openvino_server.log")
}

fn default_model_dir(root: &Path) -> PathBuf {
    root.join("models").join("qwen3-8b-int4-cw-ov")
}

fn openvino_model_profiles(root: &Path) -> Vec<OpenvinoModelProfile> {
    vec![
        OpenvinoModelProfile {
            id: "qwen3-8b-int4-cw-ov".into(),
            name: "Qwen3 8B INT4 OpenVINO".into(),
            description: "Stabilni automaticky stazitelny OpenVINO IR model pro NPU. Dobra obecna kvalita, rychly start a rozumna pametova narocnost.".into(),
            target_dir: root
                .join("models")
                .join("qwen3-8b-int4-cw-ov")
                .display()
                .to_string(),
            repo_id: Some("OpenVINO/Qwen3-8B-int4-cw-ov".into()),
            source_url: Some("https://huggingface.co/OpenVINO/Qwen3-8B-int4-cw-ov".into()),
            auto_downloadable: true,
            size_hint: "INT4 / NPU friendly".into(),
            quality_tier: "Doporuceno pro NPU".into(),
        },
        OpenvinoModelProfile {
            id: "gemma-4-e4b-openvino-manual".into(),
            name: "Gemma 4 E4B Instruct".into(),
            description: "Kvalitnejsi Gemma profil pro cestinu a lokalni chat. Pro NPU vyzaduje OpenVINO IR adresar; GGUF verze patri do GPU/RAM backendu.".into(),
            target_dir: root
                .join("models")
                .join("gemma-4-e4b-it-ov")
                .display()
                .to_string(),
            repo_id: None,
            source_url: Some("https://huggingface.co/unsloth/gemma-4-E4B-it-GGUF".into()),
            auto_downloadable: false,
            size_hint: "manual OpenVINO IR".into(),
            quality_tier: "Gemma 4 / experimental".into(),
        },
        OpenvinoModelProfile {
            id: "gemma-4-26b-a4b-openvino-manual".into(),
            name: "Gemma 4 26B-A4B Instruct".into(),
            description: "Prioritni kvalitativni cil pro vynikajici cestinu. Na RTX 3090 pouzij GGUF doporuceny model; pro NPU je nutny predem pripraveny OpenVINO IR adresar a je to experimentalni cesta.".into(),
            target_dir: root
                .join("models")
                .join("gemma-4-26b-a4b-it-ov")
                .display()
                .to_string(),
            repo_id: None,
            source_url: Some("https://huggingface.co/unsloth/gemma-4-26B-A4B-it-GGUF".into()),
            auto_downloadable: false,
            size_hint: "26B MoE / manual OpenVINO IR".into(),
            quality_tier: "Nejlepsi cestina / experimental".into(),
        },
    ]
}

fn openvino_model_profile(root: &Path, profile_id: &str) -> Result<OpenvinoModelProfile, String> {
    openvino_model_profiles(root)
        .into_iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| format!("Neznamy OpenVINO model profil: {profile_id}"))
}

async fn is_server_running() -> bool {
    server_state().lock().await.is_some()
}

async fn status_for(root: &Path) -> OpenvinoRuntimeStatus {
    OpenvinoRuntimeStatus {
        installed: marker_path(root).exists() && venv_python(root).exists(),
        server_running: is_server_running().await,
        install_dir: root.display().to_string(),
        python_path: venv_python(root).display().to_string(),
        requirements_path: requirements_path(root).display().to_string(),
        server_log_path: server_log_path(root).display().to_string(),
        default_model_dir: default_model_dir(root).display().to_string(),
    }
}

// set_readonly(false) je tu záměr: pip/venv soubory mívají na Windows readonly
// flag, který blokuje remove_dir_all při odinstalaci. Volá se těsně před
// smazáním složky, takže „world writable" na Unixu je bez praktického dopadu.
#[allow(clippy::permissions_set_readonly_false)]
fn clear_readonly_flags(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        for entry in std::fs::read_dir(path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            clear_readonly_flags(&entry.path())?;
        }
    }
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let mut permissions = metadata.permissions();
    if permissions.readonly() {
        permissions.set_readonly(false);
        std::fs::set_permissions(path, permissions).map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn emit_step(window: &Window, name: impl Into<String>) {
    let _ = window.emit(
        "openvino-install-progress",
        serde_json::json!({ "type": "step", "name": name.into() }),
    );
}

async fn emit_output(window: &Window, line: impl Into<String>) {
    let _ = window.emit(
        "openvino-install-progress",
        serde_json::json!({ "type": "output", "line": line.into() }),
    );
}

fn run_command(program: &str, args: &[String], cwd: Option<&Path>) -> Result<String, String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }

    let output = cmd
        .output()
        .map_err(|e| format!("Nepodarilo se spustit {program}: {e}"))?;

    let mut combined = String::new();
    combined.push_str(&String::from_utf8_lossy(&output.stdout));
    combined.push_str(&String::from_utf8_lossy(&output.stderr));

    if !output.status.success() {
        return Err(format!(
            "{program} skoncil s kodem {:?}:\n{}",
            output.status.code(),
            combined
        ));
    }

    Ok(combined)
}

fn write_runtime_files(root: &Path) -> Result<(), String> {
    let requirements = r#"openvino>=2026.2,<2027
openvino-genai>=2026.2,<2027
openvino-tokenizers>=2026.2,<2027
huggingface-hub>=0.27
fastapi>=0.115
uvicorn[standard]>=0.32
"#;
    std::fs::write(requirements_path(root), requirements).map_err(|e| e.to_string())?;

    let smoke = r#"import json
import openvino as ov
import openvino_genai as ov_genai

core = ov.Core()
devices = core.available_devices
print(json.dumps({
    "openvino": ov.__version__,
    "openvino_genai": getattr(ov_genai, "__version__", "unknown"),
    "devices": devices,
    "has_npu": "NPU" in devices,
}))
"#;
    std::fs::write(root.join("smoke_openvino.py"), smoke).map_err(|e| e.to_string())?;

    let server = r#"import argparse
import json
import time
import uuid
from typing import Any, Optional

import openvino_genai as ov_genai
import uvicorn
from fastapi import FastAPI
from fastapi.responses import StreamingResponse
from pydantic import BaseModel


class Message(BaseModel):
    role: str
    content: str


class ChatRequest(BaseModel):
    model: Optional[str] = None
    messages: list[Message]
    max_tokens: Optional[int] = None
    temperature: Optional[float] = 0.7
    stream: Optional[bool] = True


def render_prompt(messages: list[Message]) -> str:
    lines: list[str] = []
    for message in messages:
        role = message.role.strip().lower() or "user"
        lines.append(f"{role}: {message.content}")
    lines.append("assistant:")
    return "\n".join(lines)


def make_chunk(request_id: str, model: str, content: str, finish_reason: Any = None) -> str:
    return "data: " + json.dumps({
        "id": request_id,
        "object": "chat.completion.chunk",
        "created": int(time.time()),
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {"content": content} if content else {},
            "finish_reason": finish_reason,
        }],
    }) + "\n\n"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model-dir", required=True)
    parser.add_argument("--device", default="NPU")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8091)
    args = parser.parse_args()

    pipe = ov_genai.LLMPipeline(args.model_dir, args.device)
    app = FastAPI(title="Weave OpenVINO NPU Server")
    model_id = args.model_dir.replace("\\", "/").rstrip("/").split("/")[-1] or "openvino-npu"

    @app.get("/v1/models")
    def list_models():
        return {
            "object": "list",
            "data": [{"id": model_id, "object": "model", "owned_by": "weave-openvino"}],
        }

    @app.post("/v1/chat/completions")
    def chat(req: ChatRequest):
        request_id = f"chatcmpl-{uuid.uuid4().hex}"
        prompt = render_prompt(req.messages)
        max_new_tokens = int(req.max_tokens or 512)
        temperature = float(req.temperature or 0.7)
        text = str(pipe.generate(prompt, max_new_tokens=max_new_tokens, temperature=temperature))

        if req.stream:
            def event_stream():
                yield make_chunk(request_id, model_id, text)
                yield make_chunk(request_id, model_id, "", "stop")
                yield "data: [DONE]\n\n"

            return StreamingResponse(event_stream(), media_type="text/event-stream")

        return {
            "id": request_id,
            "object": "chat.completion",
            "created": int(time.time()),
            "model": model_id,
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": text},
                "finish_reason": "stop",
            }],
        }

    uvicorn.run(app, host=args.host, port=args.port)


if __name__ == "__main__":
    main()
"#;
    std::fs::write(server_script_path(root), server).map_err(|e| e.to_string())?;

    let downloader = r#"import sys
from huggingface_hub import snapshot_download

if len(sys.argv) != 3:
    raise SystemExit("usage: download_recommended_openvino_model.py <target-dir> <repo-id>")

snapshot_download(
    repo_id=sys.argv[2],
    local_dir=sys.argv[1],
    local_dir_use_symlinks=False,
    resume_download=True,
)
"#;
    std::fs::write(model_download_script_path(root), downloader).map_err(|e| e.to_string())?;

    let readme = r#"Weave OpenVINO NPU runtime

This managed runtime installs OpenVINO, OpenVINO GenAI, OpenVINO Tokenizers,
FastAPI and Uvicorn into a private Python venv.

Server:
- needs an OpenVINO IR model directory, for example OpenVINO/Qwen3-8B-int4-cw-ov
- starts a local OpenAI-compatible server on http://localhost:8091/v1
- set Weave backend to OpenVINO NPU

The runtime smoke check is:
venv\Scripts\python.exe smoke_openvino.py
"#;
    std::fs::write(root.join("README.txt"), readme).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn get_openvino_runtime_status(app: AppHandle) -> Result<OpenvinoRuntimeStatus, String> {
    let root = openvino_dir(&app)?;
    Ok(status_for(&root).await)
}

#[tauri::command]
pub async fn list_openvino_model_profiles(
    app: AppHandle,
) -> Result<Vec<OpenvinoModelProfile>, String> {
    let root = openvino_dir(&app)?;
    Ok(openvino_model_profiles(&root))
}

#[tauri::command]
pub async fn install_openvino_runtime(
    window: Window,
    app: AppHandle,
) -> Result<OpenvinoRuntimeStatus, String> {
    let root = openvino_dir(&app)?;
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    write_runtime_files(&root)?;

    emit_step(&window, "Pripravuji Python venv pro OpenVINO").await;
    if !venv_python(&root).exists() {
        let args = if cfg!(windows) {
            vec![
                "-3".to_string(),
                "-m".to_string(),
                "venv".to_string(),
                venv_dir(&root).display().to_string(),
            ]
        } else {
            vec![
                "-m".to_string(),
                "venv".to_string(),
                venv_dir(&root).display().to_string(),
            ]
        };
        let launcher = if cfg!(windows) { "py" } else { "python3" };
        let out = run_command(launcher, &args, Some(&root))?;
        if !out.trim().is_empty() {
            emit_output(&window, out).await;
        }
    }

    emit_step(&window, "Aktualizuji pip").await;
    let out = run_command(
        &venv_python(&root).display().to_string(),
        &[
            "-m".to_string(),
            "pip".to_string(),
            "install".to_string(),
            "--upgrade".to_string(),
            "pip".to_string(),
        ],
        Some(&root),
    )?;
    emit_output(&window, out).await;

    emit_step(&window, "Instaluji OpenVINO GenAI runtime").await;
    let out = run_command(
        &venv_python(&root).display().to_string(),
        &[
            "-m".to_string(),
            "pip".to_string(),
            "install".to_string(),
            "-r".to_string(),
            requirements_path(&root).display().to_string(),
        ],
        Some(&root),
    )?;
    emit_output(&window, out).await;

    emit_step(&window, "Overuji OpenVINO a NPU plugin").await;
    let out = run_command(
        &venv_python(&root).display().to_string(),
        &[root.join("smoke_openvino.py").display().to_string()],
        Some(&root),
    )?;
    emit_output(&window, out).await;

    std::fs::write(marker_path(&root), "installed").map_err(|e| e.to_string())?;
    let _ = window.emit(
        "openvino-install-progress",
        serde_json::json!({ "type": "done" }),
    );

    Ok(status_for(&root).await)
}

#[tauri::command]
pub async fn uninstall_openvino_runtime(app: AppHandle) -> Result<(), String> {
    stop_managed_server().await?;
    let root = openvino_dir(&app)?;
    if root.exists() {
        clear_readonly_flags(&root)?;
        std::fs::remove_dir_all(&root)
            .map_err(|e| format!("Odinstalace OpenVINO runtime selhala: {e}"))?;
    }
    Ok(())
}

fn read_log_tail(path: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(path) else {
        return "OpenVINO server log zatim neni dostupny.".into();
    };
    let mut lines = text.lines().rev().take(80).collect::<Vec<_>>();
    lines.reverse();
    let tail = lines.join("\n");
    if tail.trim().is_empty() {
        "OpenVINO server log je prazdny.".into()
    } else {
        tail
    }
}

pub async fn stop_managed_server() -> Result<(), String> {
    let mut guard = server_state().lock().await;
    if let Some(mut child) = guard.take() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    Ok(())
}

#[tauri::command]
pub async fn start_openvino_runtime_server(
    app: AppHandle,
    model_dir: String,
) -> Result<OpenvinoRuntimeStatus, String> {
    let root = openvino_dir(&app)?;
    if !marker_path(&root).exists() || !venv_python(&root).exists() {
        return Err("OpenVINO runtime neni nainstalovany.".into());
    }
    write_runtime_files(&root)?;

    let model_dir = PathBuf::from(model_dir.trim());
    if !model_dir.exists() {
        return Err(format!(
            "OpenVINO model slozka neexistuje: {}",
            model_dir.display()
        ));
    }
    if !model_dir.join("openvino_model.xml").exists()
        && !model_dir.join("openvino_language_model.xml").exists()
    {
        return Err(format!(
            "Slozka nevypada jako OpenVINO IR model: {}",
            model_dir.display()
        ));
    }

    let mut guard = server_state().lock().await;
    if guard.is_some() {
        drop(guard);
        return Ok(status_for(&root).await);
    }

    let log_path = server_log_path(&root);
    let stdout = std::fs::File::create(&log_path)
        .map_err(|e| format!("Vytvoreni OpenVINO server logu selhalo: {e}"))?;
    let stderr = stdout
        .try_clone()
        .map_err(|e| format!("Priprava OpenVINO server logu selhala: {e}"))?;

    let child = tokio::process::Command::new(venv_python(&root))
        .arg(server_script_path(&root))
        .arg("--model-dir")
        .arg(&model_dir)
        .arg("--device")
        .arg(OPENVINO_DEVICE)
        .arg("--host")
        .arg(OPENVINO_SERVER_HOST)
        .arg("--port")
        .arg(OPENVINO_SERVER_PORT.to_string())
        .current_dir(&root)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|e| format!("Spusteni OpenVINO serveru selhalo: {e}"))?;
    *guard = Some(child);
    drop(guard);

    for _ in 0..180 {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        {
            let mut guard = server_state().lock().await;
            if let Some(child) = guard.as_mut() {
                if let Ok(Some(status)) = child.try_wait() {
                    *guard = None;
                    return Err(format!(
                        "OpenVINO server skoncil pred startem ({status}).\n\nPosledni radky logu ({}):\n{}",
                        log_path.display(),
                        read_log_tail(&log_path)
                    ));
                }
            }
        }

        if tokio::net::TcpStream::connect((OPENVINO_SERVER_HOST, OPENVINO_SERVER_PORT))
            .await
            .is_ok()
        {
            return Ok(status_for(&root).await);
        }
    }

    let _ = stop_managed_server().await;
    Err(format!(
        "OpenVINO server se nespustil do 180 sekund.\n\nPosledni radky logu ({}):\n{}",
        log_path.display(),
        read_log_tail(&log_path)
    ))
}

#[tauri::command]
pub async fn stop_openvino_runtime_server(app: AppHandle) -> Result<OpenvinoRuntimeStatus, String> {
    stop_managed_server().await?;
    let root = openvino_dir(&app)?;
    Ok(status_for(&root).await)
}

#[tauri::command]
pub async fn download_openvino_recommended_model(
    app: AppHandle,
) -> Result<OpenvinoRuntimeStatus, String> {
    download_openvino_model_profile(app, "qwen3-8b-int4-cw-ov".into()).await
}

#[tauri::command]
pub async fn download_openvino_model_profile(
    app: AppHandle,
    profile_id: String,
) -> Result<OpenvinoRuntimeStatus, String> {
    let root = openvino_dir(&app)?;
    if !marker_path(&root).exists() || !venv_python(&root).exists() {
        return Err("OpenVINO runtime neni nainstalovany.".into());
    }
    write_runtime_files(&root)?;

    let profile = openvino_model_profile(&root, profile_id.trim())?;
    let Some(repo_id) = profile.repo_id.clone() else {
        return Err(format!(
            "{} zatim nema automaticky stazitelny OpenVINO IR repozitar. Vyber uz pripravenou slozku OpenVINO modelu nebo pouzij Gemma 4 GGUF pres GPU/RAM backend.",
            profile.name
        ));
    };
    if !profile.auto_downloadable {
        return Err(format!(
            "{} neni oznacen jako automaticky stazitelny OpenVINO model.",
            profile.name
        ));
    }

    let target = PathBuf::from(profile.target_dir);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let python = venv_python(&root);
    let script = model_download_script_path(&root);
    let root_for_task = root.clone();
    let target_for_task = target.clone();
    tokio::task::spawn_blocking(move || {
        run_command(
            &python.display().to_string(),
            &[
                script.display().to_string(),
                target_for_task.display().to_string(),
                repo_id,
            ],
            Some(&root_for_task),
        )
    })
    .await
    .map_err(|e| e.to_string())??;

    Ok(status_for(&root).await)
}
