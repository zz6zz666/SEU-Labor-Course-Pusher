//! The custom-drawn Fluent-style popup menu. A manual Win32 replica: a layered
//! popup window with rounded corners, a subtle border, hover highlight and a
//! leading icon/check gutter.
//!
//! The rows are rebuilt from the host's [`crate::TrayConfig::items`] on every
//! show, so labels and check marks always reflect current state.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUNDSMALL,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreatePen, CreateSolidBrush,
    DeleteDC, DeleteObject, EndPaint, FillRect, GetDC, HGDIOBJ, HBITMAP, InvalidateRect, ReleaseDC,
    RoundRect, SelectObject, SetBkMode, SetTextColor, DT_CENTER, DT_LEFT, DT_NOPREFIX,
    DT_SINGLELINE, DT_VCENTER, FW_NORMAL, HDC, PAINTSTRUCT, PS_SOLID, SRCCOPY, TRANSPARENT,
};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, GetCursorPos, GetWindowLongPtrW, LoadCursorW,
    RegisterClassExW, SetCursor, SetForegroundWindow, SetLayeredWindowAttributes, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, CS_DROPSHADOW, GWLP_USERDATA, HCURSOR, HTCLIENT, IDC_ARROW, LWA_ALPHA,
    SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER, WA_INACTIVE, WM_ACTIVATE, WM_CLOSE,
    WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_SETCURSOR, WNDCLASSEXW,
    WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use super::winapi::{
    clamp_to_work_area, create_font, dpi_for_point, draw_text, module_handle, text_width, utf16,
};
use crate::tray::{ItemKind, MenuItem, TrayInner};

fn null_gdiobj() -> HGDIOBJ {
    HGDIOBJ(std::ptr::null_mut())
}

pub(crate) struct Menu {
    tray: *mut TrayInner,
    hwnd: HWND,
    entries: Vec<MenuItem>,
    hover: i32,
    dpi: i32,
    scale: f64,
    text_font: HGDIOBJ,
    info_font: HGDIOBJ,
    icon_font: HGDIOBJ,
    pad_x: i32,
    pad_y: i32,
    item_h: i32,
    sep_h: i32,
    icon_col: i32,
    radius: i32,
    shown_at: u32,
    /// Cached back-buffer, resized only when the popup's size changes, so hover
    /// repaints do not allocate GDI objects every frame.
    mem_dc: HDC,
    mem_bmp: HBITMAP,
    mem_w: i32,
    mem_h: i32,
}

