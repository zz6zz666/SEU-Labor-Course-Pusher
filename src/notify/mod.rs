//! Notification channel fan-out.

pub mod event;
pub mod pushplus;
pub mod toast;

use anyhow::Result;

use crate::logging::Logger;
pub use event::Event;

pub trait Channel: Send + Sync {
    fn name(&self) -> &'static str;
    fn send(&self, e: &Event) -> Result<()>;
}

/// Fans an event out to every channel. A failing channel never blocks the
/// others.
pub struct Dispatcher {
    channels: Vec<Box<dyn Channel>>,
    log: Logger,
}

impl Dispatcher {
    pub fn new(log: Logger) -> Dispatcher {
        Dispatcher {
            channels: Vec::new(),
            log,
        }
    }

    pub fn add(&mut self, c: Box<dyn Channel>) {
        self.channels.push(c);
    }

    pub fn dispatch(&self, e: &Event) {
        for c in &self.channels {
            if let Err(err) = c.send(e) {
                self.log.warn(format!("通知通道失败 {} {}", c.name(), err));
            }
        }
    }
}
