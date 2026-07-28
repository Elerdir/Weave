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
const OPENVINO_DEVICE: &str = "NPU";

/// Výchozí NPU profil. Musí zůstat ten nejmenší — větší modely na části NPU
/// neprojdou kompilací a uživatel by rovnou narazil na nefunkční výchozí stav.
/// Stejná hodnota je i v `openvino-install.svelte.ts`.
const DEFAULT_PROFILE_ID: &str = "phi-3.5-mini-int4-cw-ov";

/// Poslední ručně zvolená složka s OpenVINO IR modelem. Bez uložení se po
/// restartu appky ztratila a server nešlo spustit bez opětovného vyhledání.
pub const OPENVINO_MODEL_DIR_KEY: &str = "llm.openvino_model_dir";

/// Kolik čekat na start serveru. První spuštění modelu na NPU znamená kompilaci
/// celého grafu uvnitř ovladače a ta roste s velikostí modelu — 2GB Phi naběhne
/// do minuty, ale naměřeno na 16,9GB Qwen3 32B: server byl připravený až po
/// zhruba 28 minutách. Dřív tu byla pevná tříminutová hranice, která takový
/// model zabila dávno předtím, než doběhl, a hlásila to jako selhání modelu.
///
/// 150 s na GB dává tomu naměřenému případu ještě rezervu (16 GB → 42 min).
/// Nejde o odhad ceny kompilace, ale o strop pro případ, že se proces zasekne:
/// pád se pozná okamžitě přes `try_wait`, takže velkorysost tu nic nestojí.
const SERVER_START_BASE_SECS: u64 = 180;
const SERVER_START_SECS_PER_GB: u64 = 150;
const SERVER_START_CAP_SECS: u64 = 90 * 60;

/// Jak často během čekání zalogovat, že kompilace pořád běží. Bez toho vypadá
/// dvacetiminutové čekání v logu stejně jako zamrznutá appka.
const SERVER_START_HEARTBEAT_SECS: u64 = 30;

fn dir_size_bytes(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.metadata() {
            Ok(meta) if meta.is_dir() => dir_size_bytes(&entry.path()),
            Ok(meta) => meta.len(),
            Err(_) => 0,
        })
        .sum()
}

fn server_start_timeout_secs(model_dir: &Path) -> u64 {
    let gigabytes = dir_size_bytes(model_dir) / 1_000_000_000;
    SERVER_START_BASE_SECS
        .saturating_add(SERVER_START_SECS_PER_GB.saturating_mul(gigabytes))
        .min(SERVER_START_CAP_SECS)
}

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
    root.join("models").join("phi-3.5-mini-instruct-int4-cw-ov")
}

