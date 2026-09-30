//! The UI-thread host: owns the surfaces, pumps the message loop, and accepts
//! jobs (open/post/close) from other threads via a queue + a thread message.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use serde_json::Value;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MsgWaitForMultipleObjects, PeekMessageW, PostThreadMessageW, TranslateMessage,
    MSG, PM_NOREMOVE, PM_REMOVE, QS_ALLINPUT, WM_APP, WM_QUIT,
};

use crate::{create, Engine, Surface, SurfaceConfig, SurfaceId, SurfaceKind};

/// Arbitrary private message used only to wake the host's message loop.
const WAKE: u32 = WM_APP + 0x2F;

type Surfaces = HashMap<SurfaceId, Box<dyn Surface>>;
type Job = Box<dyn FnOnce(&mut Surfaces) + Send>;

struct Inner {
    thread_id: u32,
    queue: Mutex<Vec<Job>>,
    next_id: AtomicU64,
}

/// A thread-safe handle for opening, driving and closing surfaces from any
/// thread.
#[derive(Clone)]
pub struct WebHostHandle(Arc<Inner>);

/// The UI-thread owner of the surfaces. Create it, [`WebHost::open`] surfaces,
/// then [`WebHost::run`] until they all close.
pub struct WebHost {
    inner: Arc<Inner>,
    surfaces: Surfaces,
}

