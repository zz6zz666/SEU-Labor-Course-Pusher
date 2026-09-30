//! A surface backed by an installed Chromium browser, used as the fallback when
//! no WebView2 runtime is present. The browser's own window *is* the surface,
//! so it is external (out-of-process) and its title/icon come from the page.

use anyhow::Result;
use serde_json::Value;

use browserhost::{Session, SessionConfig, WindowMode};

use crate::{Caps, Surface, SurfaceConfig, SurfaceKind};

pub struct BorrowedSurface {
    session: Session,
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
        let session = Session::launch(SessionConfig {
            visible: true,
            profile_dir: profile,
            url: Some(cfg.url),
            mode,
            exec_path: cfg.browser_override,
            profile_name: cfg.profile_name,
        })?;
        Ok(Box::new(BorrowedSurface { session }))
    }
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
}
