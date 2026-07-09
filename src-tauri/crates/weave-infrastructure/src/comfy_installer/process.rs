use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use weave_application::{
    error::{AppError, AppResult},
    ports::comfy_installer_port::InstallProgress,
};

/// Kolik posledních řádků stdout+stderr přiložit k chybě při selhání příkazu —
/// dřív se hlásil jen exit kód (např. "pip install torch ... kód 1") bez
/// skutečné pip chybové hlášky, což selhání nešlo diagnostikovat bez ručního
/// zopakování příkazu.
const ERROR_TAIL_LINES: usize = 20;

/// Spustí příkaz, streamuje stdout+stderr řádek po řádku do `tx` jako
/// `InstallProgress::Output` (dlouhotrvající kroky typu `pip install torch`
/// by jinak vypadaly jako zamrzlé), a vrátí chybu při nenulovém exit kódu.
pub async fn run_streamed(
    program: &str,
    args: &[&str],
    cwd: Option<&std::path::Path>,
    tx: &mpsc::Sender<InstallProgress>,
) -> AppResult<()> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    crate::spawn::hide_console(&mut cmd);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::ComfyUi(format!("Nelze spustit {program}: {e}")))?;

    let stdout = child.stdout.take().expect("stdout je piped");
    let stderr = child.stderr.take().expect("stderr je piped");

    let tail: Arc<Mutex<VecDeque<String>>> =
        Arc::new(Mutex::new(VecDeque::with_capacity(ERROR_TAIL_LINES + 1)));

    let tx_out = tx.clone();
    let tail_out = tail.clone();
    let out_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            push_tail(&tail_out, &line);
            let _ = tx_out.send(InstallProgress::Output(line)).await;
        }
    });

    let tx_err = tx.clone();
    let tail_err = tail.clone();
    let err_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            push_tail(&tail_err, &line);
            let _ = tx_err.send(InstallProgress::Output(line)).await;
        }
    });

    let status = child
        .wait()
        .await
        .map_err(|e| AppError::ComfyUi(format!("{program} selhal: {e}")))?;
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
        return Err(AppError::ComfyUi(format!(
            "{program} {} skončil s chybou (kód {:?}){suffix}",
            args.join(" "),
            status.code(),
        )));
    }
    Ok(())
}

fn push_tail(tail: &Arc<Mutex<VecDeque<String>>>, line: &str) {
    let mut guard = tail.lock().expect("tail mutex poisoned");
    if guard.len() >= ERROR_TAIL_LINES {
        guard.pop_front();
    }
    guard.push_back(line.to_string());
}

/// Najde funkční Python 3 interpret (python / python3 / launcher `py -3`).
pub fn find_system_python() -> Option<String> {
    for candidate in ["python3", "python", "py"] {
        let args: &[&str] = if candidate == "py" {
            &["-3", "--version"]
        } else {
            &["--version"]
        };
        let mut probe = std::process::Command::new(candidate);
        probe.args(args);
        crate::spawn::hide_console_std(&mut probe);
        if let Ok(output) = probe.output() {
            if output.status.success() {
                let text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                if is_python3(&text) {
                    return Some(candidate.to_string());
                }
            }
        }
    }
    None
}

fn is_python3(version_output: &str) -> bool {
    version_output
        .split_whitespace()
        .find_map(|tok| tok.strip_prefix("3."))
        .is_some()
        || version_output.contains("Python 3")
}

/// Cesta k Python interpretu uvnitř virtuálního prostředí (liší se Win/Unix).
pub fn venv_python_path(venv_dir: &std::path::Path) -> std::path::PathBuf {
    if cfg!(windows) {
        venv_dir.join("Scripts").join("python.exe")
    } else {
        venv_dir.join("bin").join("python")
    }
}

/// Detekuje NVIDIA GPU přes nvidia-smi (bez závislosti na jiném portu).
pub fn has_nvidia_gpu() -> bool {
    let mut cmd = std::process::Command::new("nvidia-smi");
    cmd.arg("--query-gpu=name").arg("--format=csv,noheader");
    crate::spawn::hide_console_std(&mut cmd);
    cmd.output().map(|o| o.status.success()).unwrap_or(false)
}

