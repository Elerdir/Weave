use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter, Manager, State, Window};
use tokio::process::Child;
use tokio::sync::Mutex;

use crate::state::AppState;

const OPENVINO_SERVER_PORT: u16 = 8091;
const OPENVINO_SERVER_HOST: &str = "127.0.0.1";
/// Výchozí zařízení, když si uživatel žádné nezvolil. NPU je preferované
/// (nízkopříkonové), ale na strojích se starým ovladačem selže — pak jde
/// v UI přepnout na GPU (Intel Arc) nebo CPU.
const OPENVINO_DEFAULT_DEVICE: &str = "NPU";

/// Poslední ručně zvolená složka s OpenVINO IR modelem. Bez uložení se po
/// restartu appky ztratila a server nešlo spustit bez opětovného vyhledání.
pub const OPENVINO_MODEL_DIR_KEY: &str = "llm.openvino_model_dir";

/// Naposledy zvolené OpenVINO zařízení (NPU/GPU/CPU); přežije restart appky.
pub const OPENVINO_DEVICE_KEY: &str = "llm.openvino_device";

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
    /// Naposledy zvolená složka modelu (přežije restart appky); prázdné,
    /// dokud uživatel nespustil server.
    pub saved_model_dir: String,
    /// Naposledy zvolené zařízení (NPU/GPU/CPU); přežije restart appky.
    /// Prázdné = ještě nezvoleno, UI použije výchozí NPU.
    pub saved_device: String,
    /// Výsledek posledního ověření OpenVINO zařízení při instalaci.
    /// `None` = runtime ještě nebyl ověřen.
    pub device_check: Option<OpenvinoDeviceCheck>,
}

/// Co OpenVINO na tomhle stroji vidí za zařízení. Bez NPU v seznamu nemá
/// smysl NPU server vůbec spouštět — dřív to skončilo až Python tracebackem
/// v logu po několikaminutovém načítání modelu.
/// Serializuje se do camelCase pro frontend, ale `alias` musí zůstat —
/// smoke skript je Python a píše `has_npu` ve snake_case.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenvinoDeviceCheck {
    #[serde(default)]
    pub devices: Vec<String>,
    #[serde(default, alias = "has_npu")]
    pub has_npu: bool,
    #[serde(default)]
    pub openvino: String,
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
            id: "mistral-7b-instruct-v0.3-int4-cw-ov".into(),
            name: "Mistral 7B Instruct v0.3 INT4".into(),
            description: "Silny 7B model s dobrou cestinou. Vhodna alternativa, kdyz je Qwen3 8B na NPU pomaly.".into(),
            target_dir: root
                .join("models")
                .join("Mistral-7B-Instruct-v0.3-int4-cw-ov")
                .display()
                .to_string(),
            repo_id: Some("OpenVINO/Mistral-7B-Instruct-v0.3-int4-cw-ov".into()),
            source_url: Some(
                "https://huggingface.co/OpenVINO/Mistral-7B-Instruct-v0.3-int4-cw-ov".into(),
            ),
            auto_downloadable: true,
            size_hint: "7B INT4 / ~3,8 GB".into(),
            quality_tier: "Vyvazeny pomer kvalita/rychlost".into(),
        },
        OpenvinoModelProfile {
            id: "deepseek-r1-distill-qwen-7b-nf4-ov".into(),
            name: "DeepSeek-R1-Distill-Qwen 7B NF4".into(),
            description: "Model s durazem na uvazovani (reasoning). POZOR: kvantizace nf4 nejde na NPU (\"Unsupported data type 'nf4'\") — vyber v Zarizeni GPU nebo CPU. Odpovedi jsou pomalejsi, protoze model nejdriv premysli.".into(),
            target_dir: root
                .join("models")
                .join("DeepSeek-R1-Distill-Qwen-7B-nf4-ov")
                .display()
                .to_string(),
            repo_id: Some("OpenVINO/DeepSeek-R1-Distill-Qwen-7B-nf4-ov".into()),
            source_url: Some(
                "https://huggingface.co/OpenVINO/DeepSeek-R1-Distill-Qwen-7B-nf4-ov".into(),
            ),
            auto_downloadable: true,
            size_hint: "7B NF4 / ~4,4 GB".into(),
            quality_tier: "Uvazovani — jen GPU/CPU".into(),
        },
        OpenvinoModelProfile {
            id: "phi-3.5-mini-int4-cw-ov".into(),
            name: "Phi-3.5 mini Instruct INT4".into(),
            description: "Nejmensi a nejrychlejsi profil -- dobry na prvni overeni, ze NPU inference vubec bezi. Cestina je slabsi nez u Qwen3.".into(),
            target_dir: root
                .join("models")
                .join("phi-3.5-mini-instruct-int4-cw-ov")
                .display()
                .to_string(),
            repo_id: Some("OpenVINO/Phi-3.5-mini-instruct-int4-cw-ov".into()),
            source_url: Some(
                "https://huggingface.co/OpenVINO/Phi-3.5-mini-instruct-int4-cw-ov".into(),
            ),
            auto_downloadable: true,
            size_hint: "3,8B INT4 / nejrychlejsi start".into(),
            quality_tier: "Rychly test NPU".into(),
        },
    ]
}

