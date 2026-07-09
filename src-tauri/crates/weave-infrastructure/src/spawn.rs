//! Pomůcky pro spouštění podprocesů.
//!
//! Weave je na Windows GUI aplikace (subsystém "windows") — každý spawnutý
//! *konzolový* proces (python, pip, git, nvidia-smi, ComfyUI/OpenVINO server)
//! by bez `CREATE_NO_WINDOW` vyhodil vlastní blikající konzolové okno.
//! V dev režimu (`pnpm tauri dev`) to vidět není, v release buildu ano.

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Skryje konzolové okno spawnutého procesu (tokio varianta). Na ne-Windows no-op.
pub fn hide_console(cmd: &mut tokio::process::Command) -> &mut tokio::process::Command {
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// Skryje konzolové okno spawnutého procesu (std varianta). Na ne-Windows no-op.
pub fn hide_console_std(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
