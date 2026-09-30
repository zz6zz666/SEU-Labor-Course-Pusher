# traykit

A resident system-tray icon with a custom-drawn, Fluent-style popup menu.

The crate owns the Win32 **mechanism**: the tray icon, its hidden message window,
the DPI-aware layered popup with rounded corners, and keyboard/mouse handling.
The host supplies the **policy** as plain data: the icon bytes, the tooltip text,
the menu rows, and what each command id does.

- `Tray::new(TrayConfig { .. })` / `run()` / `update()` / `stop()`
- `TrayConfig { icon_ico, tooltip, items, on_command, on_left_click }`
- `MenuItem::{command, toggle, info, separator}`

```rust,no_run
use std::sync::Arc;

let tray = traykit::Tray::new(traykit::TrayConfig {
    icon_ico: &[],
    tooltip: Arc::new(|| "status".to_string()),
    items: Arc::new(|| vec![traykit::MenuItem::command(1, "Open", 0xE774)]),
    on_command: Arc::new(|_id| {}),
    on_left_click: Arc::new(|| {}),
})?;
tray.run();
# Ok::<(), anyhow::Error>(())
```
