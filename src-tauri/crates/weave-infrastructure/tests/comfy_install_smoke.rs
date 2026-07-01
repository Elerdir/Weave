//! Manuální smoke test reálné instalace ComfyUI + PuLID. Nikdy neběží v CI
//! (#[ignore] — vyžaduje Python/Git na stroji a stahuje gigabajty dat).
//!
//! Spouštět ručně: cargo test -p weave-infrastructure --test comfy_install_smoke
//!   -- --ignored --nocapture

use tokio::sync::mpsc;
use weave_application::ports::comfy_installer_port::{
    ComfyInstallerPort, ComfyStatus, InstallProgress,
};
use weave_infrastructure::comfy_installer::LocalComfyInstaller;

#[tokio::test]
#[ignore = "stahuje gigabajty dat (PyTorch, ComfyUI, PuLID), spouštět ručně"]
async fn installs_comfyui_and_pulid_end_to_end() {
    let dir = std::env::temp_dir().join("weave_comfy_smoke_test");
    let installer = LocalComfyInstaller::new(dir.clone());

    assert_eq!(installer.status().await.unwrap(), ComfyStatus::NotInstalled);

    let (tx, mut rx) = mpsc::channel(256);
    let log_task = tokio::spawn(async move {
        while let Some(progress) = rx.recv().await {
            match progress {
                InstallProgress::Step { name } => println!("=== {name} ==="),
                InstallProgress::Output(line) => println!("{line}"),
                InstallProgress::Done => println!("=== HOTOVO ==="),
                InstallProgress::Error(e) => println!("=== CHYBA: {e} ==="),
            }
        }
    });

    installer.install(tx).await.expect("instalace selhala");
    log_task.await.ok();

    assert_eq!(installer.status().await.unwrap(), ComfyStatus::Installed);
    assert!(dir.join("main.py").exists());
    assert!(dir.join("custom_nodes/PuLID_ComfyUI").exists());
    assert!(dir
        .join("models/checkpoints/sd_xl_base_1.0.safetensors")
        .exists());
    assert!(dir
        .join("models/pulid/ip-adapter_pulid_sdxl_fp16.safetensors")
        .exists());
    assert!(dir.join("models/insightface/models/antelopev2").is_dir());

    println!("Spouštím server pro ověření health checku...");
    installer
        .start_server()
        .await
        .expect("start serveru selhal");
    assert_eq!(installer.status().await.unwrap(), ComfyStatus::Running);

    installer.stop_server().await.expect("stop serveru selhal");
}
