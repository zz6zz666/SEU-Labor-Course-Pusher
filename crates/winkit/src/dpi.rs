//! The primary display scale factor (1.0 == 96 DPI).

use windows::Win32::UI::HiDpi::GetDpiForSystem;

pub fn dpi_scale() -> f64 {
    let d = unsafe { GetDpiForSystem() };
    if d >= 96 {
        d as f64 / 96.0
    } else {
        1.0
    }
}
