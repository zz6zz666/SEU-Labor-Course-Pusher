//! The resident tray icon and its message window. Menu content is supplied by
//! the host as data (see [`MenuItem`]); this module only owns the mechanism.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, GetSystemMetrics,
    GetWindowLongPtrW, LoadCursorW, LoadImageW, PostQuitMessage, RegisterClassExW,
    RegisterWindowMessageW, SetWindowLongPtrW, TranslateMessage, GWLP_USERDATA, HICON, IDC_ARROW,
    IMAGE_FLAGS, IMAGE_ICON, MSG, SM_CXSMICON, SM_CYSMICON, WM_APP, WM_CLOSE, WM_CONTEXTMENU,
    WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSEXW, WINDOW_EX_STYLE, WINDOW_STYLE,
};

use crate::menu::Menu;
use crate::winapi::{copy_into, module_handle, utf16};

pub type BoolFn = Arc<dyn Fn() -> bool + Send + Sync>;
pub type StringFn = Arc<dyn Fn() -> String + Send + Sync>;
pub type Callback = Arc<dyn Fn() + Send + Sync>;
pub type CommandFn = Arc<dyn Fn(u32) + Send + Sync>;
pub type ItemsFn = Arc<dyn Fn() -> Vec<MenuItem> + Send + Sync>;

/// What a menu row is.
pub enum ItemKind {
    /// Clickable command; its id is delivered to `on_command`.
    Command,
    /// Clickable toggle with a live check mark; clicking keeps the menu open so
    /// the check updates in place.
    Toggle(BoolFn),
    /// A non-clickable status line rendered in grey.
    Info,
    /// A horizontal rule.
    Separator,
}

impl ItemKind {
    pub(crate) fn is_separator(&self) -> bool {
        matches!(self, ItemKind::Separator)
    }
    pub(crate) fn is_info(&self) -> bool {
        matches!(self, ItemKind::Info)
    }
}

/// One row of the popup menu. Build these on every show so labels and check
/// marks reflect current state.
pub struct MenuItem {
    pub id: u32,
    pub label: String,
    pub glyph: u16,
    pub kind: ItemKind,
}

impl MenuItem {
    pub fn command(id: u32, label: impl Into<String>, glyph: u16) -> Self {
        Self {
            id,
            label: label.into(),
            glyph,
            kind: ItemKind::Command,
        }
    }
    pub fn toggle(id: u32, label: impl Into<String>, checked: BoolFn) -> Self {
        Self {
            id,
            label: label.into(),
            glyph: 0,
            kind: ItemKind::Toggle(checked),
        }
    }
    pub fn info(label: impl Into<String>) -> Self {
        Self {
            id: 0,
            label: label.into(),
            glyph: 0,
            kind: ItemKind::Info,
        }
    }
    pub fn separator() -> Self {
        Self {
            id: 0,
            label: String::new(),
            glyph: 0,
            kind: ItemKind::Separator,
        }
    }
}

/// The host-supplied policy for a tray: its icon, tooltip, rows and callbacks.
pub struct TrayConfig {
    pub icon_ico: &'static [u8],
    pub tooltip: StringFn,
    pub items: ItemsFn,
    pub on_command: CommandFn,
    pub on_left_click: Callback,
}

/// The leaked tray state. Shared with the popup menu via raw pointer.
pub(crate) struct TrayInner {
    pub(crate) tooltip: StringFn,
    pub(crate) items: ItemsFn,
    pub(crate) on_command: CommandFn,
    pub(crate) on_left_click: Callback,
    pub(crate) hwnd: HWND,
    pub(crate) icon: HICON,
    pub(crate) callback: u32,
    pub(crate) taskbar_id: u32,
    pub(crate) added: bool,
    pub(crate) menu: *mut Menu,
    pub(crate) stopped: bool,
}

/// Owns the leaked tray state and offers thread-safe operations.
#[derive(Clone, Copy)]
pub struct Tray(pub(crate) *mut TrayInner);

unsafe impl Send for Tray {}
unsafe impl Sync for Tray {}

