use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use weave_application::{
    error::{AppError, AppResult},
    ports::comfy_installer_port::InstallProgress,
};

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
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::ComfyUi(format!("Nelze spustit {program}: {e}")))?;

    let stdout = child.stdout.take().expect("stdout je piped");
    let stderr = child.stderr.take().expect("stderr je piped");

    let tx_out = tx.clone();
    let out_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx_out.send(InstallProgress::Output(line)).await;
        }
    });

    let tx_err = tx.clone();
    let err_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
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
        return Err(AppError::ComfyUi(format!(
            "{program} {} skončil s chybou (kód {:?})",
            args.join(" "),
            status.code()
        )));
    }
    Ok(())
}

/// Najde funkční Python 3 interpret (python / python3 / launcher `py -3`).
pub fn find_system_python() -> Option<String> {
    for candidate in ["python3", "python", "py"] {
        let args: &[&str] = if candidate == "py" {
            &["-3", "--version"]
        } else {
            &["--version"]
        };
        if let Ok(output) = std::process::Command::new(candidate).args(args).output() {
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
    std::process::Command::new("nvidia-smi")
        .arg("--query-gpu=name")
        .arg("--format=csv,noheader")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
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
}
