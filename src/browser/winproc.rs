//! Native, console-free process management for our dedicated browser profiles.
//!
//! Chromium hands a new launch off to an existing process that is already using
//! the same `--user-data-dir`; that process then exits without a debugging
//! port, which would break us. Before launching we therefore terminate any
//! browser process bound to our profile. This is done with Win32 APIs only
//! (Toolhelp snapshot + command-line query), so nothing flashes a console.

use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
};

use super::discovery::is_browser_exe;

#[link(name = "ntdll")]
extern "system" {
    fn NtQueryInformationProcess(
        process: HANDLE,
        class: u32,
        info: *mut core::ffi::c_void,
        len: u32,
        ret: *mut u32,
    ) -> i32;
}

/// `ProcessCommandLineInformation`.
const PROCESS_COMMAND_LINE_INFORMATION: u32 = 60;

/// Terminates every browser process whose command line references `dir`.
/// Best-effort: failures (e.g. another user's/elevated process) are ignored.
pub fn kill_browsers_for_profile(dir: &str) {
    let needle = dir.to_lowercase();
    if needle.is_empty() {
        return;
    }
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return;
        };
        if snapshot == INVALID_HANDLE_VALUE {
            return;
        }
        let mut entry = PROCESSENTRY32W::default();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let name = wide_field(&entry.szExeFile).to_lowercase();
                if is_browser_exe(&name) && entry.th32ProcessID != 0 {
                    if let Some(cmd) = command_line(entry.th32ProcessID) {
                        if cmd.to_lowercase().contains(&needle) {
                            terminate(entry.th32ProcessID);
                        }
                    }
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }
}

unsafe fn command_line(pid: u32) -> Option<String> {
    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
    let mut size: u32 = 0;
    // First probe to learn the required buffer size.
    let _ = NtQueryInformationProcess(
        handle,
        PROCESS_COMMAND_LINE_INFORMATION,
        std::ptr::null_mut(),
        0,
        &mut size,
    );
    if size == 0 {
        size = 8192;
    }
    let mut buf = vec![0u8; size as usize];
    let status = NtQueryInformationProcess(
        handle,
        PROCESS_COMMAND_LINE_INFORMATION,
        buf.as_mut_ptr() as *mut core::ffi::c_void,
        buf.len() as u32,
        &mut size,
    );
    let result = if status >= 0 {
        // UNICODE_STRING: u16 length (bytes), u16 max, pad, then buffer pointer
        // at offset 8 on 64-bit.
        let length = *(buf.as_ptr() as *const u16) as usize;
        let buffer = *(buf.as_ptr().add(8) as *const *const u16);
        if length > 0 && !buffer.is_null() && length <= size as usize {
            Some(String::from_utf16_lossy(std::slice::from_raw_parts(
                buffer,
                length / 2,
            )))
        } else {
            None
        }
    } else {
        None
    };
    let _ = CloseHandle(handle);
    result
}

unsafe fn terminate(pid: u32) {
    if let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, pid) {
        let _ = TerminateProcess(handle, 1);
        let _ = CloseHandle(handle);
    }
}

fn wide_field(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}
