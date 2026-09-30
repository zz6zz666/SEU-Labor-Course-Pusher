//! A resident tray icon with a custom-drawn Fluent-style popup menu.
//!
//! The crate owns the Win32 *mechanism*: the tray icon, the hidden message
//! window, the layered popup with rounded corners, DPI-aware GDI rendering and
//! keyboard/mouse handling. The host supplies the *policy*: the icon bytes, the
//! tooltip text, the menu rows and what each command id does.
//!
//! ```ignore
//! let tray = traykit::Tray::new(traykit::TrayConfig {
//!     icon_ico: ICON,
//!     tooltip: Arc::new(|| "status line".to_string()),
//!     items: Arc::new(|| vec![traykit::MenuItem::command(1, "Open", 0xE774)]),
//!     on_command: Arc::new(|id| { /* dispatch */ }),
//!     on_left_click: Arc::new(|| { /* ... */ }),
//! })?;
//! tray.run();
//! ```

mod menu;
mod tray;
mod winapi;

pub use tray::{
    BoolFn, Callback, CommandFn, ItemKind, ItemsFn, MenuItem, StringFn, Tray, TrayConfig,
};
