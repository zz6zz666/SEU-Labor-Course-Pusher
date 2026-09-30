//! Drives an installed Chromium-family browser over CDP.
//!
//! The crate never bundles an engine: the executable is the Edge/Chrome (or any
//! Chromium fork) that is already present, located by [`discovery`]. A session
//! owns its own browser process and a dedicated profile; callers cancel by
//! closing the session.
//!
//! ```no_run
//! use browserhost::{Session, SessionConfig, WindowMode};
//!
//! let mut s = Session::launch(SessionConfig {
//!     visible: true,
//!     profile_dir: String::new(),
//!     url: Some("https://example.com".into()),
//!     mode: WindowMode::App,
//!     exec_path: None,
//!     profile_name: "demo".into(),
//! })?;
//! let _title = s.eval("document.title")?;
//! s.close();
//! # Ok::<(), anyhow::Error>(())
//! ```

mod cdp;
mod cookie;
mod discovery;
mod winproc;

pub use cdp::{Session, SessionConfig, WindowMode};
pub use cookie::Cookie;
pub use discovery::{is_no_browser, NoBrowser};
pub use winproc::kill_for_profile;
