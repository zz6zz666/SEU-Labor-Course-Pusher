//! Opens files, folders and URLs with the shell. The process AppUserModelID and
//! DPI awareness are applied via `winkit` at startup.

use std::io::Write;
use std::process::Command;

use anyhow::{anyhow, Result};
use windows::Win32::System::Console::{
    AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE,
};

pub const APP_APP_USER_MODEL_ID: &str = "SEU.Labor.Pusher";

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Builds a `Command` that never flashes a console window. Every child process
/// spawned from this GUI app must go through here, otherwise Windows briefly
/// pops a black console window.
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Writes a line to the command line, for CLI flags like `-version`.
///
/// A GUI-subsystem (release) process started from a terminal has no stdout
/// handle, so a plain `println!` would be invisible. When that is the case we
/// attach to the parent console and write to `CONOUT$`; otherwise standard
/// output (a console, a pipe or a redirected file) is used as usual.
pub fn console_println(msg: &str) {
    let has_stdout = matches!(
        unsafe { GetStdHandle(STD_OUTPUT_HANDLE) },
        Ok(h) if !h.is_invalid() && !h.0.is_null()
    );
    if !has_stdout {
        unsafe {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open("CONOUT$") {
            let _ = f.write_all(msg.as_bytes());
            let _ = f.write_all(b"\r\n");
            return;
        }
    }
    println!("{}", msg);
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

/// Encodes `s` as a NUL-terminated UTF-16 buffer, the form Win32 wide-string
/// APIs expect.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

