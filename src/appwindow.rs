//! Hosts the settings UI in a native Win32 window backed by the system Microsoft
//! Edge WebView2 runtime. This reuses the Edge component that ships with
//! Windows, so no browser engine is bundled.
//!
//! In the resident daemon this runs in a separate short-lived process (this same
//! executable with `-wizard-ui`) so closing the window reclaims all of its
//! memory.

use std::cell::RefCell;

use anyhow::{anyhow, Result};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{E_POINTER, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::assets;

pub struct Options {
    pub title: String,
    pub width: i32,
    pub height: i32,
    pub min_width: i32,
    pub min_height: i32,
    pub data_path: String,
}

struct WinState {
    controller: Option<ICoreWebView2Controller>,
    min_width: i32,
    min_height: i32,
}

thread_local! {
    static STATE: RefCell<Option<WinState>> = const { RefCell::new(None) };
}

pub struct Window {
    hwnd: HWND,
}

/// Creates the window and starts loading `url`. Must be called on the same
/// thread that will later call `run`.
pub fn open(url: &str, opts: &Options) -> Result<Window> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    let class = w!("seuLaborWizard");
    let hmodule = unsafe { GetModuleHandleW(None)? };
    let hinstance = HINSTANCE(hmodule.0);
    let wc = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: hinstance,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        lpszClassName: class,
        ..Default::default()
    };
    unsafe {
        RegisterClassW(&wc);
    }

    let title: Vec<u16> = opts
        .title
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let hwnd = unsafe {
        CreateWindowExW(
            Default::default(),
            class,
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            opts.width,
            opts.height,
            None,
            None,
            Some(hinstance),
            None,
        )?
    };

    set_window_icon(hwnd);

    STATE.with(|s| {
        *s.borrow_mut() = Some(WinState {
            controller: None,
            min_width: opts.min_width,
            min_height: opts.min_height,
        });
    });

    let environment = create_environment(&opts.data_path)?;
    let controller = create_controller(&environment, hwnd)?;
    let webview = unsafe { controller.CoreWebView2()? };
    unsafe {
        let settings = webview.Settings()?;
        settings.SetAreDefaultContextMenusEnabled(false)?;
        settings.SetAreDevToolsEnabled(false)?;
    }

    let (cx, cy) = client_size(hwnd);
    unsafe {
        controller.SetBounds(RECT {
            left: 0,
            top: 0,
            right: cx,
            bottom: cy,
        })?;
        controller.SetIsVisible(true)?;
    }
    STATE.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            st.controller = Some(controller);
        }
    });

    let url_w: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        webview.Navigate(PCWSTR(url_w.as_ptr()))?;
    }

    Ok(Window { hwnd })
}

impl Window {
    /// Pumps the window message loop until the window closes.
    pub fn run(&self) -> Result<()> {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = Gdi::UpdateWindow(self.hwnd);
            let _ = SetFocus(Some(self.hwnd));
        }

        let mut msg = MSG::default();
        loop {
            let r = unsafe { GetMessageW(&mut msg, None, 0, 0) }.0;
            match r {
                -1 => return Err(anyhow!("消息循环失败")),
                0 => return Ok(()),
                _ => unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                },
            }
        }
    }
}

fn create_environment(
    data_path: &str,
) -> Result<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment> {
    let data_path = data_path.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| {
            let data_w: Vec<u16> = data_path
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            unsafe {
                CreateCoreWebView2EnvironmentWithOptions(
                    PCWSTR::null(),
                    PCWSTR(data_w.as_ptr()),
                    None,
                    &handler,
                )
                .map_err(webview2_com::Error::WindowsError)
            }
        }),
        Box::new(move |error_code, environment| {
            error_code?;
            tx.send(environment.ok_or_else(|| windows::core::Error::from(E_POINTER)))
                .ok();
            Ok(())
        }),
    )
    .map_err(|e| anyhow!("创建 WebView2 环境失败: {}", e))?;

    rx.recv()
        .map_err(|_| anyhow!("创建 WebView2 环境失败"))?
        .map_err(|e| anyhow!("创建 WebView2 环境失败: {}", e))
}

fn create_controller(
    environment: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment,
    parent: HWND,
) -> Result<ICoreWebView2Controller> {
    let (tx, rx) = std::sync::mpsc::channel();
    let environment = environment.clone();
    CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            environment
                .CreateCoreWebView2Controller(parent, &handler)
                .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(move |error_code, controller| {
            error_code?;
            tx.send(controller.ok_or_else(|| windows::core::Error::from(E_POINTER)))
                .ok();
            Ok(())
        }),
    )
    .map_err(|e| anyhow!("创建 WebView2 控制器失败: {}", e))?;

    rx.recv()
        .map_err(|_| anyhow!("创建 WebView2 控制器失败"))?
        .map_err(|e| anyhow!("创建 WebView2 控制器失败: {}", e))
}

fn client_size(hwnd: HWND) -> (i32, i32) {
    let mut rect = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rect);
    }
    (rect.right - rect.left, rect.bottom - rect.top)
}

fn set_window_icon(hwnd: HWND) {
    let icon = unsafe { crate::ui::winapi::create_icon_from_ico(assets::ICON_ICO, 32) };
    if icon != 0 {
        let h = hwnd.0 as isize;
        unsafe {
            crate::ui::winapi::SendMessageW(h, WM_SETICON, ICON_BIG as usize, icon);
            crate::ui::winapi::SendMessageW(h, WM_SETICON, ICON_SMALL as usize, icon);
        }
    }
}

extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_SIZE => {
            let (cx, cy) = client_size(hwnd);
            STATE.with(|s| {
                if let Some(st) = s.borrow().as_ref() {
                    if let Some(c) = &st.controller {
                        let _ = unsafe {
                            c.SetBounds(RECT {
                                left: 0,
                                top: 0,
                                right: cx,
                                bottom: cy,
                            })
                        };
                    }
                }
            });
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            STATE.with(|s| {
                if let Some(st) = s.borrow().as_ref() {
                    let mmi = lparam.0 as *mut MINMAXINFO;
                    if !mmi.is_null() {
                        unsafe {
                            (*mmi).ptMinTrackSize.x = st.min_width;
                            (*mmi).ptMinTrackSize.y = st.min_height;
                        }
                    }
                }
            });
            LRESULT(0)
        }
        WM_CLOSE => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
