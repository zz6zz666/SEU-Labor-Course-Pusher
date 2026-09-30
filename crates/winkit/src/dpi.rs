//! Process DPI awareness and the primary display scale factor (1.0 == 96 DPI).

use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwareness, SetProcessDpiAwarenessContext,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, PROCESS_DPI_AWARENESS,
};
use windows::Win32::UI::WindowsAndMessaging::SetProcessDPIAware;

/// Opts the process into Per-Monitor V2 DPI awareness so windows render crisply
/// on scaled displays instead of being bitmap-stretched.
///
/// This is a process-global, once-only setting: if awareness was already set
/// (by an earlier call, the host app, or a manifest) these calls are no-ops and
/// the existing setting is left untouched. GUI crates call it when they create
/// their first window; a host may also call it explicitly at startup.
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

/// The primary display scale factor (1.0 == 96 DPI). Meaningful only once the
/// process is DPI aware (see [`enable_per_monitor_dpi`]).
pub fn dpi_scale() -> f64 {
    let d = unsafe { windows::Win32::UI::HiDpi::GetDpiForSystem() };
    if d >= 96 {
        d as f64 / 96.0
    } else {
        1.0
    }
}
