# AGENTS.md

Guidance for working in this repository: the `seu-labor` course watcher.

## Layout

- `core/` — portable Rust core (`seu-labor-core`): HTTP session/cookie jar, CAS
  login replay, page parsing, selection, config/state, polling state machine and
  PushPlus. No Windows or Android APIs.
- `src/` — Windows desktop shell: tray, WebView2 wizard, Chromium login/course
  windows, WinRT toast, autostart, single-instance.
- `android/` — Android shell: `specialUse` foreground service, notifications,
  WebView login/course pages, settings UI, boot receiver and WorkManager
  watchdog. It calls the core through `core/src/ffi.rs` (JNI).
- `../rust-webui-kit` — reusable Windows/web-UI crates (`winkit`, `traykit`,
  `browserhost`, `webmsg`, `websurface`), referenced by relative path from the
  desktop shell only.

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

Android (JDK 25, Android SDK Platform 36, Build Tools 36.1.0, NDK
27.0.12077973, plus the Rust target and `cargo-ndk`):

```powershell
rustup target add aarch64-linux-android
cargo install cargo-ndk
```

Open `android/` in Android Studio, or build from that directory with the
checked-in Gradle wrapper (Gradle 9.8, AGP 9.4.1, built-in Kotlin):

```powershell
.\gradlew.bat :app:assembleDebug
```

Set `ANDROID_HOME` to the SDK location, or let Android Studio create a local,
uncommitted `android/local.properties` with `sdk.dir`. Do not commit that file.
The Gradle `cargoBuild` task invokes `cargo ndk` with
`--profile android_release --no-default-features --features
rustls,android-ffi` and writes
`android/app/src/main/jniLibs/arm64-v8a/libseu_labor_core.so`.

The JNI layer is type-checked on the desktop with
`cargo check -p seu-labor-core --features android-ffi`; CI runs it too, so keep
it warning-free.

Crate-level work (feature combinations, the push self-test, the Orbit demo)
belongs in `../rust-webui-kit`, which has its own `AGENTS.md`.

## Conventions

- Do not add comments unless they explain *why*.
- The reusable crates use English error messages; this app maps them to Chinese
  user-facing text (see `browser_error` in `src/main.rs`).
- Reusable crates must not contain SEU-specific strings or policy — that code
  lives here, not in `../rust-webui-kit`.
- Keep `cargo check --workspace --all-targets` at zero warnings.
- Keep `core/` platform-neutral: no `windows`, `browserhost`, Android or JNI
  types outside `#[cfg(feature = "android-ffi")]`.

## Runtime switches

- `SEU_WIZARD_ENGINE=browser|webview` forces the settings window's engine.
- `SEU_DAEMON_DATA_DIR` / `SEU_DAEMON_CONFIG` override the data dir / config path
  for development.
- `seu-labor -version`, `-once`, `-login`, `-wizard`, `-wizard-ui` are CLI flags.
