//! Minimal raw Win32 FFI used by the tray icon and its custom-drawn menu.
//! Declared by hand (rather than via a bindings crate) so the layouts mirror
//! the original implementation exactly.

#![allow(non_snake_case, non_camel_case_types)]

use std::ffi::c_void;

pub type HWND = isize;
pub type HINSTANCE = isize;
pub type HICON = isize;
pub type HCURSOR = isize;
pub type HBRUSH = isize;
pub type HDC = isize;
pub type HBITMAP = isize;
pub type HGDIOBJ = isize;
pub type HMENU = isize;
pub type LRESULT = isize;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type ATOM = u16;
pub type WndProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

pub const WS_POPUP: u32 = 0x8000_0000;
pub const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
pub const WS_EX_TOPMOST: u32 = 0x0000_0008;
pub const WS_EX_LAYERED: u32 = 0x0008_0000;
pub const CS_DROPSHADOW: u32 = 0x0002_0000;

pub const SW_HIDE: i32 = 0;
pub const SW_SHOWNOACTIVATE: i32 = 4;

pub const SWP_NOZORDER: u32 = 0x0004;
pub const SWP_NOACTIVATE: u32 = 0x0010;

pub const WM_DESTROY: u32 = 0x0002;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_PAINT: u32 = 0x000F;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const WM_ACTIVATE: u32 = 0x0006;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_RBUTTONUP: u32 = 0x0205;
pub const WM_MOUSELEAVE: u32 = 0x02A3;
pub const WM_SETCURSOR: u32 = 0x0020;
pub const WM_CONTEXTMENU: u32 = 0x007B;
pub const WM_APP: u32 = 0x8000;
pub const WA_INACTIVE: u16 = 0;

pub const HTCLIENT: u32 = 1;
pub const IDC_ARROW: usize = 32512;

pub const SM_CXSMICON: i32 = 49;
pub const SM_CYSMICON: i32 = 50;

pub const IMAGE_ICON: u32 = 1;

pub const NIM_ADD: u32 = 0;
pub const NIM_MODIFY: u32 = 1;
pub const NIM_DELETE: u32 = 2;

pub const NIF_MESSAGE: u32 = 0x0000_0001;
pub const NIF_ICON: u32 = 0x0000_0002;
pub const NIF_TIP: u32 = 0x0000_0004;

pub const TRANSPARENT: i32 = 1;
pub const DT_LEFT: u32 = 0x0000;
pub const DT_CENTER: u32 = 0x0001;
pub const DT_VCENTER: u32 = 0x0004;
pub const DT_SINGLELINE: u32 = 0x0020;
pub const DT_NOPREFIX: u32 = 0x0800;
pub const SRCCOPY: u32 = 0x00CC_0020;
pub const PS_SOLID: i32 = 0;
pub const FW_NORMAL: i32 = 400;
pub const DEFAULT_CHARSET: u32 = 1;
pub const CLEARTYPE_QUALITY: u32 = 5;

pub const TME_LEAVE: u32 = 0x0000_0002;

pub const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
pub const DWMWCP_ROUNDSMALL: i32 = 3;
pub const DWMWA_BORDER_COLOR: u32 = 34;

pub const MONITOR_DEFAULTTONEAREST: u32 = 2;

pub const LWA_ALPHA: u32 = 0x0000_0002;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SIZE {
    pub cx: i32,
    pub cy: i32,
}

#[repr(C)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: u32,
    pub wparam: WPARAM,
    pub lparam: LPARAM,
    pub time: u32,
    pub pt: POINT,
    pub lprivate: u32,
}

