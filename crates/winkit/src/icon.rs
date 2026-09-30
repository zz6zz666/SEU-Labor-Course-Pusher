//! `HICON` construction from raw `.ico` bytes, for the tray and for window
//! title-bar icons.

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, SendMessageW, HICON, ICON_BIG, ICON_SMALL, IMAGE_FLAGS, WM_SETICON,
};

/// Builds an icon from raw `.ico` bytes, choosing the image closest to the
/// requested width. Returns a null handle on failure.
pub fn from_ico(ico: &[u8], want: i32) -> HICON {
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

/// Sets both the small and large window icons from raw `.ico` bytes.
pub fn set_window_icon(hwnd: HWND, ico: &[u8]) {
    let icon = from_ico(ico, 32);
    if icon.0.is_null() {
        return;
    }
    unsafe {
        let _ = SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_BIG as usize)),
            Some(LPARAM(icon.0 as isize)),
        );
        let _ = SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_SMALL as usize)),
            Some(LPARAM(icon.0 as isize)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal ICO directory header with `count` entries.
    fn header(count: u16) -> Vec<u8> {
        let mut v = vec![0u8, 0, 1, 0];
        v.extend_from_slice(&count.to_le_bytes());
        v
    }

    /// width, height, colors, reserved, planes, bpp, length, offset.
    fn entry(width: u8, length: u32, offset: u32) -> Vec<u8> {
        let mut v = vec![width, width, 0, 0, 1, 0, 32, 0];
        v.extend_from_slice(&length.to_le_bytes());
        v.extend_from_slice(&offset.to_le_bytes());
        v
    }

    #[test]
    fn rejects_input_that_is_not_an_ico() {
        assert!(from_ico(&[], 16).0.is_null());
        assert!(from_ico(&[0, 0, 1], 16).0.is_null());
        assert!(from_ico(b"not an icon at all!!", 16).0.is_null());
    }

    #[test]
    fn rejects_bad_or_out_of_range_entries() {
        // Zero entries.
        assert!(from_ico(&header(0), 16).0.is_null());
        // One entry whose image extends past the end of the buffer.
        let mut v = header(1);
        v.extend_from_slice(&entry(16, 100, 50));
        assert!(from_ico(&v, 16).0.is_null());
    }
}
