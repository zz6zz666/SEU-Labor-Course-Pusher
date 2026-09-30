//! A surface backed by an installed Chromium browser, used as the fallback when
//! no WebView2 runtime is present. The browser's own window *is* the surface,
//! so it is external (out-of-process) and its title/icon come from the page.

use anyhow::Result;
use serde_json::Value;

use browserhost::{Session, SessionConfig, WindowMode};

use crate::{Caps, Surface, SurfaceConfig, SurfaceKind};

pub struct BorrowedSurface {
    session: Session,
    zoom: f64,
}

impl BorrowedSurface {
    pub fn open(cfg: SurfaceConfig) -> Result<Box<dyn Surface>> {
        let profile = cfg.data_dir.join("browser").to_string_lossy().into_owned();
        // Chromeless renders as an `--app` window; otherwise a normal window
        // with tabs and an address bar (which also drops the fixed size).
        let mode = if cfg.chromeless {
            WindowMode::AppFramed {
                width: cfg.logical_width,
                height: cfg.logical_height,
            }
        } else {
            WindowMode::Browser
        };
        let zoom = if cfg.zoom > 0.0 { cfg.zoom } else { 1.0 };
        let mut session = Session::launch(SessionConfig {
            visible: true,
            profile_dir: profile,
            url: Some(cfg.url),
            mode,
            exec_path: cfg.browser_override,
            profile_name: cfg.profile_name,
        })?;
        // The borrowed browser has no switch to disable user zoom, so install a
        // small in-page guard and apply the initial zoom there. The initial
        // navigation is still in flight when the target appears, so wait for the
        // real document to commit before evaluating (otherwise the effect is
        // lost with the provisional document).
        let ready = "(document.readyState === 'complete' && !!document.documentElement && location.href.indexOf('about:blank') !== 0) ? 'yes' : 'no'";
        for _ in 0..100 {
            match session.eval(ready) {
                Ok(v) if v.as_str() == Some("yes") => break,
                _ => std::thread::sleep(std::time::Duration::from_millis(50)),
            }
        }
        let _ = session.eval(&page_zoom_script(zoom, cfg.zoomable));
        Ok(Box::new(BorrowedSurface { session, zoom }))
    }
}

/// Applies `zoom` in the page and, unless `zoomable`, blocks the user's zoom
/// gestures (Ctrl+wheel, Ctrl +/-/0, touch pinch).
fn page_zoom_script(zoom: f64, zoomable: bool) -> String {
    let guard = if zoomable {
        ""
    } else {
        "\n  document.addEventListener('wheel', function (e) { if (e.ctrlKey) e.preventDefault(); }, { passive: false });\n  document.addEventListener('keydown', function (e) { if (e.ctrlKey && (e.key === '+' || e.key === '-' || e.key === '=' || e.key === '0')) e.preventDefault(); });\n  if (document.documentElement) document.documentElement.style.touchAction = 'pan-x pan-y';"
    };
    format!(
        "(function () {{\n  var z = {zoom};\n  function apply() {{ if (document.documentElement) document.documentElement.style.zoom = String(z); }}\n  apply();\n  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', apply);{guard}\n}})();"
    )
}

impl Surface for BorrowedSurface {
    fn kind(&self) -> SurfaceKind {
        SurfaceKind::External
    }

    fn caps(&self) -> Caps {
        Caps {
            embedded: false,
            can_push: true,
            fixed_size: true,
            can_zoom: true,
        }
    }

    fn navigate(&mut self, url: &str) -> Result<()> {
        self.session.navigate(url)
    }

    fn eval(&mut self, js: &str) -> Result<Value> {
        self.session.eval(js)
    }

    fn is_alive(&mut self) -> bool {
        self.session.is_alive()
    }

    fn close(&mut self) {
        self.session.close();
    }

    fn set_zoom(&mut self, factor: f64) -> Result<()> {
        let factor = if factor > 0.0 { factor } else { 1.0 };
        self.session.eval(&format!(
            "document.documentElement && (document.documentElement.style.zoom = '{}')",
            factor
        ))?;
        self.zoom = factor;
        Ok(())
    }

    fn zoom(&self) -> f64 {
        self.zoom
    }
}
