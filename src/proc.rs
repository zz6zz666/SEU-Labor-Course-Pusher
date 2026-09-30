//! A minimal process handle used for the short-lived wizard window process: we
//! need to terminate it (the page's close button) and to detect its exit (the
//! user closed the window) without holding a blocking `std::process::Child`.

use std::path::Path;
use std::sync::Arc;

const SYNCHRONIZE: u32 = 0x0010_0000;
const PROCESS_TERMINATE: u32 = 0x0001;
const INFINITE: u32 = 0xFFFF_FFFF;

#[link(name = "kernel32")]
extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
    fn TerminateProcess(handle: isize, code: u32) -> i32;
    fn WaitForSingleObject(handle: isize, ms: u32) -> u32;
    fn CloseHandle(handle: isize) -> i32;
}

struct Inner {
    pid: u32,
    handle: isize,
}

unsafe impl Send for Inner {}
unsafe impl Sync for Inner {}

impl Drop for Inner {
    fn drop(&mut self) {
        if self.handle != 0 {
            unsafe {
                CloseHandle(self.handle);
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
        if self.inner.handle != 0 {
            unsafe {
                TerminateProcess(self.inner.handle, 1);
            }
        }
    }

    pub fn wait(&self) {
        if self.inner.handle != 0 {
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
    let handle = unsafe { OpenProcess(SYNCHRONIZE | PROCESS_TERMINATE, 0, pid) };
    Ok(ChildProc {
        inner: Arc::new(Inner { pid, handle }),
    })
}
