//! A loopback HTTP bridge that exposes a host API to a web page rendered by any
//! surface (WebView2 or a borrowed browser). The page calls
//! `window.<namespace>.<method>(arg)`, which becomes a message; the host may
//! also push events via `window.__hostDeliver(name, data)`.
//!
//! ```no_run
//! use std::sync::Arc;
//! use serde_json::json;
//!
//! let server = webmsg::serve(
//!     webmsg::Config {
//!         namespace: "app".into(),
//!         title: "App".into(),
//!         index_html: "<html><head></head><body></body></html>".into(),
//!         icon_ico: &[],
//!         extra_csp: "connect-src 'self';",
//!     },
//!     Arc::new(|name, data| Ok(json!({ "echo": name, "got": data }))),
//! )?;
//! println!("page at {}", server.url());
//! # Ok::<(), anyhow::Error>(())
//! ```

mod envelope;
mod server;
mod shim;

pub use envelope::{Envelope, Kind};
pub use server::{serve, Config, Handler, Server};
