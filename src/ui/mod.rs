//! The resident tray icon and its Fluent popup menu. The look and behaviour are
//! a manual Win32 replica of the original implementation.

pub mod menu;
pub mod winapi;

use std::sync::atomic::{AtomicUsize, Ordering};
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
    LoadCursorW, LoadImageW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW,
    TranslateMessage, HICON, IDC_ARROW, IMAGE_FLAGS, IMAGE_ICON, MSG,
    SM_CXSMICON, SM_CYSMICON, WM_APP, WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY, WM_LBUTTONUP,
    WM_RBUTTONUP, WNDCLASSEXW, WINDOW_EX_STYLE, WINDOW_STYLE,
};

use crate::assets;
use menu::{current_menu_ptr, set_current_menu, Menu};
use winapi::{copy_into, create_icon_from_ico, module_handle, utf16};

pub type Callback = Arc<dyn Fn() + Send + Sync>;
pub type BoolGetter = Arc<dyn Fn() -> bool + Send + Sync>;
pub type StringGetter = Arc<dyn Fn() -> String + Send + Sync>;
pub type LinesGetter = Arc<dyn Fn() -> Vec<String> + Send + Sync>;

/// The callbacks the tray menu invokes. The status and toggle getters are
/// evaluated when the menu is about to be shown, so labels and check marks
/// always reflect current state.
#[derive(Clone)]
pub struct Actions {
    pub status_text: StringGetter,
    pub status_lines: LinesGetter,
    pub version: String,
    pub on_open_course: Callback,
    pub on_login: Callback,
    pub on_fetch_now: Callback,
    pub on_open_wizard: Callback,
    pub on_open_config: Callback,
    pub on_open_logs: Callback,
    pub is_auto_start: BoolGetter,
    pub toggle_auto_start: Callback,
    pub is_auto_select: BoolGetter,
    pub toggle_auto_select: Callback,
    pub on_quit: Callback,
}

static CURRENT_TRAY: AtomicUsize = AtomicUsize::new(0);

fn current_tray_ptr() -> *mut Tray {
    CURRENT_TRAY.load(Ordering::SeqCst) as *mut Tray
}

pub struct Tray {
    pub actions: Actions,
    pub hwnd: HWND,
    pub icon: HICON,
    pub callback: u32,
    pub taskbar_id: u32,
    pub added: bool,
    pub menu: *mut Menu,
    /// Reserved for a future graceful shutdown (the app currently exits via
    /// `process::exit`, so the flag is not set yet).
    pub stopped: bool,
}

/// Owns the leaked tray state and offers thread-safe operations.
#[derive(Clone, Copy)]
pub struct TrayHandle(pub *mut Tray);

unsafe impl Send for TrayHandle {}
unsafe impl Sync for TrayHandle {}

impl TrayHandle {
    /// Prepares the tray icon and menu. Must be called on the thread that will
    /// later call `run` (the message-loop thread).
    pub fn new(actions: Actions) -> Result<TrayHandle> {
        let hinst = module_handle();

        let cx = unsafe { GetSystemMetrics(SM_CXSMICON) };
        let cy = unsafe { GetSystemMetrics(SM_CYSMICON) };
        let mut icon = unsafe { create_icon_from_ico(assets::ICON_ICO, cx) };
        if icon.0.is_null() {
            icon = unsafe {
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

        let cls = utf16("seuLaborTrayMsg");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            hInstance: hinst,
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() },
            lpfnWndProc: Some(tray_wnd_proc),
            lpszClassName: PCWSTR(cls.as_ptr()),
            ..Default::default()
        };
        if unsafe { RegisterClassExW(&wc) } == 0 {
            return Err(anyhow!("创建托盘消息窗口失败"));
        }

        let title = utf16("SEU 劳动教育课程监控");
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

        let mut tray = Box::new(Tray {
            actions,
            hwnd,
            icon,
            callback,
            taskbar_id,
            added: false,
            menu: std::ptr::null_mut(),
            stopped: false,
        });
        let tray_ptr: *mut Tray = &mut *tray;
        let menu_ptr = match Menu::new(tray_ptr) {
            Some(m) => Box::into_raw(m),
            None => {
                let _ = unsafe { DestroyWindow(hwnd) };
                return Err(anyhow!("创建菜单窗口失败"));
            }
        };
        tray.menu = menu_ptr;
        let ptr = Box::into_raw(tray);
        CURRENT_TRAY.store(ptr as usize, Ordering::SeqCst);
        set_current_menu(menu_ptr);

        let handle = TrayHandle(ptr);
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
        copy_into(&mut nid.szTip, &(t.actions.status_text)());
        if !unsafe { Shell_NotifyIconW(NIM_ADD, &nid) }.as_bool() {
            return Err(anyhow!("添加托盘图标失败"));
        }
        t.added = true;
        Ok(())
    }

    /// Removes the tray icon. Reserved for a future graceful shutdown.
    #[allow(dead_code)]
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
        copy_into(&mut nid.szTip, &(t.actions.status_text)());
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

    /// Removes the icon and quits the message loop. Reserved for a future
    /// graceful shutdown.
    #[allow(dead_code)]
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
    let ptr = current_tray_ptr();
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    }
    let t = unsafe { &mut *ptr };
    if msg == t.callback {
        match lp.0 as u32 {
            WM_LBUTTONUP => {
                let menu = current_menu_ptr();
                if !menu.is_null() {
                    unsafe { (*menu).hide() };
                }
                let action = t.actions.on_open_course.clone();
                std::thread::spawn(move || action());
            }
            WM_RBUTTONUP | WM_CONTEXTMENU => {
                let menu = current_menu_ptr();
                if !menu.is_null() {
                    unsafe { (*menu).show() };
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
                let _ = TrayHandle(ptr).add_icon();
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
    }
}
