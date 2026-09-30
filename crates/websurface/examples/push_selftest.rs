//! End-to-end check that a host push reaches the hosted page, on either engine.
//!
//! ```text
//! cargo run -p websurface --example push_selftest -- borrowed
//! cargo run -p websurface --example push_selftest -- webview2
//! ```
//!
//! The page subscribes to a `selftest` event and reports what it received back
//! over the bridge; the host broadcasts the event after the page has loaded.
//! Exits 0 when the round trip succeeds.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::json;
use websurface::{Engine, SurfaceConfig, WebHost};

const NO_ICON: &[u8] = &[];

const PAGE: &str = r#"<!doctype html><html><head></head><body>
<script>
window.seuWizard.on('selftest', function (d) {
  window.seuWizard.report({ got: d });
});
</script>
</body></html>"#;

fn main() {
    let engine = match std::env::args().nth(1).as_deref() {
        Some("borrowed") => Engine::Borrowed,
        Some("webview2") => Engine::WebView2,
        _ => {
            eprintln!("usage: push_selftest <borrowed|webview2>");
            std::process::exit(2);
        }
    };

    let got: Arc<Mutex<Option<serde_json::Value>>> = Arc::new(Mutex::new(None));
    let got_cb = got.clone();
    let server = webmsg::serve(
        webmsg::Config {
            namespace: "seuWizard".to_string(),
            title: "push selftest".to_string(),
            index_html: PAGE.to_string(),
            icon_ico: NO_ICON,
            extra_csp: "connect-src 'self';",
        },
        Arc::new(move |name, data| {
            if name == "report" {
                *got_cb.lock().unwrap() = Some(data.clone());
            }
            Ok(json!({ "ok": true }))
        }),
    )
    .expect("start bridge");

    let cfg = SurfaceConfig {
        url: server.url(),
        title: "push selftest".to_string(),
        logical_width: 480,
        logical_height: 360,
        min_width: 320,
        min_height: 240,
        icon_ico: NO_ICON,
        data_dir: std::env::temp_dir().join("websurface-push-selftest"),
        browser_override: None,
        profile_name: "push selftest".to_string(),
        chromeless: true,
        zoom: 1.0,
        zoomable: false,
    };

    let mut host = WebHost::new().expect("create host");
    let id = host.open(cfg, engine).expect("open surface");
    let handle = host.handle();
    thread::spawn(move || {
        // Give the page time to load and subscribe, then push and close.
        thread::sleep(Duration::from_secs(4));
        handle.broadcast("selftest", json!({ "n": 7 }));
        thread::sleep(Duration::from_secs(4));
        handle.close(id);
    });
    host.run().expect("run host");

    let delivered = got
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|v| v.get("got"))
        .and_then(|g| g.get("n"))
        .and_then(|n| n.as_i64());
    if delivered == Some(7) {
        println!("PASS: push delivered via {engine:?}");
    } else {
        eprintln!("FAIL: page did not report the pushed event");
        std::process::exit(1);
    }
}