impl Tray {
    /// Creates the tray icon and its popup menu. Must be called on the thread
    /// that will later call [`Tray::run`] (the message-loop thread).
    pub fn new(cfg: TrayConfig) -> Result<Tray> {
        // The popup is drawn at the cursor monitor's scale; skip this and Windows
        // would stretch the unaware popup.
        winkit::enable_per_monitor_dpi();
        let hinst = module_handle();

        let cx = unsafe { GetSystemMetrics(SM_CXSMICON) };
        let cy = unsafe { GetSystemMetrics(SM_CYSMICON) };
        let mut hicon = winkit::from_ico(cfg.icon_ico, cx);
        if hicon.0.is_null() {
            hicon = unsafe {
                LoadImageW(
                    Some(hinst),
                    PCWSTR(32512 as *const u16),
                    IMAGE_ICON,
                    cx,
                    cy,
                    IMAGE_FLAGS(0),
                )
            }
            .map(|h| HICON(h.0))
            .unwrap_or_default();
        }

        let callback = WM_APP + 1;
        let taskbar = utf16("TaskbarCreated");
        let taskbar_id = unsafe { RegisterWindowMessageW(PCWSTR(taskbar.as_ptr())) };

        let cls = utf16("traykit_tray");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            hInstance: hinst,
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() },
            lpfnWndProc: Some(tray_wnd_proc),
            lpszClassName: PCWSTR(cls.as_ptr()),
            ..Default::default()
        };
        // A second tray in the same process re-registers the same class, which
        // is harmless; `CreateWindowExW` below is the real success gate.
        unsafe { RegisterClassExW(&wc) };

        let title = utf16("traykit");
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(cls.as_ptr()),
                PCWSTR(title.as_ptr()),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                None,
                None,
                Some(hinst),
                None,
            )
        }?;

        let mut inner = Box::new(TrayInner {
            tooltip: cfg.tooltip,
            items: cfg.items,
            on_command: cfg.on_command,
            on_left_click: cfg.on_left_click,
            hwnd,
            icon: hicon,
            callback,
            taskbar_id,
            added: false,
            menu: std::ptr::null_mut(),
            stopped: false,
        });
        let inner_ptr: *mut TrayInner = &mut *inner;
        let menu_ptr = match Menu::new(inner_ptr) {
            Some(m) => Box::into_raw(m),
            None => {
                let _ = unsafe { DestroyWindow(hwnd) };
                return Err(anyhow!("failed to create the popup menu"));
            }
        };
        inner.menu = menu_ptr;
        let ptr = Box::into_raw(inner);
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as isize) };

        let handle = Tray(ptr);
        handle.add_icon()?;
        Ok(handle)
    }

    fn add_icon(&self) -> Result<()> {
        let t = unsafe { &mut *self.0 };
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: t.hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: t.callback,
            hIcon: t.icon,
            ..Default::default()
        };
        copy_into(&mut nid.szTip, &(t.tooltip)());
        if !unsafe { Shell_NotifyIconW(NIM_ADD, &nid) }.as_bool() {
            return Err(anyhow!("failed to add the tray icon"));
        }
        t.added = true;
        Ok(())
    }

    /// Removes the tray icon.
    fn remove_icon(&self) {
        let t = unsafe { &mut *self.0 };
        if !t.added {
            return;
        }
        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: t.hwnd,
            uID: 1,
            ..Default::default()
        };
        let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) };
        t.added = false;
    }

    /// Refreshes the tooltip. Safe to call from any thread.
    pub fn update(&self) {
        let t = unsafe { &*self.0 };
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: t.hwnd,
            uID: 1,
            uFlags: NIF_TIP,
            ..Default::default()
        };
        copy_into(&mut nid.szTip, &(t.tooltip)());
        let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
    }

    /// Blocks on the tray message loop; must run on the main thread.
    pub fn run(&self) -> i32 {
        let mut msg = MSG::default();
        loop {
            let r = unsafe { GetMessageW(&mut msg, None, 0, 0) }.0;
            if r <= 0 {
                return r;
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    /// Removes the icon and quits the message loop.
    pub fn stop(&self) {
        let t = unsafe { &mut *self.0 };
        if t.stopped {
            return;
        }
        t.stopped = true;
        self.remove_icon();
        unsafe { PostQuitMessage(0) };
    }
}

unsafe extern "system" fn tray_wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut TrayInner;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    }
    let t = unsafe { &mut *ptr };
    if msg == t.callback {
        match lp.0 as u32 {
            WM_LBUTTONUP => {
                if !t.menu.is_null() {
                    unsafe { (*t.menu).hide() };
                }
                let action = t.on_left_click.clone();
                std::thread::spawn(move || action());
            }
            WM_RBUTTONUP | WM_CONTEXTMENU => {
                if !t.menu.is_null() {
                    unsafe { (*t.menu).show() };
                }
            }
            _ => {}
        }
        return LRESULT(0);
    }
    match msg {
        WM_CLOSE => {
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => {
            if t.taskbar_id != 0 && msg == t.taskbar_id {
                // Explorer restarted: re-add the icon.
                t.added = false;
                let _ = Tray(ptr).add_icon();
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
    }
}
