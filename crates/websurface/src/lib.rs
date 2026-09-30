//! A multi-window host for web-technology UIs.
//!
//! A [`Surface`] is one window rendering one page. Two engines implement it:
//! an embedded WebView2 ([`webview2`]) and a borrowed installed Chromium
//! ([`borrowed`]). [`WebHost`] owns the surfaces on a single UI thread, pumps
//! the message loop, and lets other threads push events to any surface.
//!
//! ```no_run
//! use websurface::{Engine, SurfaceConfig, WebHost};
//!
//! let mut host = WebHost::new()?;
//! let id = host.open(
//!     SurfaceConfig {
//!         url: "https://example.com".into(),
//!         title: "Demo".into(),
//!         logical_width: 800,
//!         logical_height: 600,
//!         min_width: 400,
//!         min_height: 300,
//!         icon_ico: &[],
//!         data_dir: std::env::temp_dir(),
//!         browser_override: None,
//!         profile_name: "demo".into(),
//!         chromeless: true,
//!     },
//!     Engine::Auto,
//! )?;
//! host.handle().close(id);
//! host.run()?;
//! # Ok::<(), anyhow::Error>(())
//! ```

mod host;

#[cfg(feature = "borrowed-browser")]
mod borrowed;
#[cfg(feature = "webview2")]
mod webview2;

use std::path::PathBuf;

use anyhow::Result;
use serde_json::Value;

pub use host::{WebHost, WebHostHandle};

/// Identifies a surface within a [`WebHost`].
pub type SurfaceId = u64;

/// Which engine to use for a surface.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Engine {
    /// WebView2 when available, otherwise a borrowed Chromium.
    Auto,
    /// Force the embedded WebView2 runtime.
    WebView2,
    /// Force a borrowed installed Chromium.
    Borrowed,
}

/// Where a surface's page is actually rendered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SurfaceKind {
    /// A window we own, with the engine embedded in it.
    Embedded,
    /// An external browser process's own window.
    External,
}

/// What a surface can do, so callers degrade instead of assuming parity.
#[derive(Clone, Copy, Debug)]
pub struct Caps {
    /// The page is embedded in a window we own.
    pub embedded: bool,
    /// The host can push events to the page.
    pub can_push: bool,
    /// The surface honours a fixed logical size.
    pub fixed_size: bool,
}

/// Everything host-specific needed to open a surface.
pub struct SurfaceConfig {
    pub url: String,
    pub title: String,
    pub logical_width: i32,
    pub logical_height: i32,
    pub min_width: i32,
    pub min_height: i32,
    pub icon_ico: &'static [u8],
    /// WebView2 user-data folder. A borrowed surface uses `<data_dir>/browser`.
    pub data_dir: PathBuf,
    /// Explicit Chromium executable for the borrowed engine.
    pub browser_override: Option<String>,
    /// Profile display name, shown by the borrowed browser if it draws chrome.
    pub profile_name: String,
    /// Hide the browser's own chrome (tabs, address bar) and render the page in
    /// a minimal app-style window. Honoured by the borrowed engine (which can
    /// also run a normal, chromeful window); the WebView2 engine is always
    /// chromeless because the window is one we own.
    pub chromeless: bool,
}

pub trait Surface {
    fn kind(&self) -> SurfaceKind;
    fn caps(&self) -> Caps;
    fn navigate(&mut self, url: &str) -> Result<()>;

    /// Runs script in the page context and decodes its JSON result.
    ///
    /// The WebView2 implementation pumps the message loop while awaiting the
    /// async result, so it must not be called from inside a host job: those
    /// already run on the UI thread while the host loop is draining. Use
    /// [`Surface::post`] for one-way events instead.
    fn eval(&mut self, js: &str) -> Result<Value>;

    /// Pushes an event to the page's `window.__hostDeliver`.
    fn post(&mut self, name: &str, data: Value) -> Result<()> {
        self.eval(&deliver_js(name, &data)).map(|_| ())
    }

    fn is_alive(&mut self) -> bool;
    fn close(&mut self);

    /// The native window handle for an embedded surface, or `None` for an
    /// external one (whose window belongs to another process). Lets a host
    /// focus or reposition an embedded window.
    fn hwnd(&self) -> Option<isize> {
        None
    }
}

/// The JS that delivers a pushed event to the page shim.
pub(crate) fn deliver_js(name: &str, data: &Value) -> String {
    let name = serde_json::to_string(name).unwrap_or_else(|_| "\"\"".to_string());
    let data = serde_json::to_string(data).unwrap_or_else(|_| "null".to_string());
    format!("window.__hostDeliver && window.__hostDeliver({name}, {data});")
}

/// Creates a surface on the calling thread with the requested engine.
pub fn create(cfg: SurfaceConfig, engine: Engine) -> Result<Box<dyn Surface>> {
    let engine = match engine {
        Engine::Auto => {
            #[cfg(feature = "webview2")]
            {
                if webview2::is_available() {
                    Engine::WebView2
                } else {
                    Engine::Borrowed
                }
            }
            #[cfg(not(feature = "webview2"))]
            {
                Engine::Borrowed
            }
        }
        other => other,
    };

    match engine {
        Engine::WebView2 => {
            #[cfg(feature = "webview2")]
            {
                webview2::WebView2Surface::open(cfg)
            }
            #[cfg(not(feature = "webview2"))]
            {
                Err(anyhow!("this build has no WebView2 engine"))
            }
        }
        Engine::Borrowed => {
            #[cfg(feature = "borrowed-browser")]
            {
                borrowed::BorrowedSurface::open(cfg)
            }
            #[cfg(not(feature = "borrowed-browser"))]
            {
                Err(anyhow!("this build has no borrowed-browser engine"))
            }
        }
        Engine::Auto => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deliver_js_targets_host_deliver_and_json_escapes() {
        let js = deliver_js("evt", &json!({ "a": "x\"y", "n": 1 }));
        assert!(js.starts_with("window.__hostDeliver && window.__hostDeliver("));
        assert!(js.contains("\"evt\""));
        // The payload is JSON-encoded, so a quote cannot break out of the string.
        assert!(js.contains("x\\\"y"), "got: {js}");
    }

    #[test]
    fn deliver_js_passes_null_data_literally() {
        assert!(deliver_js("e", &Value::Null).contains(", null)"));
    }
}
