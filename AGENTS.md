# AGENTS.md

Guidance for working in this repository: the `seu-labor` desktop app.

## Layout

A single binary crate at the root (`src/`). The reusable Windows / web-UI
building blocks it uses live in a **separate** sibling project,
`../rust-webui-kit` (`winkit`, `traykit`, `browserhost`, `webmsg`,
`websurface`), and are referenced by relative path. This repo keeps only the
school-specific flows (`src/login.rs`, `src/login_scripts.rs`, …) and the app
shell.

If the sibling checkout is missing, clone it next to this repo:

```powershell
git clone https://github.com/zz6zz666/rust-webui-kit ..\rust-webui-kit
```

CI checks this repo and the kit out as siblings inside the runner workspace and
builds from the `app/` directory, so the same relative path resolves there.

## Commands

Prepend the toolchain to `PATH` in a fresh PowerShell session:

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path
```

- Check — must be warning-free:
  ```powershell
  cargo check --all-targets --message-format short
  ```
- Unit tests:
  ```powershell
  cargo test
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

Crate-level work (feature combinations, the push self-test, the Orbit demo)
belongs in `../rust-webui-kit`, which has its own `AGENTS.md`.

## Conventions

- Do not add comments unless they explain *why*.
- The reusable crates use English error messages; this app maps them to Chinese
  user-facing text (see `browser_error` in `src/main.rs`).
- Reusable crates must not contain SEU-specific strings or policy — that code
  lives here, not in `../rust-webui-kit`.
- Keep `cargo check --all-targets` at zero warnings.

## Runtime switches

- `SEU_WIZARD_ENGINE=browser|webview` forces the settings window's engine.
- `SEU_DAEMON_DATA_DIR` / `SEU_DAEMON_CONFIG` override the data dir / config path
  for development.
- `seu-labor -version`, `-once`, `-login`, `-wizard`, `-wizard-ui` are CLI flags.