#[repr(C)]
pub struct WNDCLASSEXW {
    pub cb_size: u32,
    pub style: u32,
    pub lpfn_wnd_proc: Option<WndProc>,
    pub cb_cls_extra: i32,
    pub cb_wnd_extra: i32,
    pub h_instance: HINSTANCE,
    pub h_icon: HICON,
    pub h_cursor: HCURSOR,
    pub hbr_background: HBRUSH,
    pub lpsz_menu_name: *const u16,
    pub lpsz_class_name: *const u16,
    pub h_icon_sm: HICON,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct GUID {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

#[repr(C)]
pub struct NOTIFYICONDATAW {
    pub cb_size: u32,
    pub hwnd: HWND,
    pub uid: u32,
    pub uflags: u32,
    pub ucallback_message: u32,
    pub hicon: HICON,
    pub sz_tip: [u16; 128],
    pub dw_state: u32,
    pub dw_state_mask: u32,
    pub sz_info: [u16; 256],
    pub uversion: u32,
    pub sz_info_title: [u16; 64],
    pub dw_info_flags: u32,
    pub guid_item: GUID,
    pub h_balloon_icon: HICON,
}

#[repr(C)]
pub struct MONITORINFO {
    pub cb_size: u32,
    pub rc_monitor: RECT,
    pub rc_work: RECT,
    pub dw_flags: u32,
}

#[repr(C)]
pub struct TRACKMOUSEEVENT {
    pub cb_size: u32,
    pub dw_flags: u32,
    pub hwnd_track: HWND,
    pub dw_hover_time: u32,
}

#[repr(C)]
pub struct PAINTSTRUCT {
    pub hdc: HDC,
    pub f_erase: i32,
    pub rc_paint: RECT,
    pub f_restore: i32,
    pub f_inc_update: i32,
    pub rgb_reserved: [u8; 32],
}

pub fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub unsafe fn get_module_handle() -> HINSTANCE {
    GetModuleHandleW(std::ptr::null())
}

pub unsafe fn copy_utf16(dst: &mut [u16], s: &str) {
    let u: Vec<u16> = s.encode_utf16().collect();
    let n = u.len().min(dst.len().saturating_sub(1));
    dst[..n].copy_from_slice(&u[..n]);
    if n < dst.len() {
        dst[n] = 0;
    }
}

#[link(name = "user32")]
extern "system" {
    pub fn RegisterClassExW(lpwcx: *const WNDCLASSEXW) -> ATOM;
    pub fn CreateWindowExW(
        ex_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: HWND,
        menu: HMENU,
        instance: HINSTANCE,
        param: *mut c_void,
    ) -> HWND;
    pub fn DefWindowProcW(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
    pub fn GetMessageW(msg: *mut MSG, hwnd: HWND, min: u32, max: u32) -> i32;
    pub fn TranslateMessage(msg: *const MSG) -> i32;
    pub fn DispatchMessageW(msg: *const MSG) -> LRESULT;
    pub fn PostQuitMessage(code: i32);
    pub fn ShowWindow(hwnd: HWND, cmd: i32) -> i32;
    pub fn SetForegroundWindow(hwnd: HWND) -> i32;
    pub fn GetCursorPos(pt: *mut POINT) -> i32;
    pub fn GetClientRect(hwnd: HWND, r: *mut RECT) -> i32;
    pub fn InvalidateRect(hwnd: HWND, r: *const RECT, erase: i32) -> i32;
    pub fn BeginPaint(hwnd: HWND, ps: *mut PAINTSTRUCT) -> HDC;
    pub fn EndPaint(hwnd: HWND, ps: *const PAINTSTRUCT) -> i32;
    pub fn SetWindowPos(
        hwnd: HWND,
        after: HWND,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        flags: u32,
    ) -> i32;
    pub fn DestroyWindow(hwnd: HWND) -> i32;
    pub fn LoadImageW(
        instance: HINSTANCE,
        name: *const u16,
        ty: u32,
        cx: i32,
        cy: i32,
        fu: u32,
    ) -> isize;
    pub fn CreateIconFromResourceEx(
        bits: *const u8,
        size: u32,
        is_icon: i32,
        version: u32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> HICON;
    pub fn GetSystemMetrics(index: i32) -> i32;
    pub fn LoadCursorW(instance: HINSTANCE, name: *const u16) -> HCURSOR;
    pub fn SetCursor(cur: HCURSOR) -> HCURSOR;
    pub fn GetDC(hwnd: HWND) -> HDC;
    pub fn ReleaseDC(hwnd: HWND, dc: HDC) -> i32;
    pub fn TrackMouseEvent(tme: *mut TRACKMOUSEEVENT) -> i32;
    pub fn GetDpiForWindow(hwnd: HWND) -> u32;
    pub fn MonitorFromPoint(pt: POINT, flags: u32) -> isize;
    pub fn GetMonitorInfoW(monitor: isize, info: *mut MONITORINFO) -> i32;
    pub fn RegisterWindowMessageW(name: *const u16) -> u32;
    pub fn SetLayeredWindowAttributes(hwnd: HWND, key: u32, alpha: u8, flags: u32) -> i32;
    pub fn FillRect(dc: HDC, r: *const RECT, brush: HBRUSH) -> i32;
    pub fn DrawTextW(dc: HDC, text: *const u16, count: i32, r: *mut RECT, fmt: u32) -> i32;
    pub fn SendMessageW(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
}

#[link(name = "shell32")]
extern "system" {
    pub fn Shell_NotifyIconW(msg: u32, data: *const NOTIFYICONDATAW) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    pub fn GetModuleHandleW(name: *const u16) -> HINSTANCE;
    pub fn GetTickCount() -> u32;
}

#[link(name = "dwmapi")]
extern "system" {
    pub fn DwmSetWindowAttribute(
        hwnd: HWND,
        attribute: u32,
        value: *const c_void,
        size: u32,
    ) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    pub fn CreateCompatibleDC(dc: HDC) -> HDC;
    pub fn CreateCompatibleBitmap(dc: HDC, w: i32, h: i32) -> HBITMAP;
    pub fn SelectObject(dc: HDC, obj: HGDIOBJ) -> HGDIOBJ;
    pub fn DeleteObject(obj: HGDIOBJ) -> i32;
    pub fn DeleteDC(dc: HDC) -> i32;
    pub fn CreateSolidBrush(color: u32) -> HBRUSH;
    pub fn CreatePen(style: i32, width: i32, color: u32) -> HGDIOBJ;
    pub fn RoundRect(
        dc: HDC,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        w: i32,
        h: i32,
    ) -> i32;
    pub fn BitBlt(
        dst: HDC,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        src: HDC,
        sx: i32,
        sy: i32,
        rop: u32,
    ) -> i32;
    pub fn SetBkMode(dc: HDC, mode: i32) -> i32;
    pub fn SetTextColor(dc: HDC, color: u32) -> u32;
    pub fn CreateFontW(
        height: i32,
        width: i32,
        escapement: i32,
        orientation: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strike_out: u32,
        charset: u32,
        out_precision: u32,
        clip_precision: u32,
        quality: u32,
        pitch_and_family: u32,
        face: *const u16,
    ) -> HGDIOBJ;
    pub fn GetTextExtentPoint32W(dc: HDC, s: *const u16, len: i32, size: *mut SIZE) -> i32;
}

pub unsafe fn create_font(face: &str, height: i32, weight: i32) -> HGDIOBJ {
    let face = utf16(face);
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
        0,
        0,
        CLEARTYPE_QUALITY,
        0,
        face.as_ptr(),
    )
}

pub unsafe fn draw_text(dc: HDC, s: &str, r: &mut RECT, format: u32) {
    let u = utf16(s);
    DrawTextW(dc, u.as_ptr(), -1, r, format);
}

pub unsafe fn text_width(dc: HDC, s: &str) -> i32 {
    let u = utf16(s);
    let mut sz = SIZE::default();
    GetTextExtentPoint32W(dc, u.as_ptr(), (u.len() - 1) as i32, &mut sz);
    sz.cx
}

pub unsafe fn set_rounded_corners(hwnd: HWND) {
    let v: i32 = DWMWCP_ROUNDSMALL;
    DwmSetWindowAttribute(
        hwnd,
        DWMWA_WINDOW_CORNER_PREFERENCE,
        &v as *const i32 as *const c_void,
        std::mem::size_of::<i32>() as u32,
    );
}

pub unsafe fn set_border_color(hwnd: HWND, color: u32) {
    DwmSetWindowAttribute(
        hwnd,
        DWMWA_BORDER_COLOR,
        &color as *const u32 as *const c_void,
        std::mem::size_of::<u32>() as u32,
    );
}

pub unsafe fn set_layered_alpha(hwnd: HWND, alpha: u8) {
    SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA);
}

pub unsafe fn dpi_for_window(hwnd: HWND) -> i32 {
    let d = GetDpiForWindow(hwnd);
    if d < 96 {
        96
    } else {
        d as i32
    }
}

pub unsafe fn clamp_to_work_area(x: i32, y: i32, w: i32, h: i32, pt: POINT) -> (i32, i32) {
    let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
    if mon == 0 {
        return (x, y);
    }
    let mut mi: MONITORINFO = std::mem::zeroed();
    mi.cb_size = std::mem::size_of::<MONITORINFO>() as u32;
    if GetMonitorInfoW(mon, &mut mi) == 0 {
        return (x, y);
    }
    let wa = mi.rc_work;
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

/// Builds an HICON from raw .ico bytes, choosing the image closest to the
/// requested size. Returns 0 on failure.
pub unsafe fn create_icon_from_ico(ico: &[u8], want: i32) -> HICON {
    if ico.len() < 6 || ico[0] != 0 || ico[1] != 0 || ico[2] != 1 || ico[3] != 0 {
        return 0;
    }
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    let mut best_off = 0usize;
    let mut best_len = 0usize;
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
        let off = u32::from_le_bytes([ico[base + 12], ico[base + 13], ico[base + 14], ico[base + 15]])
            as usize;
        let len = u32::from_le_bytes([ico[base + 8], ico[base + 9], ico[base + 10], ico[base + 11]])
            as usize;
        if len == 0 || off + len > ico.len() {
            continue;
        }
        let score = (w - want).abs();
        if score < best_score {
            best_score = score;
            best_off = off;
            best_len = len;
        }
    }
    if best_len == 0 {
        return 0;
    }
    CreateIconFromResourceEx(
        ico[best_off..].as_ptr(),
        best_len as u32,
        1,
        0x0003_0000,
        0,
        0,
        0,
    )
}
