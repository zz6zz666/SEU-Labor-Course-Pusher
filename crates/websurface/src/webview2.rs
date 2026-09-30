//! An embedded Microsoft Edge WebView2 surface: a native Win32 window we own,
//! with the WebView2 runtime embedded in it. All calls must happen on the
//! thread that created the surface (the host's UI thread).

use std::sync::mpsc;

use anyhow::{anyhow, Result};
use serde_json::Value;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    ExecuteScriptCompletedHandler,
};
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{E_POINTER, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi;
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::{Caps, Surface, SurfaceConfig, SurfaceKind};

pub struct WebView2Surface {
    hwnd: HWND,
    controller: Option<ICoreWebView2Controller>,
    min_width: i32,
    min_height: i32,
}

/// Whether the machine has a usable WebView2 runtime.
pub fn is_available() -> bool {
    let mut version = PWSTR::null();
    let available = unsafe {
        GetAvailableCoreWebView2BrowserVersionString(PCWSTR::null(), &mut version).is_ok()
    };
    if !version.is_null() {
        unsafe { CoTaskMemFree(Some(version.0 as *const core::ffi::c_void)) };
    }
    available
}

impl WebView2Surface {
    /// Creates the window + controller and starts loading `cfg.url`.
    pub fn open(cfg: SurfaceConfig) -> Result<Box<dyn Surface>> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }

        let class = w!("websurface_wv2");
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

        // The Win32 window is sized in physical pixels.
        let scale = winkit::dpi_scale();
        let dim = |v: i32| ((v as f64) * scale).round() as i32;
        let title: Vec<u16> = cfg.title.encode_utf16().chain(std::iter::once(0)).collect();
        let hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                class,
                PCWSTR(title.as_ptr()),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                dim(cfg.logical_width),
                dim(cfg.logical_height),
                None,
                None,
                Some(hinstance),
                None,
            )?
        };

        let mut this = Box::new(WebView2Surface {
            hwnd,
            controller: None,
            min_width: dim(cfg.min_width),
            min_height: dim(cfg.min_height),
        });
        // Keep the box's heap address stable: the wndproc recovers this pointer
        // from `GWLP_USERDATA`, and an `Box<dyn Surface>` unsize coercion below
        // preserves it.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, &mut *this as *mut WebView2Surface as isize);
        }
        winkit::set_window_icon(hwnd, cfg.icon_ico);

        let environment = create_environment(&cfg.data_dir.to_string_lossy())?;
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
        this.controller = Some(controller);

        let url_w: Vec<u16> = cfg.url.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            webview.Navigate(PCWSTR(url_w.as_ptr()))?;
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = Gdi::UpdateWindow(hwnd);
            let _ = SetFocus(Some(hwnd));
        }

        let surface: Box<dyn Surface> = this;
        Ok(surface)
    }

    fn webview(&self) -> Result<ICoreWebView2> {
        let c = self
            .controller
            .as_ref()
            .ok_or_else(|| anyhow!("WebView2 is not ready yet"))?;
        Ok(unsafe { c.CoreWebView2() }?)
    }
}

impl Surface for WebView2Surface {
    fn kind(&self) -> SurfaceKind {
        SurfaceKind::Embedded
    }

    fn caps(&self) -> Caps {
        Caps {
            embedded: true,
            can_push: true,
            fixed_size: false,
        }
    }

    fn navigate(&mut self, url: &str) -> Result<()> {
        let webview = self.webview()?;
        let u: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe { webview.Navigate(PCWSTR(u.as_ptr()))? };
        Ok(())
    }

    fn eval(&mut self, js: &str) -> Result<Value> {
        let webview = self.webview()?;
        let js_w: Vec<u16> = js.encode_utf16().chain(std::iter::once(0)).collect();
        let (tx, rx) = mpsc::channel::<String>();
        ExecuteScriptCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| unsafe {
                webview
                    .ExecuteScript(PCWSTR(js_w.as_ptr()), &handler)
                    .map_err(webview2_com::Error::WindowsError)
            }),
            Box::new(move |error_code, text: String| {
                error_code?;
                let _ = tx.send(text);
                Ok(())
            }),
        )
        .map_err(|e| anyhow!("script execution failed: {}", e))?;
        let text = rx.recv().map_err(|_| anyhow!("script execution failed"))?;
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    /// Uses the native WebView2 message channel (no eval round-trip).
    fn post(&mut self, name: &str, data: Value) -> Result<()> {
        let webview = self.webview()?;
        let msg =
            serde_json::json!({ "__deliver": true, "name": name, "data": data }).to_string();
        let m: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe { webview.PostWebMessageAsJson(PCWSTR(m.as_ptr()))? };
        Ok(())
    }

    fn is_alive(&mut self) -> bool {
        unsafe { IsWindow(Some(self.hwnd)).as_bool() }
    }

    fn close(&mut self) {
        unsafe {
            let _ = PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }

    fn hwnd(&self) -> Option<isize> {
        Some(self.hwnd.0 as isize)
    }
}

impl Drop for WebView2Surface {
    fn drop(&mut self) {
        // Tells the WebView2 runtime to tear the web content down instead of
        // relying on process exit, so repeatedly opening/closing surfaces in one
        // process does not accumulate runtime state.
        if let Some(controller) = self.controller.take() {
            unsafe {
                let _ = controller.Close();
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
    .map_err(|e| anyhow!("failed to create the WebView2 environment: {}", e))?;

    rx.recv()
        .map_err(|_| anyhow!("failed to create the WebView2 environment"))?
        .map_err(|e| anyhow!("failed to create the WebView2 environment: {}", e))
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
    .map_err(|e| anyhow!("failed to create the WebView2 controller: {}", e))?;

    rx.recv()
        .map_err(|_| anyhow!("failed to create the WebView2 controller"))?
        .map_err(|e| anyhow!("failed to create the WebView2 controller: {}", e))
}

fn client_size(hwnd: HWND) -> (i32, i32) {
    let mut rect = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rect);
    }
    (rect.right - rect.left, rect.bottom - rect.top)
}

extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut WebView2Surface;
    match msg {
        WM_SIZE => {
            if !ptr.is_null() {
                let (cx, cy) = client_size(hwnd);
                let s = unsafe { &mut *ptr };
                if let Some(c) = &s.controller {
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
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            if !ptr.is_null() {
                let s = unsafe { &*ptr };
                let mmi = lparam.0 as *mut MINMAXINFO;
                if !mmi.is_null() {
                    unsafe {
                        (*mmi).ptMinTrackSize.x = s.min_width;
                        (*mmi).ptMinTrackSize.y = s.min_height;
                    }
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_DESTROY => {
            // The window is going away: drop the back-pointer so a late message
            // can never dereference a freed surface.
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
