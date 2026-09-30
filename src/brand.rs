//! Single source of truth for the product identity shown on screen, so the
//! native settings window (WebView2) and a browser-hosted one cannot drift
//! apart. Whoever draws the window chrome reads its title and size from here.

/// Caption of the settings window, in both hosting modes.
pub const TITLE: &str = "SEU 劳动教育课程推送助手 设置";

/// Logical design size of the settings window, in DIPs.
pub const DESIGN_WIDTH: i32 = 980;
pub const DESIGN_HEIGHT: i32 = 720;

/// Logical minimum size of the settings window.
pub const MIN_WIDTH: i32 = 860;
pub const MIN_HEIGHT: i32 = 640;