fn openvino_model_profile(root: &Path, profile_id: &str) -> Result<OpenvinoModelProfile, String> {
    openvino_model_profiles(root)
        .into_iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| format!("Neznamy OpenVINO model profil: {profile_id}"))
}

fn device_check_path(root: &Path) -> PathBuf {
    root.join("device-check.json")
}

pub async fn is_server_running() -> bool {
    server_state().lock().await.is_some()
}

/// Vytáhne JSON řádek ze smoke skriptu. Pip a Python můžou před něj vypsat
/// varování, proto se hledá od konce první řádek, který se povede rozparsovat.
fn parse_device_check(output: &str) -> Option<OpenvinoDeviceCheck> {
    output
        .lines()
        .rev()
        .map(str::trim)
        .filter(|line| line.starts_with('{'))
        .find_map(|line| serde_json::from_str::<OpenvinoDeviceCheck>(line).ok())
}

fn read_device_check(root: &Path) -> Option<OpenvinoDeviceCheck> {
    let text = std::fs::read_to_string(device_check_path(root)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Vypadá složka jako OpenVINO IR model? Textové modely mají
/// `openvino_model.xml`, multimodální (např. Gemma 3/4) `openvino_language_model.xml`.
fn looks_like_openvino_ir(dir: &Path) -> bool {
    dir.join("openvino_model.xml").exists() || dir.join("openvino_language_model.xml").exists()
}

/// Umí server tenhle model spustit? Server staví na `ov_genai.LLMPipeline`, která
/// zvládá jen textové modely. Multimodální IR (Gemma 3/4 — `openvino_language_model.xml`
/// + vision embeddings) by potřeboval `VLMPipeline`; s LLMPipeline se načte, ale
/// generování spadne na „Port for tensor name input_ids was not found".
fn is_text_only_ir(dir: &Path) -> bool {
    dir.join("openvino_model.xml").exists()
}

async fn status_for(root: &Path, pool: &SqlitePool) -> OpenvinoRuntimeStatus {
    let saved_model_dir = weave_infrastructure::db::app_config::get(pool, OPENVINO_MODEL_DIR_KEY)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

    let saved_device = weave_infrastructure::db::app_config::get(pool, OPENVINO_DEVICE_KEY)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

    OpenvinoRuntimeStatus {
        installed: marker_path(root).exists() && venv_python(root).exists(),
        server_running: is_server_running().await,
        install_dir: root.display().to_string(),
        python_path: venv_python(root).display().to_string(),
        requirements_path: requirements_path(root).display().to_string(),
        server_log_path: server_log_path(root).display().to_string(),
        default_model_dir: default_model_dir(root).display().to_string(),
        saved_model_dir,
        saved_device,
        device_check: read_device_check(root),
    }
}

/// Vidí OpenVINO na tomhle stroji zvolené zařízení? `GPU` matchne i `GPU.0`
/// (stejná logika jako `ensure_device` v Python serveru).
fn device_available(check: &OpenvinoDeviceCheck, device: &str) -> bool {
    check
        .devices
        .iter()
        .any(|name| name == device || name.starts_with(&format!("{device}.")))
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
    weave_infrastructure::spawn::hide_console_std(&mut cmd);
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

/// Blokující příkaz (venv/pip, klidně minuty) spuštěný mimo async runtime —
/// `run_command` volaný přímo v tauri commandu by po dobu instalace blokoval
/// tokio worker vlákno.
async fn run_command_async(
    program: String,
    args: Vec<String>,
    cwd: Option<PathBuf>,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || run_command(&program, &args, cwd.as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

fn write_runtime_files(root: &Path) -> Result<(), String> {
    // huggingface-hub je omezený zdola i shora: 1.x odstranil `resume_download`
    // a `local_dir_use_symlinks`, na kterých dřív stahování padalo. Skript je
    // dnes volá bez nich (funguje na 0.30+ i 1.x), horní mez chrání před
    // dalším breaking change ve 2.x.
    let requirements = r#"openvino>=2026.2,<2027
openvino-genai>=2026.2,<2027
openvino-tokenizers>=2026.2,<2027
huggingface-hub>=0.30,<2
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
import queue
import threading
import time
import uuid
from typing import Any, Optional

import openvino as ov
import openvino_genai as ov_genai
import uvicorn
from fastapi import FastAPI
from fastapi.responses import StreamingResponse
from pydantic import BaseModel

# LLMPipeline neni thread-safe a uvicorn obsluhuje sync endpointy ve
# vlaknovem poolu -- soubezne requesty musi cekat, jinak se generovani
# navzajem poskodi.
PIPE_LOCK = threading.Lock()


class Message(BaseModel):
    role: str
    content: str


class ChatRequest(BaseModel):
    model: Optional[str] = None
    messages: list[Message]
    max_tokens: Optional[int] = None
    temperature: Optional[float] = 0.7
    top_p: Optional[float] = None
    stream: Optional[bool] = True


def build_inputs(messages: list[Message]):
    """Preferuje ov_genai.ChatHistory -- pipeline na ni sama aplikuje chat
    sablonu modelu (<|im_start|> u Qwen, <|user|> u Phi ...). Rucne skladany
    text 'user: ...' sablonu obchazi, model pak nepozna konec odpovedi.
    Fallback je jen pro starsi runtime bez ChatHistory."""
    try:
        history = ov_genai.ChatHistory()
        for message in messages:
            role = (message.role or "user").strip().lower()
            history.append({"role": role, "content": message.content})
        return history
    except Exception:
        lines = [f"{(m.role or 'user').strip().lower()}: {m.content}" for m in messages]
        lines.append("assistant:")
        return "\n".join(lines)


def build_config(req: ChatRequest) -> "ov_genai.GenerationConfig":
    config = ov_genai.GenerationConfig()
    config.max_new_tokens = int(req.max_tokens or 512)
    temperature = float(req.temperature if req.temperature is not None else 0.7)
    # Bez do_sample je dekodovani greedy a teplota se tise ignoruje.
    if temperature > 0.0:
        config.do_sample = True
        config.temperature = temperature
        if req.top_p is not None:
            config.top_p = float(req.top_p)
    else:
        config.do_sample = False
    return config


def keep_streaming():
    status = getattr(ov_genai, "StreamingStatus", None)
    return status.RUNNING if status is not None else False


def make_chunk(
    request_id: str,
    model: str,
    content: str,
    finish_reason: Any = None,
    usage: Optional[dict] = None,
) -> str:
    payload: dict[str, Any] = {
        "id": request_id,
        "object": "chat.completion.chunk",
        "created": int(time.time()),
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {"content": content} if content else {},
            "finish_reason": finish_reason,
        }],
    }
    if usage is not None:
        payload["usage"] = usage
    return "data: " + json.dumps(payload) + "\n\n"


def ensure_device(device: str) -> None:
    available = ov.Core().available_devices
    if not any(name == device or name.startswith(f"{device}.") for name in available):
        raise SystemExit(
            f"Zarizeni {device} neni v tomto systemu dostupne. "
            f"OpenVINO vidi: {', '.join(available) or 'nic'}. "
            "Zkontroluj, ze pocitac ma NPU a nainstalovany ovladac."
        )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model-dir", required=True)
    parser.add_argument("--device", default="NPU")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8091)
    args = parser.parse_args()

    ensure_device(args.device)
    print(f"Nacitam model {args.model_dir} na {args.device} ...", flush=True)
    pipe = ov_genai.LLMPipeline(args.model_dir, args.device)
    print("Model nacten, spoustim server.", flush=True)

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
        inputs = build_inputs(req.messages)
        config = build_config(req)

        if not req.stream:
            with PIPE_LOCK:
                text = str(pipe.generate(inputs, config))
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

        # Skutecny token-by-token streaming: generovani bezi ve vlastnim
        # vlakne a streamer callback plni frontu, ze ktere SSE generator
        # rovnou posila kousky klientovi.
        def event_stream():
            tokens: "queue.Queue[Optional[str]]" = queue.Queue()
            failure: list[BaseException] = []

            def on_token(subword: str):
                tokens.put(subword)
                return keep_streaming()

            def run_generation():
                try:
                    with PIPE_LOCK:
                        pipe.generate(inputs, config, on_token)
                except BaseException as exc:  # noqa: BLE001 - hlasime klientovi
                    failure.append(exc)
                finally:
                    tokens.put(None)

            worker = threading.Thread(target=run_generation, daemon=True)
            started = time.time()
            worker.start()

            emitted = 0
            while True:
                item = tokens.get()
                if item is None:
                    break
                emitted += 1
                yield make_chunk(request_id, model_id, item)
            worker.join()

            if failure:
                yield make_chunk(
                    request_id, model_id, f"\n[chyba generovani: {failure[0]}]", "stop"
                )
            else:
                usage = {
                    "prompt_tokens": 0,
                    "completion_tokens": emitted,
                    "total_tokens": emitted,
                }
                yield make_chunk(request_id, model_id, "", "stop", usage)
            print(
                f"generovani hotovo: {emitted} tokenu za {time.time() - started:.1f}s",
                flush=True,
            )
            yield "data: [DONE]\n\n"

        return StreamingResponse(event_stream(), media_type="text/event-stream")

    uvicorn.run(app, host=args.host, port=args.port)


if __name__ == "__main__":
    main()
"#;
    std::fs::write(server_script_path(root), server).map_err(|e| e.to_string())?;

    // POZOR: `local_dir_use_symlinks` ani `resume_download` se sem nesmí vrátit —
    // huggingface_hub 1.x je odstranil a volání padalo na TypeError. Stahování
    // do `local_dir` dnes navazuje na rozdělané soubory samo.
    let downloader = r#"import sys
from huggingface_hub import snapshot_download

if len(sys.argv) != 3:
    raise SystemExit("usage: download_recommended_openvino_model.py <target-dir> <repo-id>")

snapshot_download(repo_id=sys.argv[2], local_dir=sys.argv[1])
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
pub async fn get_openvino_runtime_status(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<OpenvinoRuntimeStatus, String> {
    let root = openvino_dir(&app)?;
    Ok(status_for(&root, &state.pool).await)
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
    state: State<'_, AppState>,
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
        let out = run_command_async(launcher.to_string(), args, Some(root.clone())).await?;
        if !out.trim().is_empty() {
            emit_output(&window, out).await;
        }
    }

    emit_step(&window, "Aktualizuji pip").await;
    let out = run_command_async(
        venv_python(&root).display().to_string(),
        vec![
            "-m".to_string(),
            "pip".to_string(),
            "install".to_string(),
            "--upgrade".to_string(),
            "pip".to_string(),
        ],
        Some(root.clone()),
    )
    .await?;
    emit_output(&window, out).await;

    emit_step(&window, "Instaluji OpenVINO GenAI runtime").await;
    let out = run_command_async(
        venv_python(&root).display().to_string(),
        vec![
            "-m".to_string(),
            "pip".to_string(),
            "install".to_string(),
            "-r".to_string(),
            requirements_path(&root).display().to_string(),
        ],
        Some(root.clone()),
    )
    .await?;
    emit_output(&window, out).await;

    emit_step(&window, "Overuji OpenVINO a NPU plugin").await;
    let out = run_command_async(
        venv_python(&root).display().to_string(),
        vec![root.join("smoke_openvino.py").display().to_string()],
        Some(root.clone()),
    )
    .await?;
    emit_output(&window, out.clone()).await;

    // Výsledek uložíme, aby UI mohlo hned říct, jestli NPU vůbec existuje.
    // Dřív se jen vypsal do logu a nikdo ho nečetl — uživatel bez NPU pak
    // stahoval gigabajty modelu, který nešlo spustit.
    match parse_device_check(&out) {
        Some(check) => {
            if let Ok(json) = serde_json::to_string(&check) {
                let _ = std::fs::write(device_check_path(&root), json);
            }
            if check.has_npu {
                emit_step(
                    &window,
                    format!("NPU nalezeno (zarizeni: {})", check.devices.join(", ")),
                )
                .await;
            } else {
                emit_step(
                    &window,
                    format!(
                        "VAROVANI: NPU nenalezeno. OpenVINO vidi jen: {}. \
                         Bez NPU se server nespusti — zkontroluj ovladac NPU.",
                        check.devices.join(", ")
                    ),
                )
                .await;
            }
        }
        None => {
            let _ = std::fs::remove_file(device_check_path(&root));
            emit_step(
                &window,
                "VAROVANI: overeni zarizeni nevratilo ocekavany vystup",
            )
            .await;
        }
    }

    std::fs::write(marker_path(&root), "installed").map_err(|e| e.to_string())?;
    let _ = window.emit(
        "openvino-install-progress",
        serde_json::json!({ "type": "done" }),
    );

    Ok(status_for(&root, &state.pool).await)
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
    device: Option<String>,
    state: State<'_, AppState>,
) -> Result<OpenvinoRuntimeStatus, String> {
    start_server_inner(&app, &state.pool, model_dir, device).await
}

pub(crate) async fn start_server_inner(
    app: &AppHandle,
    pool: &SqlitePool,
    model_dir: String,
    device: Option<String>,
) -> Result<OpenvinoRuntimeStatus, String> {
    let root = openvino_dir(app)?;
    if !marker_path(&root).exists() || !venv_python(&root).exists() {
        return Err("OpenVINO runtime neni nainstalovany.".into());
    }
    write_runtime_files(&root)?;

    // Zařízení: explicitní volba > uložená > výchozí NPU.
    let device = match device.map(|d| d.trim().to_string()).filter(|d| !d.is_empty()) {
        Some(d) => d,
        None => weave_infrastructure::db::app_config::get(pool, OPENVINO_DEVICE_KEY)
            .await
            .ok()
            .flatten()
            .filter(|d| !d.trim().is_empty())
            .unwrap_or_else(|| OPENVINO_DEFAULT_DEVICE.to_string()),
    };

    // Načtení modelu trvá minuty — nedostupné zařízení má smysl ohlásit hned,
    // ne až Python tracebackem v logu po dlouhé kompilaci. U NPU je to typicky
    // starý ovladač; pak jde v UI přepnout na GPU (Intel Arc) nebo CPU.
    if let Some(check) = read_device_check(&root) {
        if !device_available(&check, &device) {
            return Err(format!(
                "Zarizeni {device} neni v tomto systemu dostupne (OpenVINO vidi: {}). \
                 Vyber jine zarizeni; u NPU zkontroluj ovladac Intel AI Boost.",
                check.devices.join(", ")
            ));
        }
    }

    let model_dir = PathBuf::from(model_dir.trim());
    if !model_dir.exists() {
        return Err(format!(
            "OpenVINO model slozka neexistuje: {}",
            model_dir.display()
        ));
    }
    if !looks_like_openvino_ir(&model_dir) {
        return Err(format!(
            "Slozka nevypada jako OpenVINO IR model: {}",
            model_dir.display()
        ));
    }
    if !is_text_only_ir(&model_dir) {
        return Err(format!(
            "Model {} je multimodalni (openvino_language_model.xml). Server umi zatim jen \
             textove modely — vyber model s openvino_model.xml, napriklad Qwen3 8B.",
            model_dir.display()
        ));
    }

    let mut guard = server_state().lock().await;
    if guard.is_some() {
        drop(guard);
        return Ok(status_for(&root, pool).await);
    }

    // Port obsazený cizím procesem (typicky osiřelý server po pádu appky):
    // bez téhle kontroly se čekací smyčka níž připojí na *starý* server,
    // vrátí „běží" a uživatel by mluvil s jiným modelem, než si vybral.
    if tokio::net::TcpStream::connect((OPENVINO_SERVER_HOST, OPENVINO_SERVER_PORT))
        .await
        .is_ok()
    {
        drop(guard);
        return Err(format!(
            "Port {OPENVINO_SERVER_PORT} uz pouziva jiny proces — nejspis OpenVINO server, \
             ktery zustal bezet po predchozim spusteni Weave. Ukonci ho ve Sprave uloh \
             (python.exe) a zkus to znovu."
        ));
    }

    let log_path = server_log_path(&root);
    let stdout = std::fs::File::create(&log_path)
        .map_err(|e| format!("Vytvoreni OpenVINO server logu selhalo: {e}"))?;
    let stderr = stdout
        .try_clone()
        .map_err(|e| format!("Priprava OpenVINO server logu selhala: {e}"))?;

    let mut cmd = tokio::process::Command::new(venv_python(&root));
    cmd.arg(server_script_path(&root))
        .arg("--model-dir")
        .arg(&model_dir)
        .arg("--device")
        .arg(&device)
        .arg("--host")
        .arg(OPENVINO_SERVER_HOST)
        .arg("--port")
        .arg(OPENVINO_SERVER_PORT.to_string())
        .current_dir(&root)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    weave_infrastructure::spawn::hide_console(&mut cmd);
    let child = cmd
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
            // Server běží → cestu k modelu i zvolené zařízení si zapamatujeme,
            // aby je uživatel po restartu appky nemusel nastavovat znovu.
            let _ = weave_infrastructure::db::app_config::set(
                pool,
                OPENVINO_MODEL_DIR_KEY,
                &model_dir.display().to_string(),
            )
            .await;
            let _ =
                weave_infrastructure::db::app_config::set(pool, OPENVINO_DEVICE_KEY, &device).await;
            return Ok(status_for(&root, pool).await);
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
pub async fn stop_openvino_runtime_server(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<OpenvinoRuntimeStatus, String> {
    stop_managed_server().await?;
    let root = openvino_dir(&app)?;
    Ok(status_for(&root, &state.pool).await)
}

#[tauri::command]
pub async fn download_openvino_recommended_model(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<OpenvinoRuntimeStatus, String> {
    download_openvino_model_profile(app, "qwen3-8b-int4-cw-ov".into(), state).await
}

#[tauri::command]
pub async fn download_openvino_model_profile(
    app: AppHandle,
    profile_id: String,
    state: State<'_, AppState>,
) -> Result<OpenvinoRuntimeStatus, String> {
    let root = openvino_dir(&app)?;
    if !marker_path(&root).exists() || !venv_python(&root).exists() {
        return Err("OpenVINO runtime neni nainstalovany.".into());
    }
    write_runtime_files(&root)?;

    let profile = openvino_model_profile(&root, profile_id.trim())?;
    let Some(repo_id) = profile.repo_id.clone() else {
        return Err(format!(
            "{} nema automaticky stazitelny OpenVINO IR repozitar. Vyber rucne pripravenou slozku OpenVINO modelu.",
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
    let source_url = profile.source_url.clone().unwrap_or_default();
    run_command_async(
        venv_python(&root).display().to_string(),
        vec![
            model_download_script_path(&root).display().to_string(),
            target.display().to_string(),
            repo_id,
        ],
        Some(root.clone()),
    )
    .await
    // Gated repo (napr. Google licence u Gemma) vraci syrovy Python traceback,
    // ze ktereho uzivatel nepozna, ze staci odsouhlasit licenci na HuggingFace.
    .map_err(|e| {
        if e.contains("gated repo") || e.contains("is restricted") {
            format!(
                "Model {} je na HuggingFace uzamceny (gated) — je potreba se prihlasit \
                 a odsouhlasit licenci na {source_url}, pak slozku vybrat rucne pres \
                 „Procházet\". Puvodni chyba:\n{e}",
                profile.name
            )
        } else {
            e
        }
    })?;

    if !looks_like_openvino_ir(&target) {
        return Err(format!(
            "Stazena slozka {} neobsahuje OpenVINO IR model (openvino_model.xml). \
             Stahovani nejspis skoncilo predcasne — zkus to znovu.",
            target.display()
        ));
    }

    Ok(status_for(&root, &state.pool).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "weave_openvino_test_{name}_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn device_check_is_read_from_last_json_line() {
        // Pip a Python si před JSON často přisypou varování — parser musí
        // vzít poslední rozparsovatelný řádek, ne první.
        let output = "UserWarning: something deprecated\n\
                      {\"openvino\": \"2026.2.1\", \"devices\": [\"CPU\", \"GPU\", \"NPU\"], \"has_npu\": true}\n";
        let check = parse_device_check(output).expect("JSON se má najít");
        assert!(check.has_npu);
        assert_eq!(check.devices, vec!["CPU", "GPU", "NPU"]);
        assert_eq!(check.openvino, "2026.2.1");
    }

    #[test]
    fn device_check_reports_missing_npu() {
        let output =
            "{\"openvino\": \"2026.2.1\", \"devices\": [\"CPU\", \"GPU\"], \"has_npu\": false}";
        let check = parse_device_check(output).expect("JSON se má najít");
        assert!(!check.has_npu);
        assert_eq!(check.devices, vec!["CPU", "GPU"]);
    }

    #[test]
    fn device_check_is_none_without_parsable_json() {
        assert!(parse_device_check("").is_none());
        assert!(parse_device_check("Traceback (most recent call last):").is_none());
        assert!(parse_device_check("{nevalidni json").is_none());
    }

    #[test]
    fn openvino_ir_detected_for_text_and_multimodal_layouts() {
        let text_model = temp_dir("ir_text");
        std::fs::write(text_model.join("openvino_model.xml"), "<net/>").unwrap();
        assert!(looks_like_openvino_ir(&text_model));

        // Gemma 3 a další multimodální modely mají jazykovou část zvlášť.
        let multimodal = temp_dir("ir_multimodal");
        std::fs::write(multimodal.join("openvino_language_model.xml"), "<net/>").unwrap();
        assert!(looks_like_openvino_ir(&multimodal));

        // Nedostažená složka (jen tokenizer) se nesmí tvářit jako hotový model.
        let incomplete = temp_dir("ir_incomplete");
        std::fs::write(incomplete.join("tokenizer.json"), "{}").unwrap();
        assert!(!looks_like_openvino_ir(&incomplete));

        for dir in [text_model, multimodal, incomplete] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn every_profile_target_dir_is_unique_and_downloadable_profiles_have_repo() {
        let root = Path::new("C:/weave/openvino");
        let profiles = openvino_model_profiles(root);

        let mut dirs: Vec<&str> = profiles.iter().map(|p| p.target_dir.as_str()).collect();
        dirs.sort_unstable();
        let count = dirs.len();
        dirs.dedup();
        assert_eq!(dirs.len(), count, "profily nesmí sdílet cílovou složku");

        for profile in &profiles {
            // Profil označený jako automaticky stažitelný musí mít repo_id,
            // jinak tlačítko „Stáhnout" spadne až za běhu.
            assert_eq!(
                profile.auto_downloadable,
                profile.repo_id.is_some(),
                "profil {} má nekonzistentní auto_downloadable/repo_id",
                profile.id
            );
        }
    }

    #[test]
    fn multimodal_ir_is_not_text_only() {
        // Server staví na LLMPipeline — multimodální model (Gemma 3/4) se sice
        // tváří jako platné IR, ale generování na něm padá. Musí jít odlišit,
        // aby uživatel dostal srozumitelnou hlášku, ne „input_ids not found".
        let text_model = temp_dir("text_only");
        std::fs::write(text_model.join("openvino_model.xml"), "<net/>").unwrap();
        assert!(looks_like_openvino_ir(&text_model));
        assert!(is_text_only_ir(&text_model));

        let multimodal = temp_dir("multimodal");
        std::fs::write(multimodal.join("openvino_language_model.xml"), "<net/>").unwrap();
        std::fs::write(multimodal.join("openvino_vision_embeddings_model.xml"), "<net/>").unwrap();
        assert!(looks_like_openvino_ir(&multimodal));
        assert!(!is_text_only_ir(&multimodal), "multimodální IR není text-only");

        for dir in [text_model, multimodal] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn shipped_profiles_are_text_only_and_ungated() {
        // Gemma 3 4B se z nabídky odstranila: na HuggingFace je gated (auto-download
        // spadne) a je multimodální, takže by ji LLMPipeline stejně nespustila.
        let root = Path::new("C:/weave/openvino");
        let profiles = openvino_model_profiles(root);
        assert!(
            !profiles.iter().any(|p| p.id.contains("gemma")),
            "gated/multimodální Gemma profil se nesmí vrátit do nabídky"
        );
        assert!(profiles.len() >= 3, "nabídka má mít aspoň tři modely");
    }

    #[test]
    fn default_profile_id_resolves() {
        let root = Path::new("C:/weave/openvino");
        // Na tohle ID padá `download_openvino_recommended_model` i fallback ve storu.
        let profile = openvino_model_profile(root, "qwen3-8b-int4-cw-ov").expect("výchozí profil");
        assert!(profile.auto_downloadable);
        assert!(openvino_model_profile(root, "neexistuje").is_err());
    }
}