impl Menu {
    pub fn new(tray: *mut TrayInner) -> Option<Box<Menu>> {
        let hinst = module_handle();
        let cls = utf16("traykit_menu");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DROPSHADOW,
            hInstance: hinst,
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() },
            lpfnWndProc: Some(menu_wnd_proc),
            lpszClassName: PCWSTR(cls.as_ptr()),
            ..Default::default()
        };
        unsafe { RegisterClassExW(&wc) };
        let name = utf16("menu");
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_LAYERED,
                PCWSTR(cls.as_ptr()),
                PCWSTR(name.as_ptr()),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(hinst),
                None,
            )
        }
        .ok()?;
        let m = Box::new(Menu {
            tray,
            hwnd,
            entries: Vec::new(),
            hover: -1,
            dpi: 0,
            scale: 1.0,
            text_font: null_gdiobj(),
            info_font: null_gdiobj(),
            icon_font: null_gdiobj(),
            pad_x: 0,
            pad_y: 0,
            item_h: 0,
            sep_h: 0,
            icon_col: 0,
            radius: 0,
            shown_at: 0,
            mem_dc: HDC::default(),
            mem_bmp: HBITMAP::default(),
            mem_w: 0,
            mem_h: 0,
        });
        unsafe {
            set_rounded_corners(hwnd);
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 240, LWA_ALPHA);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, &*m as *const Menu as isize);
        }
        Some(m)
    }

    fn build_entries(&mut self) {
        self.entries = unsafe { ((*self.tray).items)() };
    }

    fn ensure_dpi(&mut self, dpi: i32) {
        if dpi == self.dpi && !self.text_font.0.is_null() {
            return;
        }
        self.dpi = dpi;
        self.scale = dpi as f64 / 96.0;
        for font in [self.text_font, self.info_font, self.icon_font] {
            if !font.0.is_null() {
                let _ = unsafe { DeleteObject(font) };
            }
        }
        self.text_font = font("Segoe UI Variable Text", (12.0 * self.scale).round() as i32);
        self.info_font = font("Segoe UI Variable Text", (11.0 * self.scale).round() as i32);
        self.icon_font = font("Segoe MDL2 Assets", (14.0 * self.scale).round() as i32);
        self.pad_x = (14.0 * self.scale).round() as i32;
        self.pad_y = (6.0 * self.scale).round() as i32;
        self.item_h = (29.0 * self.scale).round() as i32;
        self.sep_h = (10.0 * self.scale).round() as i32;
        self.icon_col = (31.0 * self.scale).round() as i32;
        self.radius = (6.0 * self.scale).round() as i32;
    }

    fn measure(&mut self) -> (i32, i32) {
        let dc = unsafe { GetDC(Some(self.hwnd)) };
        let mut max_w = 0;
        for e in &self.entries {
            if e.kind.is_separator() {
                continue;
            }
            let font = if e.kind.is_info() {
                self.info_font
            } else {
                self.text_font
            };
            unsafe { SelectObject(dc, font) };
            let w = unsafe { text_width(dc, &e.label) };
            if w > max_w {
                max_w = w;
            }
        }
        unsafe { ReleaseDC(Some(self.hwnd), dc) };

        let mut width = self.pad_x * 2 + self.icon_col + max_w + (14.0 * self.scale).round() as i32;
        let min = (210.0 * self.scale).round() as i32;
        let max = (560.0 * self.scale).round() as i32;
        if width < min {
            width = min;
        }
        if width > max {
            width = max;
        }

        let mut height = self.pad_y * 2;
        for e in &self.entries {
            height += if e.kind.is_separator() {
                self.sep_h
            } else {
                self.item_h
            };
        }
        (width, height)
    }

    pub fn show(&mut self) {
        // Size against the monitor under the cursor rather than the window's
        // stale (pre-move) position, so a mixed-DPI setup still scales right.
        let mut pt = POINT::default();
        unsafe { let _ = GetCursorPos(&mut pt); };
        let dpi = unsafe { dpi_for_point(pt) };
        self.ensure_dpi(dpi);

        self.build_entries();
        let (w, h) = self.measure();

        let x = pt.x - w + (8.0 * self.scale).round() as i32;
        let y = pt.y - h - (8.0 * self.scale).round() as i32;
        let (x, y) = unsafe { clamp_to_work_area(x, y, w, h, pt) };

        unsafe {
            let _ = SetWindowPos(self.hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
            set_rounded_corners(self.hwnd);
            set_border_color(self.hwnd, 0x00E4E4E4);
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            let _ = SetForegroundWindow(self.hwnd);
            let _ = InvalidateRect(Some(self.hwnd), None, true);
        }
        self.hover = -1;
        self.shown_at = unsafe { GetTickCount() };
    }

    pub fn hide(&mut self) {
        // Release the cached back-buffer while the popup is not visible; it is
        // rebuilt on the next show, so the process footprint does not stay high
        // after the menu has been used.
        if !self.mem_bmp.0.is_null() {
            let _ = unsafe { DeleteObject(HGDIOBJ(self.mem_bmp.0)) };
            self.mem_bmp = HBITMAP::default();
            self.mem_w = 0;
            self.mem_h = 0;
        }
        if !self.mem_dc.0.is_null() {
            let _ = unsafe { DeleteDC(self.mem_dc) };
            self.mem_dc = HDC::default();
        }
        if !self.hwnd.0.is_null() {
            let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
        }
        unsafe { let _ = SetForegroundWindow((*self.tray).hwnd); };
    }

    fn activate(&mut self, i: i32) {
        if i < 0 || i as usize >= self.entries.len() {
            return;
        }
        let (id, toggle) = match &self.entries[i as usize].kind {
            ItemKind::Info | ItemKind::Separator => return,
            ItemKind::Toggle(_) => (self.entries[i as usize].id, true),
            ItemKind::Command => (self.entries[i as usize].id, false),
        };
        let cmd = unsafe { (*self.tray).on_command.clone() };
        // Toggles keep the menu open so the check updates in place and another
        // option can be flipped immediately; a click elsewhere dismisses it.
        if toggle {
            let raw = self.hwnd.0 as isize;
            std::thread::spawn(move || {
                cmd(id);
                unsafe { let _ = InvalidateRect(Some(HWND(raw as *mut _)), None, true); };
            });
            return;
        }
        self.hide();
        std::thread::spawn(move || cmd(id));
    }

    fn on_paint(&mut self) {
        let mut ps = PAINTSTRUCT::default();
        let hdc = unsafe { BeginPaint(self.hwnd, &mut ps) };
        if hdc.0.is_null() {
            return;
        }
        let mut cr = RECT::default();
        unsafe { let _ = GetClientRect(self.hwnd, &mut cr); };
        let w = cr.right - cr.left;
        let h = cr.bottom - cr.top;
        if w <= 0 || h <= 0 {
            unsafe { let _ = EndPaint(self.hwnd, &ps); };
            return;
        }
        // Reuse a cached back-buffer sized to the popup, so hover repaints do
        // not churn GDI objects on every mouse move.
        if self.mem_dc.0.is_null() {
            self.mem_dc = unsafe { CreateCompatibleDC(Some(hdc)) };
        }
        if self.mem_bmp.0.is_null() || self.mem_w != w || self.mem_h != h {
            if !self.mem_bmp.0.is_null() {
                let _ = unsafe { DeleteObject(HGDIOBJ(self.mem_bmp.0)) };
            }
            self.mem_bmp = unsafe { CreateCompatibleBitmap(hdc, w, h) };
            self.mem_w = w;
            self.mem_h = h;
        }
        let old = unsafe { SelectObject(self.mem_dc, HGDIOBJ(self.mem_bmp.0)) };
        self.paint_into(self.mem_dc, w, h);
        unsafe {
            let _ = BitBlt(hdc, 0, 0, w, h, Some(self.mem_dc), 0, 0, SRCCOPY);
            SelectObject(self.mem_dc, old);
            let _ = EndPaint(self.hwnd, &ps);
        }
    }

    fn paint_into(&self, hdc: HDC, w: i32, h: i32) {
        let full = RECT {
            left: 0,
            top: 0,
            right: w,
            bottom: h,
        };
        let bg = unsafe { CreateSolidBrush(COLORREF(0x00F9F9F9)) };
        unsafe {
            FillRect(hdc, &full, bg);
            let _ = DeleteObject(HGDIOBJ(bg.0));
            SetBkMode(hdc, TRANSPARENT);
        }

        let mut y = self.pad_y;
        for (i, e) in self.entries.iter().enumerate() {
            if e.kind.is_separator() {
                let inset = (10.0 * self.scale).round() as i32;
                let line = RECT {
                    left: self.pad_x + inset,
                    right: w - self.pad_x - inset,
                    top: y + self.sep_h / 2,
                    bottom: y + self.sep_h / 2 + 1,
                };
                let sep = unsafe { CreateSolidBrush(COLORREF(0x00E4E4E4)) };
                unsafe {
                    FillRect(hdc, &line, sep);
                    let _ = DeleteObject(HGDIOBJ(sep.0));
                }
                y += self.sep_h;
                continue;
            }

            let row = RECT {
                left: self.pad_x,
                top: y,
                right: w - self.pad_x,
                bottom: y + self.item_h,
            };

            if e.kind.is_info() {
                let mut label = RECT {
                    left: row.left + self.icon_col,
                    top: row.top,
                    right: row.right,
                    bottom: row.bottom,
                };
                unsafe {
                    SelectObject(hdc, self.info_font);
                    SetTextColor(hdc, COLORREF(0x00808080));
                    draw_text(
                        hdc,
                        &e.label,
                        &mut label,
                        DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                    );
                }
                y += self.item_h;
                continue;
            }

            if i as i32 == self.hover {
                let hb = unsafe { CreateSolidBrush(COLORREF(0x00EBEBEB)) };
                let pen = unsafe { CreatePen(PS_SOLID, 1, COLORREF(0x00EBEBEB)) };
                let old_pen = unsafe { SelectObject(hdc, HGDIOBJ(pen.0)) };
                let old_brush = unsafe { SelectObject(hdc, HGDIOBJ(hb.0)) };
                unsafe {
                    let _ = RoundRect(
                        hdc,
                        row.left,
                        row.top,
                        row.right,
                        row.bottom,
                        self.radius * 2,
                        self.radius * 2,
                    );
                    SelectObject(hdc, old_pen);
                    SelectObject(hdc, old_brush);
                    let _ = DeleteObject(HGDIOBJ(pen.0));
                    let _ = DeleteObject(HGDIOBJ(hb.0));
                }
            }

            // Leading gutter: a check for toggles, an icon otherwise.
            let mut gutter = RECT {
                left: row.left,
                top: row.top,
                right: row.left + self.icon_col,
                bottom: row.bottom,
            };
            match &e.kind {
                ItemKind::Toggle(checked) => {
                    if checked() {
                        unsafe {
                            SelectObject(hdc, self.icon_font);
                            SetTextColor(hdc, COLORREF(0x00EB6F2F));
                            let ch = char::from_u32(0xE73E).unwrap().to_string();
                            draw_text(
                                hdc,
                                &ch,
                                &mut gutter,
                                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                            );
                        }
                    }
                }
                ItemKind::Command if e.glyph != 0 => {
                    unsafe {
                        SelectObject(hdc, self.icon_font);
                        SetTextColor(hdc, COLORREF(0x00666666));
                        let ch = char::from_u32(e.glyph as u32).unwrap().to_string();
                        draw_text(
                            hdc,
                            &ch,
                            &mut gutter,
                            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                        );
                    }
                }
                _ => {}
            }

            let mut label = RECT {
                left: row.left + self.icon_col,
                top: row.top,
                right: row.right,
                bottom: row.bottom,
            };
            unsafe {
                SelectObject(hdc, self.text_font);
                SetTextColor(hdc, COLORREF(0x001A1A1A));
                draw_text(
                    hdc,
                    &e.label,
                    &mut label,
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );
            }

            y += self.item_h;
        }
    }

    fn hit_test(&self, x: i32, y: i32) -> i32 {
        if x < self.pad_x {
            return -1;
        }
        let mut cy = self.pad_y;
        for (i, e) in self.entries.iter().enumerate() {
            let hh = if e.kind.is_separator() {
                self.sep_h
            } else {
                self.item_h
            };
            if y >= cy && y < cy + hh {
                if e.kind.is_separator() || e.kind.is_info() {
                    return -1;
                }
                return i as i32;
            }
            cy += hh;
        }
        -1
    }

    fn move_hover(&mut self, delta: i32) {
        if self.entries.is_empty() {
            return;
        }
        let mut i = self.hover;
        for _ in 0..self.entries.len() {
            i += delta;
            if i < 0 {
                i = self.entries.len() as i32 - 1;
            }
            if i >= self.entries.len() as i32 {
                i = 0;
            }
            if !self.entries[i as usize].kind.is_separator()
                && !self.entries[i as usize].kind.is_info()
            {
                break;
            }
        }
        self.hover = i;
        unsafe { let _ = InvalidateRect(Some(self.hwnd), None, true); };
    }
}

