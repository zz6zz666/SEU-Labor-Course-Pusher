//! The custom-drawn Fluent-style popup menu. A manual Win32 replica: a layered
//! popup window with rounded corners, a subtle border, hover highlight and a
//! leading icon/check gutter.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::winapi::*;
use super::{BoolGetter, Callback, Tray};

static CURRENT_MENU: AtomicUsize = AtomicUsize::new(0);

pub fn current_menu_ptr() -> *mut Menu {
    CURRENT_MENU.load(Ordering::SeqCst) as *mut Menu
}

pub fn set_current_menu(m: *mut Menu) {
    CURRENT_MENU.store(m as usize, Ordering::SeqCst);
}

/// One row of the popup menu.
struct MenuEntry {
    label: String,
    glyph: u16,
    sep: bool,
    info: bool,
    action: Option<Callback>,
    checked: Option<BoolGetter>,
}

impl MenuEntry {
    fn plain(label: &str, glyph: u16, action: Callback) -> MenuEntry {
        MenuEntry {
            label: label.to_string(),
            glyph,
            sep: false,
            info: false,
            action: Some(action),
            checked: None,
        }
    }
    fn toggle(label: &str, action: Callback, checked: BoolGetter) -> MenuEntry {
        MenuEntry {
            label: label.to_string(),
            glyph: 0,
            sep: false,
            info: false,
            action: Some(action),
            checked: Some(checked),
        }
    }
    fn separator() -> MenuEntry {
        MenuEntry {
            label: String::new(),
            glyph: 0,
            sep: true,
            info: false,
            action: None,
            checked: None,
        }
    }
    fn info_line(label: String) -> MenuEntry {
        MenuEntry {
            label,
            glyph: 0,
            sep: false,
            info: true,
            action: None,
            checked: None,
        }
    }
}

pub struct Menu {
    pub tray: *mut Tray,
    pub hwnd: HWND,
    entries: Vec<MenuEntry>,
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
}

impl Menu {
    pub fn new(tray: *mut Tray) -> Option<Box<Menu>> {
        unsafe {
            let hinst = get_module_handle();
            let cls = utf16("seuLaborMenu");
            let mut wc: WNDCLASSEXW = std::mem::zeroed();
            wc.cb_size = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.style = CS_DROPSHADOW;
            wc.h_instance = hinst;
            wc.h_cursor = load_arrow_cursor();
            wc.lpfn_wnd_proc = Some(menu_wnd_proc);
            wc.lpsz_class_name = cls.as_ptr();
            if RegisterClassExW(&wc) == 0 {
                return None;
            }
            let name = utf16("menu");
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_LAYERED,
                cls.as_ptr(),
                name.as_ptr(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                0,
                0,
                hinst,
                std::ptr::null_mut(),
            );
            if hwnd == 0 {
                return None;
            }
            let m = Box::new(Menu {
                tray,
                hwnd,
                entries: Vec::new(),
                hover: -1,
                dpi: 0,
                scale: 1.0,
                text_font: 0,
                info_font: 0,
                icon_font: 0,
                pad_x: 0,
                pad_y: 0,
                item_h: 0,
                sep_h: 0,
                icon_col: 0,
                radius: 0,
                shown_at: 0,
            });
            set_rounded_corners(hwnd);
            let ptr = Box::into_raw(m);
            set_current_menu(ptr);
            Some(Box::from_raw(ptr))
        }
    }