/// Stáhne soubor s progress reportingem po ~5% krocích do `InstallProgress::Output`
/// (stejný stream+write vzor jako `model_manager.rs`, ale napojený na instalační log
/// místo vlastního progress kanálu). Idempotentní — pokud `dest` už existuje, nic nedělá.
/// Stahuje do `.part` a až po úspěšném dokončení přejmenuje, aby napůl stažený soubor
/// nevypadal jako hotový při přerušené instalaci.
pub async fn download_file(
    http: &reqwest::Client,
    url: &str,
    dest: &std::path::Path,
    label: &str,
    tx: &mpsc::Sender<InstallProgress>,
) -> AppResult<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    if dest.exists() {
        return Ok(());
    }

    let tmp_dest = dest.with_extension("part");
    let tx = tx.clone();
    let label = label.to_string();
    let last_bucket = AtomicU64::new(u64::MAX);
    // Fallback pro servery bez Content-Length (`total == 0`): bez něj se
    // procentuální report nikdy nespustí a stahování vypadá jako zaseklé,
    // proto hlásíme aspoň průběžně stažené MB podle času.
    let last_text_report = Mutex::new(Instant::now() - Duration::from_secs(2));

    crate::parallel_download::download(http, url, &tmp_dest, move |downloaded, total| {
        if let Some(pct) = downloaded
            .checked_mul(100)
            .and_then(|n| n.checked_div(total))
        {
            let bucket = pct / 5;
            if last_bucket.swap(bucket, Ordering::Relaxed) != bucket {
                let _ = tx.try_send(InstallProgress::Output(format!(
                    "{label}: {pct}% ({:.1}/{:.1} GB)",
                    downloaded as f64 / 1e9,
                    total as f64 / 1e9
                )));
            }
        } else {
            let mut last = last_text_report.lock().expect("last_text_report poisoned");
            if last.elapsed() >= Duration::from_secs(2) {
                *last = Instant::now();
                let _ = tx.try_send(InstallProgress::Output(format!(
                    "{label}: {:.1} GB staženo",
                    downloaded as f64 / 1e9
                )));
            }
        }
    })
    .await
    .map_err(AppError::ComfyUi)?;

    std::fs::rename(&tmp_dest, dest).map_err(|e| AppError::ComfyUi(e.to_string()))?;
    Ok(())
}

/// Rozbalí .zip do cílové složky — na rozdíl od PyTorch/PuLID (pip/git) se
/// InsightFace AntelopeV2 distribuuje jen jako zip archiv.
pub fn extract_zip(zip_path: &std::path::Path, dest_dir: &std::path::Path) -> AppResult<()> {
    let file = std::fs::File::open(zip_path).map_err(|e| AppError::ComfyUi(e.to_string()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| AppError::ComfyUi(e.to_string()))?;
    archive
        .extract(dest_dir)
        .map_err(|e| AppError::ComfyUi(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_python3_detects_version_string() {
        assert!(is_python3("Python 3.10.11"));
        assert!(is_python3("Python 3.12.0"));
        assert!(!is_python3("Python 2.7.18"));
        assert!(!is_python3(""));
    }

    #[test]
    fn venv_python_path_differs_by_platform() {
        let venv = std::path::Path::new("/tmp/venv");
        let path = venv_python_path(venv);
        if cfg!(windows) {
            assert!(path.ends_with("Scripts/python.exe") || path.ends_with("Scripts\\python.exe"));
        } else {
            assert!(path.ends_with("bin/python"));
        }
    }

    fn unique_temp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "weave_process_test_{name}_{}",
            uuid::Uuid::new_v4()
        ))
    }

    fn drain_channel(mut rx: mpsc::Receiver<InstallProgress>) {
        tokio::spawn(async move { while rx.recv().await.is_some() {} });
    }

    #[tokio::test]
    async fn download_file_writes_response_body_to_dest() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello world".to_vec()))
            .mount(&server)
            .await;

        let dest = unique_temp_dir("download_ok").join("file.bin");
        let (tx, rx) = mpsc::channel(16);
        drain_channel(rx);

        download_file(
            &reqwest::Client::new(),
            &server.uri(),
            &dest,
            "test soubor",
            &tx,
        )
        .await
        .unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), b"hello world");
        assert!(!dest.with_extension("part").exists());
    }

    #[tokio::test]
    async fn download_file_skips_when_dest_already_exists() {
        let dest = unique_temp_dir("download_skip").join("file.bin");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(&dest, b"already here").unwrap();

        let (tx, rx) = mpsc::channel(16);
        drain_channel(rx);

        // Neexistující port — pokud by funkce (chybně) zkusila stahovat i přes
        // existující cíl, spojení by selhalo a test by spadl na chybě, ne na assertu.
        download_file(
            &reqwest::Client::new(),
            "http://127.0.0.1:1/unreachable",
            &dest,
            "test soubor",
            &tx,
        )
        .await
        .unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), b"already here");
    }

    #[tokio::test]
    async fn download_file_propagates_http_error_status() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let dest = unique_temp_dir("download_404").join("file.bin");
        let (tx, rx) = mpsc::channel(16);
        drain_channel(rx);

        let result = download_file(
            &reqwest::Client::new(),
            &server.uri(),
            &dest,
            "test soubor",
            &tx,
        )
        .await;

        assert!(result.is_err());
        assert!(!dest.exists());
    }

    #[test]
    fn extract_zip_preserves_nested_directory_structure() {
        use std::io::Write;

        let src_dir = unique_temp_dir("zip_src");
        std::fs::create_dir_all(&src_dir).unwrap();
        let zip_path = src_dir.join("archive.zip");

        let file = std::fs::File::create(&zip_path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        writer
            .start_file("antelopev2/glintr100.onnx", options)
            .unwrap();
        writer.write_all(b"fake-onnx-bytes").unwrap();
        writer.finish().unwrap();

        let dest_dir = unique_temp_dir("zip_dest");
        extract_zip(&zip_path, &dest_dir).unwrap();

        let extracted = dest_dir.join("antelopev2").join("glintr100.onnx");
        assert!(extracted.exists());
        assert_eq!(std::fs::read(&extracted).unwrap(), b"fake-onnx-bytes");
    }
}
