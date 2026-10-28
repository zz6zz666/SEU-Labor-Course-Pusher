//! Channel that records events for a frontend to drain. Android uses this to
//! turn core events into local notifications without a Rust→Kotlin callback.

use std::sync::{Arc, Mutex};

use anyhow::Result;

use super::{Channel, Event};

#[derive(Clone)]
pub struct CollectingChannel {
    events: Arc<Mutex<Vec<Event>>>,
}

impl CollectingChannel {
    pub fn new() -> CollectingChannel {
        CollectingChannel {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn drain(&self) -> Vec<Event> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
}

impl Default for CollectingChannel {
    fn default() -> Self {
        Self::new()
    }
}

impl Channel for CollectingChannel {
    fn name(&self) -> &'static str {
        "collect"
    }

    fn send(&self, e: &Event) -> Result<()> {
        self.events.lock().unwrap().push(e.clone());
        Ok(())
    }
}
