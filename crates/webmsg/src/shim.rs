//! The client-side shim injected into the hosted page.
//!
//! It defines `window.<namespace>` as a proxy: any method call becomes a
//! request (so `ns.saveAccount(d)` sends `{name:"saveAccount", data:d}`), plus
//! `.on(name, cb)` for events. `window.__hostDeliver(name, data)` is the entry
//! point a surface calls to push an event.

/// JSON-encodes `s` as a JavaScript string literal.
fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

/// Builds the inline `<script>` tag that installs the client API under
/// `window[namespace]`.
pub(crate) fn script_tag(namespace: &str) -> String {
    let body = r#"(function () {
  var NS = __NS__;
  var subscribers = {};
  function call(name, data) {
    return fetch('/__msg', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        id: (Date.now() * 1000) + Math.floor(Math.random() * 1000),
        kind: 'request',
        name: name,
        data: data === undefined ? null : data
      })
    }).then(function (res) {
      return res.text().then(function (text) {
        var env = null;
        try { env = text ? JSON.parse(text) : null; } catch (e) { env = null; }
        if (env && env.error) { throw new Error(env.error); }
        return env ? env.data : null;
      });
    });
  }
  var api = {
    on: function (name, cb) { (subscribers[name] || (subscribers[name] = [])).push(cb); },
    off: function (name, cb) {
      var list = subscribers[name];
      if (!list) return;
      var i = list.indexOf(cb);
      if (i >= 0) list.splice(i, 1);
    }
  };
  var ns = new Proxy(api, {
    get: function (target, name) {
      if (typeof name !== 'string') return undefined;
      if (Object.prototype.hasOwnProperty.call(target, name)) return target[name];
      return function (arg) { return call(name, arg); };
    }
  });
  window.__hostDeliver = function (name, data) {
    (subscribers[name] || []).forEach(function (cb) {
      try { cb(data); } catch (e) {}
    });
  };
  if (window.chrome && window.chrome.webview) {
    window.chrome.webview.addEventListener('message', function (ev) {
      var m = ev.data;
      if (m && m.__deliver) window.__hostDeliver(m.name, m.data);
    });
  }
  window[NS] = ns;
})();"#;
    format!(
        "<script>\n{}\n</script>",
        body.replace("__NS__", &json_string(namespace))
    )
}

/// Injects the shim and a `document.title` override into `html`, and appends
/// `extra_csp` to a `Content-Security-Policy` meta if one is present (so the
/// page may `connect-src 'self'` for its requests and `img-src` for assets).
pub(crate) fn inject(html: &str, namespace: &str, title: &str, extra_csp: &str) -> String {
    let mut page = html.to_string();

    if !extra_csp.is_empty() {
        const MARK: &str = "Content-Security-Policy\" content=\"";
        if let Some(pos) = page.find(MARK) {
            let start = pos + MARK.len();
            if let Some(end_rel) = page[start..].find('"') {
                page.insert_str(start + end_rel, extra_csp);
            }
        }
    }

    let head = format!(
        "<head>\n<script>document.title = {};</script>\n{}",
        json_string(title),
        script_tag(namespace)
    );
    if page.contains("<head>") {
        page.replacen("<head>", &head, 1)
    } else {
        format!("{}{}", head, page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_sets_title_namespace_and_appends_csp() {
        let html = r#"<html><head><meta http-equiv="Content-Security-Policy" content="default-src 'self'"></head><body></body></html>"#;
        let out = inject(html, "seuWizard", "My Title", "connect-src 'self';");
        assert!(out.contains("document.title = \"My Title\";"), "got: {out}");
        assert!(out.contains("\"seuWizard\""), "namespace is installed");
        assert!(out.contains("window.__hostDeliver"), "push entry point present");
        // CSP is extended in place, before the closing quote of `content`.
        assert!(
            out.contains("default-src 'self'connect-src 'self';"),
            "got: {out}"
        );
    }

    #[test]
    fn inject_without_head_prepends_one() {
        let out = inject("<body>x</body>", "ns", "T", "");
        assert!(out.starts_with("<head>"), "got: {out}");
    }
}
