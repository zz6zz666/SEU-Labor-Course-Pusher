//! Windows desktop notification channel (WinRT toast).

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use tauri_winrt_notification::{IconCrop, Toast as WinToast};

use crate::assets;
use crate::config::Store;
use crate::osutil;

use super::{Channel, Event};

pub struct Toast {
    store: Arc<Store>,
}

impl Toast {
    pub fn new(store: Arc<Store>) -> Toast {
        Toast { store }
    }
}

impl Channel for Toast {
    fn name(&self) -> &'static str {
        "toast"
    }

    fn send(&self, e: &Event) -> Result<()> {
        if !self.store.get().push.windows.enabled {
            return Ok(());
        }
        let icon = write_icon()?;
        let first = WinToast::new(osutil::APP_APP_USER_MODEL_ID)
            .title(&e.title)
            .text1(&e.body)
            .icon(&icon, IconCrop::Square, "")
            .show();
        match first {
            Ok(()) => Ok(()),
            Err(err) => {
                // Fall back to the always-registered PowerShell AUMID so the
                // toast still appears when our own AUMID is not registered.
                WinToast::new(WinToast::POWERSHELL_APP_ID)
                    .title(&e.title)
                    .text1(&e.body)
                    .show()
                    .map_err(|err2| anyhow!("桌面通知失败: {} / {}", err, err2))
            }
        }
    }
}

fn write_icon() -> Result<PathBuf> {
    let path = std::env::temp_dir().join("seu-labor-toast.png");
    std::fs::write(&path, assets::ICON_PNG)?;
    Ok(path)
}
