//! A minimal process handle used for the short-lived wizard window process: we
//! need to terminate it (the page's close button) and to detect its exit (the
//! user closed the window) without holding a blocking `std::process::Child`.

use std::path::Path;
use std::sync::Arc;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::SYNCHRONIZE;
use windows::Win32::System::Threading::{
    OpenProcess, TerminateProcess, WaitForSingleObject, INFINITE, PROCESS_ACCESS_RIGHTS,
    PROCESS_TERMINATE,
};

struct Inner {
    pid: u32,
    handle: HANDLE,
}

unsafe impl Send for Inner {}
unsafe impl Sync for Inner {}

impl Drop for Inner {
    fn drop(&mut self) {
        if !self.handle.0.is_null() {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

#[derive(Clone)]
pub struct ChildProc {
    inner: Arc<Inner>,
}

impl ChildProc {
    pub fn pid(&self) -> u32 {
        self.inner.pid
    }

    pub fn kill(&self) {
        if !self.inner.handle.0.is_null() {
            unsafe {
                let _ = TerminateProcess(self.inner.handle, 1);
            }
        }
    }

    pub fn wait(&self) {
        if !self.inner.handle.0.is_null() {
            unsafe {
                WaitForSingleObject(self.inner.handle, INFINITE);
            }
        }
    }
}

pub fn spawn(exe: &Path, args: &[String]) -> std::io::Result<ChildProc> {
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let child = cmd.spawn()?;
    let pid = child.id();
    drop(child);
    // `OpenProcess` wants PROCESS_ACCESS_RIGHTS; SYNCHRONIZE is a shared
    // standard right, so merge the two raw bit masks.
    let access = PROCESS_ACCESS_RIGHTS(SYNCHRONIZE.0 | PROCESS_TERMINATE.0);
    let handle = unsafe { OpenProcess(access, false, pid) }.unwrap_or_default();
    Ok(ChildProc {
        inner: Arc::new(Inner { pid, handle }),
    })
}
