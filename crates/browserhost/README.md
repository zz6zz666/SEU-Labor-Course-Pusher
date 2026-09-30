# browserhost

Locate, launch and drive an **installed** Chromium-family browser (Edge, Chrome,
Brave, Vivaldi, …) over the Chrome DevTools Protocol. Nothing is bundled: the
executable is discovered from an explicit override, the default-browser
association, well-known install locations and the registry's App Paths.

Each [`Session`] owns its own browser process and a dedicated profile; the caller
closes the session to stop it. Process management is native (Toolhelp + NtQuery)
so no console window ever flashes.

## API

- `SessionConfig` – `visible`, `profile_dir`, `url`, `mode`, `exec_path`, `profile_name`.
- `Session::launch(cfg)` / `navigate` / `eval` / `cookies` / `set_cookies` / `is_alive` / `close`.
- `WindowMode` – `App` (chromeless `--app`), `Browser` (tabs + address bar), `AppFramed { width, height }`.
- `Cookie` – the CDP cookie shape.
- `kill_for_profile(dir)` – reap any lingering browser bound to a profile.
- `is_no_browser(err)` / `NoBrowser` – detect "nothing usable was found" and show your own guidance.

## Example

```rust,no_run
use browserhost::{Session, SessionConfig, WindowMode};

let mut s = Session::launch(SessionConfig {
    visible: true,
    profile_dir: String::new(),
    url: Some("https://example.com".into()),
    mode: WindowMode::App,
    exec_path: None,
    profile_name: "demo".into(),
})?;
let _title = s.eval("document.title")?;
s.close();
# Ok::<(), anyhow::Error>(())
```
