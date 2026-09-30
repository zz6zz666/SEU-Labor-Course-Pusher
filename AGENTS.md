# AGENTS.md

Guidance for working in this repository.

## Layout

A Cargo workspace: the `seu-labor` executable at the root, plus reusable crates
under `crates/` (`winkit`, `traykit`, `browserhost`, `webmsg`, `websurface`).
Dependency direction: `seu-labor` → `websurface` → { `webmsg`, `browserhost`,
`winkit` }; `traykit` → `winkit`.

## Commands

Prepend the toolchain to `PATH` in a fresh PowerShell session:

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path
```

- Check — must be warning-free:
  ```powershell
  cargo check --workspace --all-targets --message-format short
  ```
- Unit tests:
  ```powershell
  cargo test --workspace
  ```
- Release build (also copies the exe to `release\`):
  ```powershell
  .\build.ps1 -Version 1.2.0
  ```
- Installer (needs Inno Setup 6):
  ```powershell
  .\package.ps1 -Version 1.2.0
  ```
- Smoke test (launches the real app/tray/windows):
  ```powershell
  .\test\smoke.ps1
  ```
- Feature-combination checks:
  ```powershell
  cargo check -p websurface --no-default-features
  cargo check -p websurface --no-default-features --features webview2
  cargo check -p websurface --no-default-features --features borrowed-browser
  ```

## Conventions

- Do not add comments unless they explain *why*.
- Reusable crates use English error messages; the `seu-labor` app maps them to
  Chinese user-facing text (see `browser_error` in `src/main.rs`).
- Reusable crates must not contain SEU-specific strings or policy; site flows
  (`src/login.rs`, `src/login_scripts.rs`) stay in the application.
- Keep `cargo check --workspace --all-targets` at zero warnings.

## Runtime switches

- `SEU_WIZARD_ENGINE=browser|webview` forces the settings window's engine.
- `SEU_DAEMON_DATA_DIR` / `SEU_DAEMON_CONFIG` override the data dir / config path
  for development.
- `seu-labor -version`, `-once`, `-login`, `-wizard`, `-wizard-ui` are CLI flags.