/// NPU umí jen INT4 modely kvantované podle receptu z OpenVINO NPU guide
/// (`--sym --ratio 1.0`), a to buď kanálově (`-cw-ov`, group_size -1), nebo
/// skupinově (`-gq-ov`, group_size 128; ten navíc vyžaduje ovladač
/// 32.0.100.4023+). Běžné `-int4-ov` jsou INT4_ASYM s ratio < 1.0 a pro NPU
/// nejsou určené. Ověřeno proti HF API: takhle kvantovaných textových modelů
/// existuje jen hrstka a profily níž jsou prakticky celá ta množina.
///
/// OpenVINO sám nad 8B nic nepublikuje. Větší profily jsou komunitní konverze
/// — bereme jen ty, které mají použitelný tvar (jediný `openvino_model.xml`
/// plus tokenizer i detokenizer, symetrický nebo kanálový INT4) a jsou volně
/// stažitelné. Že je NPU zkompiluje, nikdo negarantuje, proto jsou v UI
/// označené jako experimentální a řazené podle velikosti.
///
/// Gemma tu chybí záměrně, ne omylem (ověřeno proti HF API):
///   - Gemma 4 (E2B/E4B/26B-A4B/31B) má jen `-int4-ov`, tedy skupinovou
///     kvantizaci (INT4_ASYM, group_size 128) — kanálově kvantovaná verze
///     neexistuje. Navíc je celá rodina multimodální (`image-text-to-text`:
///     repo nemá `openvino_model.xml`, ale rozpad na language/text/vision
///     embeddings), takže ji `LLMPipeline` nenačte, a 31B váží 18,7 GB.
///   - Gemma 3 4B `-int4-cw-ov` kanálově kvantovaná je, ale taky multimodální,
///     takže by vyžadovala `VLMPipeline`.
///
/// Gemma 4 je proto v GGUF katalogu pro CUDA, ne tady.
///
/// Pořadí = doporučení: menší modely NPU zkompiluje spolehlivěji,
/// 7B/8B na části NPU spadne v Level Zero compileru už při startu.
fn openvino_model_profiles(root: &Path) -> Vec<OpenvinoModelProfile> {
    vec![
        OpenvinoModelProfile {
            id: "phi-3.5-mini-int4-cw-ov".into(),
            name: "Phi-3.5 mini Instruct INT4 (3,8B)".into(),
            description: "Nejspolehlivejsi volba pro NPU — mala a rychle se zkompiluje. Zacni tudy a over, ze NPU inference vubec bezi. Cestina je slabsi nez u vetsich modelu.".into(),
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
            size_hint: "3,8B INT4 / ~2 GB".into(),
            quality_tier: "Doporuceno pro NPU".into(),
        },
        OpenvinoModelProfile {
            id: "phi-3-mini-4k-int4-cw-ov".into(),
            name: "Phi-3 mini 4k Instruct INT4 (3,8B)".into(),
            description: "Starsi sourozenec Phi-3.5 se stejnou velikosti. Zaloha, kdyz Phi-3.5 na tvem NPU z nejakeho duvodu neprojde kompilaci.".into(),
            target_dir: root
                .join("models")
                .join("phi-3-mini-4k-instruct-int4-cw-ov")
                .display()
                .to_string(),
            repo_id: Some("OpenVINO/Phi-3-mini-4k-instruct-int4-cw-ov".into()),
            source_url: Some(
                "https://huggingface.co/OpenVINO/Phi-3-mini-4k-instruct-int4-cw-ov".into(),
            ),
            auto_downloadable: true,
            size_hint: "3,8B INT4 / ~2 GB".into(),
            quality_tier: "Zalozni mala varianta".into(),
        },
        OpenvinoModelProfile {
            id: "mistral-7b-v03-int4-cw-ov".into(),
            name: "Mistral 7B Instruct v0.3 INT4".into(),
            description: "Vetsi model s lepsi kvalitou i cestinou. Na slabsich NPU uz nemusi projit kompilaci — kdyz start skonci chybou Level Zero compileru, vrat se k Phi-3.5.".into(),
            target_dir: root
                .join("models")
                .join("mistral-7b-instruct-v0.3-int4-cw-ov")
                .display()
                .to_string(),
            repo_id: Some("OpenVINO/Mistral-7B-Instruct-v0.3-int4-cw-ov".into()),
            source_url: Some(
                "https://huggingface.co/OpenVINO/Mistral-7B-Instruct-v0.3-int4-cw-ov".into(),
            ),
            auto_downloadable: true,
            size_hint: "7B INT4 / ~4 GB".into(),
            quality_tier: "Vyssi kvalita, narocnejsi na NPU".into(),
        },
        OpenvinoModelProfile {
            id: "qwen3-8b-int4-cw-ov".into(),
            name: "Qwen3 8B INT4 (nejlepsi cestina)".into(),
            description: "Nejvetsi overeny model od OpenVINO a nejlepsi cestina z bezpecne casti nabidky — Qwen3 je trenovany napric 119 jazyky, zatimco Phi je silne anglocentricke. Potrebuje ovladac 32.0.100.4023 nebo novejsi, na starsim start spadne v Level Zero compileru.".into(),
            target_dir: root
                .join("models")
                .join("qwen3-8b-int4-cw-ov")
                .display()
                .to_string(),
            repo_id: Some("OpenVINO/Qwen3-8B-int4-cw-ov".into()),
            source_url: Some("https://huggingface.co/OpenVINO/Qwen3-8B-int4-cw-ov".into()),
            auto_downloadable: true,
            size_hint: "8B INT4 / ~4,75 GB".into(),
            quality_tier: "Nejlepsi overena cestina".into(),
        },
        // --- Nad 8B uz nic neni od OpenVINO. Profily niz jsou komunitni
        // konverze, ktere maji spravny tvar (jediny openvino_model.xml plus
        // tokenizer, symetricky nebo kanalovy INT4), ale nikdo u nich NPU
        // negarantuje. Poradi je podle velikosti, protoze prave ta rozhoduje,
        // jestli je Level Zero compiler jeste zvladne prelozit.
        OpenvinoModelProfile {
            id: "qwen3-14b-int4-sym-ov".into(),
            name: "Qwen3 14B INT4 (experimentalni)".into(),
            description: "Nejrozumnejsi krok nahoru za Qwen3 8B: stejne silna cestina, vic znalosti a lepsi uvazovani. Komunitni symetricka INT4 konverze — tvar modelu je pro NPU spravny, ale nikdo to na NPU neoveril. Pocitej s delsi prvni kompilaci.".into(),
            target_dir: root
                .join("models")
                .join("qwen3-14b-int4-sym-ov")
                .display()
                .to_string(),
            repo_id: Some("Echo9Zulu/Qwen3-14B-int4_sym-ov".into()),
            source_url: Some("https://huggingface.co/Echo9Zulu/Qwen3-14B-int4_sym-ov".into()),
            auto_downloadable: true,
            size_hint: "14B INT4 / ~8,4 GB".into(),
            quality_tier: "Vyssi kvalita, neovereno na NPU".into(),
        },
        OpenvinoModelProfile {
            id: "gpt-oss-20b-int4-cw-ov".into(),
            name: "gpt-oss 20B INT4 (experimentalni)".into(),
            description: "MoE model — 20B parametru, ale na token jich pracuje jen zlomek, takze bezi svizne i na vetsi velikost. Jediny model nad 8B, ktery je kanalove kvantovany, tedy presne tak, jak to NPU chce. Cestina je slabsi nez u Qwen3, sila je v uvazovani a kodu.".into(),
            target_dir: root
                .join("models")
                .join("gpt-oss-20b-int4-cw-ov")
                .display()
                .to_string(),
            repo_id: Some("keitokei1994/gpt-oss-20b-int4-cw-ov".into()),
            source_url: Some("https://huggingface.co/keitokei1994/gpt-oss-20b-int4-cw-ov".into()),
            auto_downloadable: true,
            size_hint: "20B MoE INT4 / ~11,1 GB".into(),
            quality_tier: "Nejlepsi kvantizace pro NPU, slabsi cestina".into(),
        },
        OpenvinoModelProfile {
            id: "qwen3-32b-int4-sym-awq-ov".into(),
            name: "Qwen3 32B INT4 (maximum, casto nezkompilovatelny)".into(),
            description: "Nejsilnejsi model, ktery pro NPU vubec existuje — vyborna cestina a znalosti. Bud ale realisticky: 18 GB je hodne za tim, co Level Zero compiler bezne zvladne, takze pocitej s tim, ze start muze skoncit chybou. Stahuj, jen kdyz to chces zkusit; na jistotu zustan u Qwen3 8B nebo 14B.".into(),
            target_dir: root
                .join("models")
                .join("qwen3-32b-int4-sym-awq-ov")
                .display()
                .to_string(),
            repo_id: Some("Echo9Zulu/Qwen3-32B-Instruct-int4_sym-awq-ov".into()),
            source_url: Some(
                "https://huggingface.co/Echo9Zulu/Qwen3-32B-Instruct-int4_sym-awq-ov".into(),
            ),
            auto_downloadable: true,
            size_hint: "32B INT4 / ~18,2 GB".into(),
            quality_tier: "Maximum, vysoke riziko nezkompilovani".into(),
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
/// `openvino_model.xml`, multimodální (např. Gemma 3) `openvino_language_model.xml`.
fn looks_like_openvino_ir(dir: &Path) -> bool {
    dir.join("openvino_model.xml").exists() || dir.join("openvino_language_model.xml").exists()
}

async fn status_for(root: &Path, pool: &SqlitePool) -> OpenvinoRuntimeStatus {
    let saved_model_dir = weave_infrastructure::db::app_config::get(pool, OPENVINO_MODEL_DIR_KEY)
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
        device_check: read_device_check(root),
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

/// Kolik posledních řádků výstupu přiložit k chybě, když příkaz spadne.
const ERROR_TAIL_LINES: usize = 20;
/// Průběhové rámce (pip/tqdm překreslují řádek přes `\r`) chodí i stokrát za
/// sekundu — bez škrcení by zahltily IPC kanál do UI.
const PROGRESS_THROTTLE: std::time::Duration = std::time::Duration::from_millis(150);

/// Čte stream po bajtech a posílá hotové úseky do okna. Dělí na `\n` i `\r`:
/// pip i huggingface_hub kreslí průběh přepisováním řádku přes `\r`, takže na
/// samotné `\n` by se čekalo až do konce celého stahování.
async fn pump_output<R>(
    window: Window,
    reader: R,
    tail: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<String>>>,
) where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;

    let mut reader = reader;
    let mut buf = [0u8; 4096];
    let mut line = String::new();
    let mut last_progress = std::time::Instant::now() - PROGRESS_THROTTLE;

    loop {
        let n = match reader.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        for ch in String::from_utf8_lossy(&buf[..n]).chars() {
            match ch {
                '\n' | '\r' => {
                    let text = line.trim_end().to_string();
                    line.clear();
                    if text.is_empty() {
                        continue;
                    }
                    // Skutečné řádky logu posíláme vždy, průběhové rámce škrtíme.
                    let is_progress = ch == '\r';
                    if is_progress && last_progress.elapsed() < PROGRESS_THROTTLE {
                        continue;
                    }
                    if is_progress {
                        last_progress = std::time::Instant::now();
                    } else {
                        let mut guard = tail.lock().expect("tail mutex poisoned");
                        if guard.len() >= ERROR_TAIL_LINES {
                            guard.pop_front();
                        }
                        guard.push_back(text.clone());
                    }
                    emit_output(&window, text).await;
                }
                _ => line.push(ch),
            }
        }
    }

    let text = line.trim_end().to_string();
    if !text.is_empty() {
        emit_output(&window, text).await;
    }
}

/// Spustí příkaz a streamuje výstup živě do okna. `run_command` s `.output()`
/// bufferuje všechno až do konce procesu — během několikaminutového
/// `pip install` nebo stahování modelu tak UI nedostalo ani řádek a vypadalo
/// zaseknutě. Chyba nese posledních pár řádků výstupu, ne jen exit kód.
async fn run_command_streamed(
    window: &Window,
    program: &str,
    args: &[String],
    cwd: Option<&Path>,
) -> Result<(), String> {
    run_command_streamed_env(window, program, args, cwd, &[]).await
}

/// Jako `run_command_streamed`, ale s doplněnými proměnnými prostředí —
/// tudy se předává HF token, aby se neobjevil v argumentech (a tím i v logu).
async fn run_command_streamed_env(
    window: &Window,
    program: &str,
    args: &[String],
    cwd: Option<&Path>,
    env: &[(&str, String)],
) -> Result<(), String> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (key, value) in env {
        cmd.env(key, value);
    }
    weave_infrastructure::spawn::hide_console(&mut cmd);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Nepodarilo se spustit {program}: {e}"))?;
    let stdout = child.stdout.take().expect("stdout je piped");
    let stderr = child.stderr.take().expect("stderr je piped");

    let tail = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
    let out_task = tokio::spawn(pump_output(window.clone(), stdout, tail.clone()));
    let err_task = tokio::spawn(pump_output(window.clone(), stderr, tail.clone()));

    let status = child
        .wait()
        .await
        .map_err(|e| format!("{program} selhal: {e}"))?;
    let _ = out_task.await;
    let _ = err_task.await;

    if !status.success() {
        let context: Vec<String> = tail
            .lock()
            .expect("tail mutex poisoned")
            .iter()
            .cloned()
            .collect();
        let suffix = if context.is_empty() {
            String::new()
        } else {
            format!("\n---\n{}", context.join("\n"))
        };
        return Err(format!(
            "{program} skoncil s kodem {:?}{suffix}",
            status.code()
        ));
    }
    Ok(())
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
import os
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
    # NPU kompiluje model uvnitr ovladace a u velkych modelu to trva desitky
    # minut. CACHE_DIR ulozi zkompilovany blob, takze druhy a dalsi start je
    # otazka sekund. Bez nej se cela kompilace opakovala pri kazdem spusteni.
    cache_dir = os.path.join(args.model_dir, ".ov-cache")
    os.makedirs(cache_dir, exist_ok=True)
    cached = os.listdir(cache_dir)
    if cached:
        print(f"Nacitam model {args.model_dir} na {args.device} z cache ...", flush=True)
    else:
        print(
            f"Nacitam model {args.model_dir} na {args.device} ... "
            "prvni start kompiluje model v ovladaci NPU a u velkych modelu "
            "muze trvat i desitky minut. Dalsi starty uz budou rychle.",
            flush=True,
        )
    pipe = ov_genai.LLMPipeline(args.model_dir, args.device, CACHE_DIR=cache_dir)
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
    // Token bereme z prostředí (HF_TOKEN), ne z argumentu — v argumentech by
    // se objevil ve výpisu procesů i ve streamovaném logu. Bez něj se gated
    // repozitáře (Gemma) stáhnou jen zpola: README projde, váhy ne, a chyba
    // se dřív ztratila, takže složka vypadala „stažená".
    let downloader = r#"import os
import sys
from huggingface_hub import snapshot_download

if len(sys.argv) != 3:
    raise SystemExit("usage: download_recommended_openvino_model.py <target-dir> <repo-id>")

token = os.environ.get("HF_TOKEN") or None
snapshot_download(repo_id=sys.argv[2], local_dir=sys.argv[1], token=token)
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
    tracing::info!(target: "openvino", dir = %root.display(), "Instalace OpenVINO runtime zahajena");

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
        run_command_streamed(&window, launcher, &args, Some(&root)).await?;
    }

    emit_step(&window, "Aktualizuji pip").await;
    run_command_streamed(
        &window,
        &venv_python(&root).display().to_string(),
        &[
            "-m".to_string(),
            "pip".to_string(),
            "install".to_string(),
            "--upgrade".to_string(),
            "pip".to_string(),
        ],
        Some(&root),
    )
    .await?;

    emit_step(
        &window,
        "Instaluji OpenVINO GenAI runtime (stahuje stovky MB, muze trvat minuty)",
    )
    .await;
    run_command_streamed(
        &window,
        &venv_python(&root).display().to_string(),
        &[
            "-m".to_string(),
            "pip".to_string(),
            "install".to_string(),
            "-r".to_string(),
            requirements_path(&root).display().to_string(),
        ],
        Some(&root),
    )
    .await?;

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
                tracing::info!(
                    target: "openvino",
                    devices = %check.devices.join(", "),
                    "NPU nalezeno"
                );
                emit_step(
                    &window,
                    format!("NPU nalezeno (zarizeni: {})", check.devices.join(", ")),
                )
                .await;
            } else {
                tracing::warn!(
                    target: "openvino",
                    devices = %check.devices.join(", "),
                    "NPU nenalezeno, server nepujde spustit"
                );
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
            tracing::warn!(target: "openvino", "Overeni zarizeni nevratilo ocekavany vystup");
            emit_step(
                &window,
                "VAROVANI: overeni zarizeni nevratilo ocekavany vystup",
            )
            .await;
        }
    }

    std::fs::write(marker_path(&root), "installed").map_err(|e| e.to_string())?;
    tracing::info!(target: "openvino", dir = %root.display(), "OpenVINO runtime nainstalovan");
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
    if !root.exists() {
        return Ok(());
    }
    clear_readonly_flags(&root)?;

    // Windows drží zámky na .exe/.dll ještě chvíli po zabití procesu (a python
    // si spouští vlastní potomky), takže remove_dir_all hned po kill() umí
    // spadnout na "access denied". Pár pokusů s pauzou to spolehlivě vyřeší;
    // bez toho zůstala odinstalace viset s nicneříkající chybou.
    let mut last_err = None;
    for attempt in 0..5 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            // Potomci mohli mezitím vytvořit další soubory (např. __pycache__).
            clear_readonly_flags(&root)?;
        }
        match std::fs::remove_dir_all(&root) {
            Ok(()) => return Ok(()),
            Err(e) => last_err = Some(e),
        }
    }

    Err(format!(
        "Odinstalace OpenVINO runtime selhala: {}. Slozka {} je nejspis \
         drzena jinym procesem — zavri pripadny bezici python/OpenVINO server \
         a zkus to znovu.",
        last_err.map(|e| e.to_string()).unwrap_or_default(),
        root.display()
    ))
}

