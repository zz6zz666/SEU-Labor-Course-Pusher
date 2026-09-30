# websurface

A multi-window host for web-technology UIs on Windows.

A `Surface` is one window rendering one page. Two engines implement it:

- **WebView2** – the embedded Edge runtime in a Win32 window we own (preferred).
- **Borrowed** – an installed Chromium launched as a chromeless `--app` window,
  used as a fallback when WebView2 is unavailable.

`WebHost` owns the surfaces on a single UI thread, pumps the message loop, and
lets other threads open, push to, or close surfaces through `WebHostHandle`.
Surfaces report their `Caps`, so callers degrade instead of assuming parity.

- `Engine::{Auto, WebView2, Borrowed}`, `SurfaceKind`, `Caps`, `SurfaceId`
- `Surface::{kind, caps, navigate, eval, post, is_alive, close, set_zoom, zoom, hwnd}`
- `SurfaceConfig { url, title, logical_width, logical_height, min_width, min_height, icon_ico, data_dir, browser_override, profile_name, chromeless, zoom, zoomable }`
- `WebHost::{new, open, handle, run}`; `WebHostHandle::{open, post, broadcast, close}`

User zoom is disabled by default (`zoomable: false`); the host can still scale
the page with `Surface::set_zoom` (WebView2 uses the native zoom factor, the
borrowed engine applies a CSS zoom plus an in-page gesture guard).

```rust,no_run
use websurface::{Engine, SurfaceConfig, WebHost};

let mut host = WebHost::new()?;
let id = host.open(
    SurfaceConfig {
        url: "https://example.com".into(),
        title: "Demo".into(),
        logical_width: 800,
        logical_height: 600,
        min_width: 400,
        min_height: 300,
        icon_ico: &[],
        data_dir: std::env::temp_dir(),
        browser_override: None,
        profile_name: "demo".into(),
        chromeless: true,
    },
    Engine::Auto,
)?;
let handle = host.handle();
handle.broadcast("hello", serde_json::json!({ "n": 1 }));
handle.close(id);
host.run()?;
# Ok::<(), anyhow::Error>(())
```
