//! Small helpers over the `windows` crate for the tray icon and its
//! custom-drawn menu: GDI text/font, work-area clamping and DPI.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, POINT, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, DrawTextW, GetMonitorInfoW, GetTextExtentPoint32W, MonitorFromPoint,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DRAW_TEXT_FORMAT, HFONT, HDC,
    MONITORINFO, MONITOR_DEFAULTTONEAREST, OUT_DEFAULT_PRECIS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

/// UTF-16 with a trailing NUL, for `PCWSTR` arguments.
pub fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// UTF-16 without a NUL, for the length-delimited GDI text APIs.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Copies `s` into a fixed-size NUL-terminated buffer, truncating to fit.
pub fn copy_into(dst: &mut [u16], s: &str) {
    let u: Vec<u16> = s.encode_utf16().collect();
    let n = u.len().min(dst.len().saturating_sub(1));
    dst[..n].copy_from_slice(&u[..n]);
    if n < dst.len() {
        dst[n] = 0;
    }
}

/// The module handle of the running executable.
pub fn module_handle() -> HINSTANCE {
    unsafe { GetModuleHandleW(None) }
        .map(|m| HINSTANCE(m.0))
        .unwrap_or_default()
}

/// Creates a font with the given family, height (negative = character height)
/// and weight, using ClearType rendering.
pub unsafe fn create_font(face: &str, height: i32, weight: i32) -> HFONT {
    let face = utf16(face);
    unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            PCWSTR(face.as_ptr()),
        )
    }
}

/// Draws a single line of text within `r` using the given format flags.
pub unsafe fn draw_text(hdc: HDC, s: &str, r: &mut RECT, format: DRAW_TEXT_FORMAT) {
    let mut u = wide(s);
    unsafe {
        DrawTextW(hdc, &mut u, r, format);
    }
}

/// The width in pixels of `s` in the DC's currently selected font.
pub unsafe fn text_width(hdc: HDC, s: &str) -> i32 {
    let u = wide(s);
    let mut sz = SIZE::default();
    unsafe {
        let _ = GetTextExtentPoint32W(hdc, &u, &mut sz);
    }
    sz.cx
}

/// The effective DPI of the monitor nearest `pt`, floored at the standard 96.
///
/// The menu is sized and drawn *before* it is moved under the cursor, so it
/// must take its DPI from the target monitor rather than from the window's
/// current (stale) position — otherwise a mixed-DPI multi-monitor setup renders
/// the popup at the wrong scale.
pub unsafe fn dpi_for_point(pt: POINT) -> i32 {
    let mon = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
    if mon.0.is_null() {
        return 96;
    }
    let mut x: u32 = 0;
    let mut y: u32 = 0;
    match unsafe { GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut x, &mut y) } {
        Ok(()) if x >= 96 => x as i32,
        _ => 96,
    }
}

/// Nudges a popup of size `w`x`h` at `(x, y)` so it stays inside the work area
/// of the monitor nearest `pt`.
pub unsafe fn clamp_to_work_area(x: i32, y: i32, w: i32, h: i32, pt: POINT) -> (i32, i32) {
    let mon = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
    if mon.0.is_null() {
        return (x, y);
    }
    let mut mi = MONITORINFO::default();
    mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if !unsafe { GetMonitorInfoW(mon, &mut mi) }.as_bool() {
        return (x, y);
    }
    let wa = mi.rcWork;
    let mut x = x;
    let mut y = y;
    if x + w > wa.right {
        x = wa.right - w;
    }
    if y + h > wa.bottom {
        y = wa.bottom - h;
    }
    if x < wa.left {
        x = wa.left;
    }
    if y < wa.top {
        y = wa.top;
    }
    (x, y)
}