    fn build_entries(&mut self) {
        let actions = unsafe { &(*self.tray).actions };
        self.entries.clear();

        for line in (actions.status_lines)() {
            if line.is_empty() {
                continue;
            }
            self.entries.push(MenuEntry::info_line(line));
        }
        if !self.entries.is_empty() {
            self.entries.push(MenuEntry::separator());
        }

        self.entries
            .push(MenuEntry::plain("打开选课页", 0xE774, actions.on_open_course.clone()));
        self.entries
            .push(MenuEntry::plain("重新登录", 0xE72E, actions.on_login.clone()));
        self.entries.push(MenuEntry::separator());
        self.entries
            .push(MenuEntry::plain("立即抓取一次", 0xE72C, actions.on_fetch_now.clone()));
        self.entries
            .push(MenuEntry::plain("打开日志目录", 0xE8B7, actions.on_open_logs.clone()));
        self.entries
            .push(MenuEntry::plain("打开 config.json", 0xE8A5, actions.on_open_config.clone()));
        self.entries
            .push(MenuEntry::plain("设置向导", 0xE713, actions.on_open_wizard.clone()));
        self.entries.push(MenuEntry::separator());
        self.entries.push(MenuEntry::toggle(
            "开机自启",
            actions.toggle_auto_start.clone(),
            actions.is_auto_start.clone(),
        ));
        self.entries.push(MenuEntry::toggle(
            "自动选课",
            actions.toggle_auto_select.clone(),
            actions.is_auto_select.clone(),
        ));
        self.entries.push(MenuEntry::separator());
        if !actions.version.is_empty() {
            self.entries
                .push(MenuEntry::info_line(format!("版本 {}", actions.version)));
        }
        self.entries
            .push(MenuEntry::plain("退出", 0xE8BB, actions.on_quit.clone()));
    }

    fn ensure_dpi(&mut self) {
        unsafe {
            let dpi = dpi_for_window(self.hwnd);
            if dpi == self.dpi && self.text_font != 0 {
                return;
            }
            self.dpi = dpi;
            self.scale = dpi as f64 / 96.0;
            if self.text_font != 0 {
                DeleteObject(self.text_font);
            }
            if self.info_font != 0 {
                DeleteObject(self.info_font);
            }
            if self.icon_font != 0 {
                DeleteObject(self.icon_font);
            }
            self.text_font = create_font(
                "Segoe UI Variable Text",
                -((12.0 * self.scale).round() as i32),
                FW_NORMAL,
            );
            self.info_font = create_font(
                "Segoe UI Variable Text",
                -((11.0 * self.scale).round() as i32),
                FW_NORMAL,
            );
            self.icon_font = create_font(
                "Segoe MDL2 Assets",
                -((14.0 * self.scale).round() as i32),
                FW_NORMAL,
            );
            self.pad_x = (14.0 * self.scale).round() as i32;
            self.pad_y = (6.0 * self.scale).round() as i32;
            self.item_h = (29.0 * self.scale).round() as i32;
            self.sep_h = (10.0 * self.scale).round() as i32;
            self.icon_col = (31.0 * self.scale).round() as i32;
            self.radius = (6.0 * self.scale).round() as i32;
        }
    }

