# webmsg

A loopback HTTP bridge that exposes a Rust host API to a hosted web page.

The page talks to the host through `window.<namespace>.<method>(arg)`, which the
injected shim turns into a `POST /__msg` carrying an [`Envelope`]. The host
answers with a response, and may push events at any time to
`window.__hostDeliver(name, data)` (also wired to the WebView2 message channel).
The reserved message name `close` replies `{ok:true}` and shuts the server down,
so a hosted window can dismiss itself.

The server binds `127.0.0.1` and validates `Host`/`Origin`, blocking DNS-rebinding
requests from remote pages.

- `Config { namespace, title, index_html, icon_ico, extra_csp }`
- `serve(config, handler) -> Arc<Server>`; `Server::{url, addr, close, set_on_close}`
- `Envelope { id, kind, name, data }`, `Kind::{Request, Response, Event}`

```rust,no_run
use std::sync::Arc;
use serde_json::json;

let server = webmsg::serve(
    webmsg::Config {
        namespace: "app".into(),
        title: "App".into(),
        index_html: "<html><head></head><body></body></html>".into(),
        icon_ico: &[],
        extra_csp: "connect-src 'self';",
    },
    Arc::new(|name, data| Ok(json!({ "echo": name, "got": data }))),
)?;
println!("page at {}", server.url());
# Ok::<(), anyhow::Error>(())
```
