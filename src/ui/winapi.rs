//! Small helpers over the `windows` crate for the tray icon and its
//! custom-drawn menu: GDI text/font, work-area clamping and icon loading.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, DrawTextW, GetMonitorInfoW, GetTextExtentPoint32W, MonitorFromPoint,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DRAW_TEXT_FORMAT, HFONT, HDC,
    MONITORINFO, MONITOR_DEFAULTTONEAREST, OUT_DEFAULT_PRECIS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{CreateIconFromResourceEx, HICON, IMAGE_FLAGS};

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

/// The window's DPI, floored at the standard 96.
pub unsafe fn dpi_for_window(hwnd: HWND) -> i32 {
    let d = unsafe { GetDpiForWindow(hwnd) };
    if d < 96 { 96 } else { d as i32 }
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

/// Builds an icon from raw `.ico` bytes, choosing the image closest to the
/// requested width. Returns a null handle on failure.
pub unsafe fn create_icon_from_ico(ico: &[u8], want: i32) -> HICON {
    if ico.len() < 6 || ico[0] != 0 || ico[1] != 0 || ico[2] != 1 || ico[3] != 0 {
        return HICON::default();
    }
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    let mut best: Option<(usize, usize)> = None;
    let mut best_score = i32::MAX;
    for i in 0..count {
        let base = 6 + i * 16;
        if base + 16 > ico.len() {
            break;
        }
        let mut w = ico[base] as i32;
        if w == 0 {
            w = 256;
        }
        let len = u32::from_le_bytes([ico[base + 8], ico[base + 9], ico[base + 10], ico[base + 11]])
            as usize;
        let off = u32::from_le_bytes([
            ico[base + 12],
            ico[base + 13],
            ico[base + 14],
            ico[base + 15],
        ]) as usize;
        if len == 0 || off + len > ico.len() {
            continue;
        }
        let score = (w - want).abs();
        if score < best_score {
            best_score = score;
            best = Some((off, len));
        }
    }
    let Some((off, len)) = best else {
        return HICON::default();
    };
    unsafe {
        CreateIconFromResourceEx(&ico[off..off + len], true, 0x0003_0000, 0, 0, IMAGE_FLAGS(0))
            .unwrap_or_default()
    }
}