/// Přeloží typické pády NPU startu do věty, se kterou jde něco udělat.
/// Bez toho uživatel dostal jen zeď C++ výjimek z Level Zero compileru.
fn npu_failure_hint(log_tail: &str) -> &'static str {
    let lower = log_tail.to_lowercase();
    if lower.contains("ze_result_error_invalid_argument")
        || lower.contains("compilation failed")
        || lower.contains("failed to create executable")
    {
        return "\n\nNPU nedokazalo model zkompilovat. Nejcasteji je vinik ZASTARALY OVLADAC \
                NPU: kompilace probiha az v ovladaci, takze stary ovladac neumi graf z \
                novejsiho OpenVINO. Zkontroluj verzi v Nastaveni -> AI model (sekce NPU \
                zarizeni) a stahni aktualni z \
                https://www.intel.com/content/www/us/en/download/794734/intel-npu-driver-windows.html \
                Az kdyz je ovladac aktualni a chyba trva, zkus mensi profil (Phi-3.5 mini).";
    }
    if lower.contains("openvino_language_model.xml") || lower.contains("vlmpipeline") {
        return "\n\nVypada to na multimodalni (obrazkovy) model — NPU server umi jen \
                textove modely. Vyber jiny profil.";
    }
    if lower.contains("401") || lower.contains("403") || lower.contains("gated") {
        return "\n\nModel je na HuggingFace gated. Prijmi jeho licenci na HuggingFace a \
                vloz HF token v Nastaveni -> API klice.";
    }
    ""
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
        tracing::info!(target: "openvino", pid = ?child.id(), "Zastavuji NPU server");
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    Ok(())
}