fn font(face: &str, height: i32) -> HGDIOBJ {
    HGDIOBJ(unsafe { create_font(face, -height, FW_NORMAL.0 as i32) }.0)
}

unsafe fn set_rounded_corners(hwnd: HWND) {
    let v = DWMWCP_ROUNDSMALL;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &v as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&v) as u32,
        );
    }
}

unsafe fn set_border_color(hwnd: HWND, color: u32) {
    let c = COLORREF(color);
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &c as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&c) as u32,
        );
    }
}

unsafe extern "system" fn menu_wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Menu;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    }
    let m = unsafe { &mut *ptr };
    match msg {
        WM_PAINT => {
            m.on_paint();
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_SETCURSOR => {
            if (lp.0 as u32 & 0xFFFF) == HTCLIENT {
                unsafe { set_arrow_cursor() };
                return LRESULT(1);
            }
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
        WM_MOUSEMOVE => {
            let x = (lp.0 & 0xFFFF) as u16 as i16 as i32;
            let y = ((lp.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
            let h = m.hit_test(x, y);
            if h != m.hover {
                m.hover = h;
                unsafe { let _ = InvalidateRect(Some(hwnd), None, true); };
            }
            let mut tme = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                ..Default::default()
            };
            unsafe { let _ = TrackMouseEvent(&mut tme); };
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            if m.hover != -1 {
                m.hover = -1;
                unsafe { let _ = InvalidateRect(Some(hwnd), None, true); };
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            m.activate(m.hover);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            match wp.0 as u32 {
                0x26 => m.move_hover(-1), // VK_UP
                0x28 => m.move_hover(1),  // VK_DOWN
                0x0D | 0x20 => m.activate(m.hover),
                0x1B => m.hide(),
                _ => {}
            }
            LRESULT(0)
        }
        WM_ACTIVATE => {
            if (wp.0 & 0xFFFF) as u16 == WA_INACTIVE as u16 {
                if unsafe { GetTickCount() }.wrapping_sub(m.shown_at) > 250 {
                    m.hide();
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

unsafe fn load_arrow_cursor() -> HCURSOR {
    unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() }
}

unsafe fn set_arrow_cursor() {
    unsafe { SetCursor(Some(load_arrow_cursor())) };
}
