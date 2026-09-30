//! Opens files, folders and URLs with the shell, plus process-level Windows
//! setup (DPI awareness, AppUserModelID).

use std::process::Command;

use anyhow::{anyhow, Result};
use windows::core::PCWSTR;
use windows::Win32::UI::HiDpi::{
    GetDpiForSystem, SetProcessDpiAwareness, SetProcessDpiAwarenessContext,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, PROCESS_DPI_AWARENESS,
};
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows::Win32::UI::WindowsAndMessaging::SetProcessDPIAware;

pub const APP_APP_USER_MODEL_ID: &str = "SEU.Labor.Pusher";

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Builds a `Command` that never flashes a console window. Every child process
/// spawned from this GUI app must go through here, otherwise Windows briefly
/// pops a black console window.
pub fn command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

pub fn open_target(target: &str) -> Result<()> {
    command("rundll32")
        .arg("url.dll,FileProtocolHandler")
        .arg(target)
        .spawn()?;
    Ok(())
}

pub fn open_folder(dir: &str) -> Result<()> {
    command("explorer").arg(dir).spawn()?;
    Ok(())
}

pub fn open_url(url: &str) -> Result<()> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(anyhow!("拒绝打开非 http(s) 链接"));
    }
    open_target(url)
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Sets the process AppUserModelID, required for desktop toast notifications to
/// be attributed to this app.
pub fn set_app_user_model_id(id: &str) -> Result<()> {
    let w = to_wide(id);
    unsafe { SetCurrentProcessExplicitAppUserModelID(PCWSTR(w.as_ptr())) }
        .map_err(|e| anyhow!("设置 AppUserModelID 失败: {e}"))
}

/// Reports the primary display scale factor (1.0 == 96 DPI).
pub fn dpi_scale() -> f64 {
    let d = unsafe { GetDpiForSystem() };
    if d >= 96 {
        d as f64 / 96.0
    } else {
        1.0
    }
}

/// Opts the process into Per-Monitor V2 DPI awareness so windows render crisply
/// on scaled (high-DPI) displays. Must be called before any window is created.
pub fn enable_per_monitor_dpi() {
    unsafe {
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_ok() {
            return;
        }
        if SetProcessDpiAwareness(PROCESS_DPI_AWARENESS(2)).is_ok() {
            return;
        }
        let _ = SetProcessDPIAware();
    }
}