#[tauri::command]
pub async fn start_openvino_runtime_server(
    app: AppHandle,
    model_dir: String,
    state: State<'_, AppState>,
) -> Result<OpenvinoRuntimeStatus, String> {
    start_server_inner(&app, &state.pool, model_dir).await
}

pub(crate) async fn start_server_inner(
    app: &AppHandle,
    pool: &SqlitePool,
    model_dir: String,
) -> Result<OpenvinoRuntimeStatus, String> {
    tracing::info!(target: "openvino", model_dir = %model_dir.trim(), "Pozadavek na start NPU serveru");
    let root = openvino_dir(app)?;
    if !marker_path(&root).exists() || !venv_python(&root).exists() {
        tracing::warn!(target: "openvino", "Start odmitnut: runtime neni nainstalovany");
        return Err("OpenVINO runtime neni nainstalovany.".into());
    }
    write_runtime_files(&root)?;

    // Načtení modelu na NPU trvá minuty — chybějící NPU má smysl ohlásit hned,
    // ne až Python tracebackem v logu.
    if let Some(check) = read_device_check(&root) {
        if !check.has_npu {
            return Err(format!(
                "Tento pocitac nema dostupne NPU (OpenVINO vidi: {}). \
                 Zkontroluj ovladac NPU a spust instalaci runtime znovu, \
                 nebo pouzij GPU/RAM backend.",
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
        .arg(OPENVINO_DEVICE)
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

    let timeout_secs = server_start_timeout_secs(&model_dir);
    tracing::info!(
        target: "openvino",
        model_dir = %model_dir.display(),
        device = OPENVINO_DEVICE,
        timeout_secs,
        "NPU server spusten, cekam na kompilaci modelu v ovladaci"
    );

    for elapsed in 1..=timeout_secs {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        {
            let mut guard = server_state().lock().await;
            if let Some(child) = guard.as_mut() {
                if let Ok(Some(status)) = child.try_wait() {
                    *guard = None;
                    let tail = read_log_tail(&log_path);
                    tracing::error!(
                        target: "openvino",
                        %status,
                        elapsed_secs = elapsed,
                        log = %log_path.display(),
                        "NPU server skoncil pred startem"
                    );
                    return Err(format!(
                        "OpenVINO server skoncil pred startem ({status}).{}\n\nPosledni radky logu ({}):\n{}",
                        npu_failure_hint(&tail),
                        log_path.display(),
                        tail
                    ));
                }
            }
        }

        if tokio::net::TcpStream::connect((OPENVINO_SERVER_HOST, OPENVINO_SERVER_PORT))
            .await
            .is_ok()
        {
            tracing::info!(
                target: "openvino",
                elapsed_secs = elapsed,
                model_dir = %model_dir.display(),
                "NPU server je pripraveny"
            );
            // Server běží → cestu k modelu si zapamatujeme, aby ji uživatel
            // po restartu appky nemusel hledat znovu.
            let _ = weave_infrastructure::db::app_config::set(
                pool,
                OPENVINO_MODEL_DIR_KEY,
                &model_dir.display().to_string(),
            )
            .await;
            return Ok(status_for(&root, pool).await);
        }

        if elapsed % SERVER_START_HEARTBEAT_SECS == 0 {
            tracing::info!(
                target: "openvino",
                elapsed_secs = elapsed,
                timeout_secs,
                "NPU porad kompiluje model"
            );
        }
    }

    let _ = stop_managed_server().await;
    tracing::error!(
        target: "openvino",
        timeout_secs,
        log = %log_path.display(),
        "NPU server se nespustil v limitu"
    );
    Err(format!(
        "OpenVINO server se nespustil do {timeout_secs} sekund.\n\nPosledni radky logu ({}):\n{}",
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
    window: Window,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<OpenvinoRuntimeStatus, String> {
    download_openvino_model_profile(window, app, DEFAULT_PROFILE_ID.into(), state).await
}

#[tauri::command]
pub async fn download_openvino_model_profile(
    window: Window,
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
    tracing::info!(
        target: "openvino",
        profile = %profile.id,
        repo = %repo_id,
        size_hint = %profile.size_hint,
        dir = %target.display(),
        "Stahuji NPU model"
    );
    // Stahování má jednotky GB — bez streamovaného průběhu (dřív se volalo
    // bufferovaně a bez jediného eventu) vypadalo UI celé minuty zaseknuté.
    emit_step(&window, format!("Stahuji {} ({repo_id})", profile.name)).await;
    // Gated repozitáře (Gemma a spol.) bez tokenu stáhnou jen README a mlčky
    // skončí — pokud uživatel HF token v Nastavení má, použijeme ho.
    let hf_token = state
        .keychain
        .retrieve(&weave_application::ports::keychain_port::ApiService::HuggingFace)
        .await
        .ok()
        .flatten()
        .filter(|t| !t.trim().is_empty());
    let env: Vec<(&str, String)> = match hf_token {
        Some(token) => vec![("HF_TOKEN", token)],
        None => Vec::new(),
    };

    let download_result = run_command_streamed_env(
        &window,
        &venv_python(&root).display().to_string(),
        &[
            model_download_script_path(&root).display().to_string(),
            target.display().to_string(),
            repo_id,
        ],
        Some(&root),
        &env,
    )
    .await;
    let _ = window.emit(
        "openvino-install-progress",
        serde_json::json!({ "type": "done" }),
    );
    if let Err(err) = &download_result {
        tracing::error!(target: "openvino", profile = %profile.id, %err, "Stahovani NPU modelu selhalo");
    }
    download_result?;

    if !looks_like_openvino_ir(&target) {
        tracing::error!(
            target: "openvino",
            dir = %target.display(),
            "Stazena slozka neobsahuje openvino_model.xml"
        );
        return Err(format!(
            "Stazena slozka {} neobsahuje OpenVINO IR model (openvino_model.xml). \
             Stahovani nejspis skoncilo predcasne — zkus to znovu.",
            target.display()
        ));
    }

    tracing::info!(
        target: "openvino",
        profile = %profile.id,
        dir = %target.display(),
        size_bytes = dir_size_bytes(&target),
        "NPU model stazeny"
    );
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
    fn default_profile_id_resolves() {
        let root = Path::new("C:/weave/openvino");
        // Na tohle ID padá `download_openvino_recommended_model` i fallback ve storu.
        let profile = openvino_model_profile(root, DEFAULT_PROFILE_ID).expect("výchozí profil");
        assert!(profile.auto_downloadable);
        assert!(openvino_model_profile(root, "neexistuje").is_err());
    }

    #[test]
    fn default_profile_is_the_smallest_one() {
        // Regrese: výchozí byl Qwen3 8B, který na řadě NPU neprojde kompilací —
        // uživatel tak narazil na nefunkční stav hned napoprvé.
        let profiles = openvino_model_profiles(Path::new("C:/weave/openvino"));
        assert_eq!(
            profiles.first().expect("aspoň jeden profil").id,
            DEFAULT_PROFILE_ID,
            "výchozí profil musí být první v seznamu (nejmenší = nejspolehlivější)"
        );
    }

    #[test]
    fn server_start_timeout_grows_with_model_size() {
        // Regrese: čekalo se pevných 180 s. Kompilace grafu v NPU ovladači
        // roste s velikostí modelu, takže 18GB model byl useknutý dřív, než
        // vůbec mohl naběhnout, a hlásilo se to jako selhání.
        let small = temp_dir("timeout_small");
        std::fs::write(small.join("openvino_model.bin"), vec![0u8; 1024]).expect("maly model");
        assert_eq!(
            server_start_timeout_secs(&small),
            SERVER_START_BASE_SECS,
            "u modelu pod 1 GB zustava zakladni limit"
        );

        let big = temp_dir("timeout_big");
        std::fs::write(big.join("openvino_model.bin"), vec![0u8; 3_000_000_000]).expect("velky");
        assert_eq!(
            server_start_timeout_secs(&big),
            SERVER_START_BASE_SECS + 3 * SERVER_START_SECS_PER_GB
        );

        // Neexistující složka nesmí panikařit ani vrátit nulu — jinak by se
        // smyčka ukončila hned a server by se tvářil jako mrtvý.
        assert_eq!(
            server_start_timeout_secs(Path::new("C:/weave/neexistuje")),
            SERVER_START_BASE_SECS
        );

        let _ = std::fs::remove_dir_all(&small);
        let _ = std::fs::remove_dir_all(&big);
    }

    #[test]
    fn large_profiles_are_offered_after_the_verified_ones() {
        // Uživatel chtěl i velké modely. Riziko nezkompilování roste s
        // velikostí, takže pořadí (a tím i doporučení v UI) musí zůstat
        // vzestupné: ověřené profily od OpenVINO napřed, komunitní velké až
        // za nimi. Kdyby někdo velký model omylem posunul nahoru nebo z něj
        // udělal výchozí, chytí to tenhle test spolu s tím nad ním.
        let profiles = openvino_model_profiles(Path::new("C:/weave/openvino"));
        let ids: Vec<&str> = profiles.iter().map(|p| p.id.as_str()).collect();
        let position = |id: &str| {
            ids.iter()
                .position(|candidate| *candidate == id)
                .unwrap_or_else(|| panic!("chybí profil {id}"))
        };
        assert!(position("qwen3-8b-int4-cw-ov") < position("qwen3-14b-int4-sym-ov"));
        assert!(position("qwen3-14b-int4-sym-ov") < position("gpt-oss-20b-int4-cw-ov"));
        assert!(position("gpt-oss-20b-int4-cw-ov") < position("qwen3-32b-int4-sym-awq-ov"));
    }

    #[test]
    fn npu_failure_hint_recognises_compiler_and_gating_errors() {
        assert!(npu_failure_hint(
            "Compilation failed. Level0 pfnCreate2 result: ZE_RESULT_ERROR_INVALID_ARGUMENT"
        )
        .contains("mensi profil"));
        assert!(
            npu_failure_hint("401 Client Error: Unauthorized, repo is gated").contains("HF token")
        );
        assert!(npu_failure_hint("vse v poradku, server bezi").is_empty());
    }
}