impl WebHost {
    /// Must be called on the thread that will run the loop and own the windows.
    pub fn new() -> Result<WebHost> {
        let thread_id = unsafe { GetCurrentThreadId() };
        // Force this thread's message queue into existence, so a later
        // `PostThreadMessageW` from another thread cannot race it.
        let mut msg = MSG::default();
        unsafe { let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE); };
        Ok(WebHost {
            inner: Arc::new(Inner {
                thread_id,
                queue: Mutex::new(Vec::new()),
                next_id: AtomicU64::new(1),
            }),
            surfaces: HashMap::new(),
        })
    }

    /// A cloneable handle for other threads to open, push to or close surfaces.
    pub fn handle(&self) -> WebHostHandle {
        WebHostHandle(self.inner.clone())
    }

    /// Opens a surface on the UI thread. Call before [`WebHost::run`].
    pub fn open(&mut self, cfg: SurfaceConfig, engine: Engine) -> Result<SurfaceId> {
        let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
        let surface = create(cfg, engine)?;
        self.surfaces.insert(id, surface);
        Ok(id)
    }

    /// Pumps messages until every surface has closed.
    pub fn run(&mut self) -> Result<()> {
        loop {
            self.drain();
            self.dispatch_pending();
            self.surfaces.retain(|_, s| s.is_alive());
            if self.surfaces.is_empty() {
                return Ok(());
            }
            // Block until a message (or a wake) arrives. With an external
            // surface present, time out periodically so its liveness is polled.
            let all_embedded = self
                .surfaces
                .values()
                .all(|s| s.kind() == SurfaceKind::Embedded);
            let timeout = if all_embedded { u32::MAX } else { 200 };
            unsafe { MsgWaitForMultipleObjects(None, false, timeout, QS_ALLINPUT) };
        }
    }

    fn dispatch_pending(&mut self) {
        let mut msg = MSG::default();
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            if msg.message == WM_QUIT {
                self.surfaces.clear();
                return;
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    fn drain(&mut self) {
        let jobs: Vec<Job> = {
            let mut q = self.inner.queue.lock().unwrap();
            std::mem::take(&mut *q)
        };
        for job in jobs {
            job(&mut self.surfaces);
        }
    }
}

impl WebHostHandle {
    /// Opens a surface from any thread, returning its id once created.
    ///
    /// Creation happens on the UI thread (it may need COM), so this blocks until
    /// the host loop services the request: it must be called *after*
    /// [`WebHost::run`] has started, or it will deadlock.
    pub fn open(&self, cfg: SurfaceConfig, engine: Engine) -> Result<SurfaceId> {
        let id = self.0.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        self.push(Box::new(move |surfaces| {
            let result = create(cfg, engine).map(|surface| {
                surfaces.insert(id, surface);
                id
            });
            let _ = tx.send(result);
        }));
        rx.recv()
            .map_err(|_| anyhow!("host stopped before the surface could be opened"))?
    }

    /// Pushes an event to one surface (delivered to `window.__hostDeliver`).
    pub fn post(&self, id: SurfaceId, name: &str, data: Value) {
        let name = name.to_string();
        self.push(Box::new(move |surfaces| {
            if let Some(s) = surfaces.get_mut(&id) {
                if s.caps().can_push {
                    let _ = s.post(&name, data);
                }
            }
        }));
    }

    /// Pushes an event to every surface that can receive one.
    pub fn broadcast(&self, name: &str, data: Value) {
        let name = name.to_string();
        self.push(Box::new(move |surfaces| {
            for s in surfaces.values_mut() {
                if s.caps().can_push {
                    let _ = s.post(&name, data.clone());
                }
            }
        }));
    }

    /// Closes one surface.
    pub fn close(&self, id: SurfaceId) {
        self.push(Box::new(move |surfaces| {
            if let Some(s) = surfaces.get_mut(&id) {
                s.close();
            }
        }));
    }

    fn push(&self, job: Job) {
        self.0.queue.lock().unwrap().push(job);
        unsafe {
            let _ = PostThreadMessageW(self.0.thread_id, WAKE, WPARAM(0), LPARAM(0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Caps, SurfaceKind};
    use std::sync::atomic::AtomicBool;

    /// A surface that records what the host routes to it, with no real window.
    struct FakeSurface {
        can_push: bool,
        posted: Arc<Mutex<Vec<(String, Value)>>>,
        closed: Arc<AtomicBool>,
    }

    impl Surface for FakeSurface {
        fn kind(&self) -> SurfaceKind {
            SurfaceKind::External
        }
        fn caps(&self) -> Caps {
            Caps {
                embedded: false,
                can_push: self.can_push,
                fixed_size: false,
            }
        }
        fn navigate(&mut self, _url: &str) -> Result<()> {
            Ok(())
        }
        fn eval(&mut self, _js: &str) -> Result<Value> {
            Ok(Value::Null)
        }
        fn post(&mut self, name: &str, data: Value) -> Result<()> {
            self.posted.lock().unwrap().push((name.to_string(), data));
            Ok(())
        }
        fn is_alive(&mut self) -> bool {
            true
        }
        fn close(&mut self) {
            self.closed.store(true, Ordering::SeqCst);
        }
    }

    fn fake(can_push: bool) -> (Box<dyn Surface>, Arc<Mutex<Vec<(String, Value)>>>, Arc<AtomicBool>) {
        let posted = Arc::new(Mutex::new(Vec::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let surface = Box::new(FakeSurface {
            can_push,
            posted: posted.clone(),
            closed: closed.clone(),
        });
        (surface, posted, closed)
    }

    #[test]
    fn handle_routes_post_broadcast_and_close_to_surfaces() {
        let mut host = WebHost::new().unwrap();
        let (s1, posted1, closed1) = fake(true);
        let (s2, posted2, _closed2) = fake(false);
        host.surfaces.insert(1, s1);
        host.surfaces.insert(2, s2);

        let h = host.handle();
        h.post(1, "ping", serde_json::json!(42));
        h.broadcast("all", serde_json::json!("x"));
        h.close(1);

        // Jobs are queued, not run, until the UI thread drains them.
        assert!(posted1.lock().unwrap().is_empty());
        host.drain();

        let got = posted1.lock().unwrap().clone();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], ("ping".to_string(), serde_json::json!(42)));
        assert_eq!(got[1], ("all".to_string(), serde_json::json!("x")));
        assert!(closed1.load(Ordering::SeqCst));

        // caps().can_push == false means the host must not push to it.
        assert!(posted2.lock().unwrap().is_empty());
    }
}
