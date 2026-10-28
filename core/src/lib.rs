//! Platform-neutral core: session, CAS login, parsing, selection, config,
//! state and the polling state machine. Desktop and Android frontends only
//! supply a data directory and a way to consume events.

pub mod cas;
pub mod config;
pub mod fsutil;
pub mod js;
pub mod logging;
pub mod notify;
pub mod selection;
pub mod session;
pub mod site;
pub mod state;
pub mod watcher;

#[cfg(feature = "android-ffi")]
pub mod ffi;
