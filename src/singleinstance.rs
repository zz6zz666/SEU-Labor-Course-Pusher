//! Ensures only one resident process runs, and lets a second launch ask the
//! running one to show its wizard.

use std::ffi::c_void;

const MUTEX_NAME: &str = r"Local\SEULaborPusher";
const EVENT_NAME: &str = r"Local\SEULaborPusher_ShowWizard";
const ERROR_ALREADY_EXISTS: u32 = 183;
const EVENT_MODIFY_STATE: u32 = 0x0002;
const INFINITE: u32 = 0xFFFF_FFFF;

#[link(name = "kernel32")]
extern "system" {
    fn CreateMutexW(sa: *mut c_void, initial_owner: i32, name: *const u16) -> isize;
    fn CreateEventW(
        sa: *mut c_void,
        manual_reset: i32,
        initial_state: i32,
        name: *const u16,
    ) -> isize;
    fn OpenEventW(access: u32, inherit: i32, name: *const u16) -> isize;
    fn SetEvent(handle: isize) -> i32;
    fn WaitForSingleObject(handle: isize, ms: u32) -> u32;
    fn CloseHandle(handle: isize) -> i32;
    fn GetLastError() -> u32;
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub struct Instance {
    mutex: isize,
    event: isize,
}

unsafe impl Send for Instance {}

/// Takes the single-instance lock. `already = true` means another instance owns
/// it (and the caller should signal and exit).
pub fn acquire() -> (Option<Instance>, bool) {
    let name = to_wide(MUTEX_NAME);
    let mutex = unsafe { CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr()) };
    if mutex == 0 {
        return (None, false);
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe { CloseHandle(mutex) };
        return (None, true);
    }

    let ev_name = to_wide(EVENT_NAME);
    let event = unsafe { CreateEventW(std::ptr::null_mut(), 0, 0, ev_name.as_ptr()) };
    (Some(Instance { mutex, event }), false)
}

/// Wakes the running instance's wizard request.
pub fn signal_existing() -> bool {
    let name = to_wide(EVENT_NAME);
    let event = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
    if event == 0 {
        return false;
    }
    let ok = unsafe { SetEvent(event) } != 0;
    unsafe { CloseHandle(event) };
    ok
}

impl Instance {
    /// Blocks until another process signals (auto-reset event).
    pub fn wait(&self) {
        unsafe {
            WaitForSingleObject(self.event, INFINITE);
        }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            if self.event != 0 {
                CloseHandle(self.event);
            }
            if self.mutex != 0 {
                CloseHandle(self.mutex);
            }
        }
    }
}
