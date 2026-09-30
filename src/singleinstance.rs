//! Ensures only one resident process runs, and lets a second launch ask the
//! running one to show its wizard.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE,
    INFINITE,
};

use crate::osutil::wide;

const MUTEX_NAME: &str = r"Local\SEULaborPusher";
const EVENT_NAME: &str = r"Local\SEULaborPusher_ShowWizard";

pub struct Instance {
    mutex: HANDLE,
    event: HANDLE,
}

unsafe impl Send for Instance {}

/// Takes the single-instance lock. `already = true` means another instance owns
/// it (and the caller should signal and exit).
pub fn acquire() -> (Option<Instance>, bool) {
    let name = wide(MUTEX_NAME);
    let Ok(mutex) = (unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }) else {
        return (None, false);
    };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            let _ = CloseHandle(mutex);
        }
        return (None, true);
    }

    let ev_name = wide(EVENT_NAME);
    let event =
        unsafe { CreateEventW(None, false, false, PCWSTR(ev_name.as_ptr())) }.unwrap_or_default();
    (Some(Instance { mutex, event }), false)
}

/// Wakes the running instance's wizard request.
pub fn signal_existing() -> bool {
    let name = wide(EVENT_NAME);
    let Ok(event) = (unsafe { OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(name.as_ptr())) })
    else {
        return false;
    };
    let ok = unsafe { SetEvent(event) }.is_ok();
    unsafe {
        let _ = CloseHandle(event);
    }
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
            if !self.event.0.is_null() {
                let _ = CloseHandle(self.event);
            }
            if !self.mutex.0.is_null() {
                let _ = CloseHandle(self.mutex);
            }
        }
    }
}