    fn measure(&mut self) -> (i32, i32) {
        unsafe {
            self.ensure_dpi();
            let dc = GetDC(self.hwnd);
            let mut max_w = 0;
            for e in &self.entries {
                if e.sep {
                    continue;
                }
                let font = if e.info { self.info_font } else { self.text_font };
                SelectObject(dc, font);
                let w = text_width(dc, &e.label);
                if w > max_w {
                    max_w = w;
                }
            }
            ReleaseDC(self.hwnd, dc);

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
                height += if e.sep { self.sep_h } else { self.item_h };
            }
            (width, height)
        }
    }

    pub fn show(&mut self) {
        unsafe {
            self.build_entries();
            let (w, h) = self.measure();

            let mut pt = POINT::default();
            GetCursorPos(&mut pt);
            let x = pt.x - w + (8.0 * self.scale).round() as i32;
            let y = pt.y - h - (8.0 * self.scale).round() as i32;
            let (x, y) = clamp_to_work_area(x, y, w, h, pt);

            SetWindowPos(self.hwnd, 0, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
            set_rounded_corners(self.hwnd);
            set_border_color(self.hwnd, 0x00E4E4E4);
            set_layered_alpha(self.hwnd, 240);
            self.hover = -1;
            self.shown_at = GetTickCount();

            ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            SetForegroundWindow(self.hwnd);
            InvalidateRect(self.hwnd, std::ptr::null(), 1);
        }
    }

    pub fn hide(&self) {
        unsafe {
            if self.hwnd != 0 {
                ShowWindow(self.hwnd, SW_HIDE);
            }
            SetForegroundWindow((*self.tray).hwnd);
        }
    }

    fn activate(&mut self, i: i32) {
        if i < 0 || i as usize >= self.entries.len() {
            return;
        }
        let e = &self.entries[i as usize];
        if e.sep || e.info {
            return;
        }
        let Some(action) = e.action.clone() else {
            return;
        };
        // Toggles keep the menu open so the check updates in place and another
        // option can be flipped immediately; a click elsewhere dismisses it.
        if e.checked.is_some() {
            let hwnd = self.hwnd;
            std::thread::spawn(move || {
                action();
                unsafe {
                    InvalidateRect(hwnd, std::ptr::null(), 1);
                }
            });
            return;
        }
        self.hide();
        std::thread::spawn(move || action());
    }

    fn on_paint(&mut self) {
        unsafe {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(self.hwnd, &mut ps);
            if hdc == 0 {
                return;
            }
            let mut cr = RECT::default();
            GetClientRect(self.hwnd, &mut cr);
            let w = cr.right - cr.left;
            let h = cr.bottom - cr.top;
            if w <= 0 || h <= 0 {
                EndPaint(self.hwnd, &ps);
                return;
            }
            let mem = CreateCompatibleDC(hdc);
            let bmp = CreateCompatibleBitmap(hdc, w, h);
            let old = SelectObject(mem, bmp);
            self.paint_into(mem, w, h);
            BitBlt(hdc, 0, 0, w, h, mem, 0, 0, SRCCOPY);
            SelectObject(mem, old);
            DeleteObject(bmp);
            DeleteDC(mem);
            EndPaint(self.hwnd, &ps);
        }
    }

    fn paint_into(&self, hdc: HDC, w: i32, h: i32) {
        unsafe {
            let full = RECT {
                left: 0,
                top: 0,
                right: w,
                bottom: h,
            };
            let bg = CreateSolidBrush(0x00F9F9F9);
            FillRect(hdc, &full, bg);
            DeleteObject(bg);
            SetBkMode(hdc, TRANSPARENT);

            let mut y = self.pad_y;
            for (i, e) in self.entries.iter().enumerate() {
                if e.sep {
                    let inset = (10.0 * self.scale).round() as i32;
                    let line = RECT {
                        left: self.pad_x + inset,
                        right: w - self.pad_x - inset,
                        top: y + self.sep_h / 2,
                        bottom: y + self.sep_h / 2 + 1,
                    };
                    let sep = CreateSolidBrush(0x00E4E4E4);
                    FillRect(hdc, &line, sep);
                    DeleteObject(sep);
                    y += self.sep_h;
                    continue;
                }

                let row = RECT {
                    left: self.pad_x,
                    top: y,
                    right: w - self.pad_x,
                    bottom: y + self.item_h,
                };

                if e.info {
                    let mut label = RECT {
                        left: row.left + self.icon_col,
                        top: row.top,
                        right: row.right,
                        bottom: row.bottom,
                    };
                    SelectObject(hdc, self.info_font);
                    SetTextColor(hdc, 0x00808080);
                    draw_text(
                        hdc,
                        &e.label,
                        &mut label,
                        DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                    );
                    y += self.item_h;
                    continue;
                }

                if i as i32 == self.hover {
                    let hb = CreateSolidBrush(0x00EBEBEB);
                    let pen = CreatePen(PS_SOLID, 1, 0x00EBEBEB);
                    let old_pen = SelectObject(hdc, pen);
                    let old_brush = SelectObject(hdc, hb);
                    RoundRect(
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
                    DeleteObject(pen);
                    DeleteObject(hb);
                }

                // Leading gutter: a check for toggles, an icon otherwise.
                let mut gutter = RECT {
                    left: row.left,
                    top: row.top,
                    right: row.left + self.icon_col,
                    bottom: row.bottom,
                };
                if let Some(checked) = &e.checked {
                    if checked() {
                        SelectObject(hdc, self.icon_font);
                        SetTextColor(hdc, 0x00EB6F2F); // #2F6FEB
                        let ch = char::from_u32(0xE73E).unwrap().to_string();
                        draw_text(
                            hdc,
                            &ch,
                            &mut gutter,
                            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                        );
                    }
                } else if e.glyph != 0 {
                    SelectObject(hdc, self.icon_font);
                    SetTextColor(hdc, 0x00666666);
                    let ch = char::from_u32(e.glyph as u32).unwrap().to_string();
                    draw_text(
                        hdc,
                        &ch,
                        &mut gutter,
                        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                    );
                }

                let mut label = RECT {
                    left: row.left + self.icon_col,
                    top: row.top,
                    right: row.right,
                    bottom: row.bottom,
                };
                SelectObject(hdc, self.text_font);
                SetTextColor(hdc, 0x001A1A1A);
                draw_text(
                    hdc,
                    &e.label,
                    &mut label,
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );

                y += self.item_h;
            }
        }
    }

    fn hit_test(&self, x: i32, y: i32) -> i32 {
        if x < self.pad_x {
            return -1;
        }
        let mut cy = self.pad_y;
        for (i, e) in self.entries.iter().enumerate() {
            let hh = if e.sep { self.sep_h } else { self.item_h };
            if y >= cy && y < cy + hh {
                if e.sep || e.info {
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
            let e = &self.entries[i as usize];
            if !e.sep && !e.info {
                break;
            }
        }
        self.hover = i;
        unsafe {
            InvalidateRect(self.hwnd, std::ptr::null(), 1);
        }
    }
}

unsafe extern "system" fn menu_wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let ptr = current_menu_ptr();
    if ptr.is_null() || (*ptr).hwnd != hwnd {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let m = &mut *ptr;
    match msg {
        WM_PAINT => {
            m.on_paint();
            0
        }
        WM_ERASEBKGND => 1,
        WM_SETCURSOR => {
            if (lp as u32 & 0xFFFF) == HTCLIENT {
                set_arrow_cursor();
                return 1;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_MOUSEMOVE => {
            let x = (lp & 0xFFFF) as u16 as i16 as i32;
            let y = ((lp >> 16) & 0xFFFF) as u16 as i16 as i32;
            let h = m.hit_test(x, y);
            if h != m.hover {
                m.hover = h;
                InvalidateRect(hwnd, std::ptr::null(), 1);
            }
            let mut tme: TRACKMOUSEEVENT = std::mem::zeroed();
            tme.cb_size = std::mem::size_of::<TRACKMOUSEEVENT>() as u32;
            tme.dw_flags = TME_LEAVE;
            tme.hwnd_track = hwnd;
            TrackMouseEvent(&mut tme);
            0
        }
        WM_MOUSELEAVE => {
            if m.hover != -1 {
                m.hover = -1;
                InvalidateRect(hwnd, std::ptr::null(), 1);
            }
            0
        }
        WM_LBUTTONUP => {
            m.activate(m.hover);
            0
        }
        WM_KEYDOWN => {
            match wp as u32 {
                0x26 => m.move_hover(-1), // VK_UP
                0x28 => m.move_hover(1),  // VK_DOWN
                0x0D | 0x20 => m.activate(m.hover),
                0x1B => m.hide(),
                _ => {}
            }
            0
        }
        WM_ACTIVATE => {
            if (wp & 0xFFFF) as u16 == WA_INACTIVE {
                if GetTickCount().wrapping_sub(m.shown_at) > 250 {
                    m.hide();
                }
            }
            0
        }
        WM_CLOSE => {
            ShowWindow(hwnd, SW_HIDE);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe fn load_arrow_cursor() -> HCURSOR {
    LoadCursorW(0, IDC_ARROW as *const u16)
}

unsafe fn set_arrow_cursor() {
    SetCursor(load_arrow_cursor());
}
