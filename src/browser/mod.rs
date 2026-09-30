//! Defines the seam for the only operations that need a real JS engine:
//! interactive login (captcha) and viewing the course page. Polling, selection
//! and cancellation are plain HTTP and never use it.
//!
//! The concrete engine is an installed Chromium browser (Edge, Chrome or
//! another Chromium build) driven over CDP. The browser
//! owns its own process; callers cancel by closing it.

pub mod cdp;
pub mod discovery;
pub mod scripts;
pub mod winproc;

use anyhow::Result;

use crate::session::jar::StoredCookie;

pub trait Browser: Send {
    /// Runs script in the page context and decodes its JSON result.
    fn eval(&mut self, script: &str) -> Result<serde_json::Value>;
    /// Returns every cookie in the profile, including session cookies.
    fn all_cookies(&mut self) -> Result<Vec<StoredCookie>>;
    /// Injects cookies into the profile. Required before navigating: session
    /// cookies are not retained in the on-disk profile.
    fn set_cookies(&mut self, cookies: &[StoredCookie]) -> Result<()>;
    fn close(&mut self);
}
