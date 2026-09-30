# winkit

Small, dependency-light Win32 helpers shared by the rest of this workspace.

- `enable_per_monitor_dpi()` – opts the process into Per-Monitor V2 DPI
  awareness, so windows are not bitmap-stretched on scaled displays. Idempotent
  and non-overriding; GUI crates call it when they create their first window.
- `dpi_scale()` – the system's DPI scale factor.
- `from_ico(bytes, want)` – builds an `HICON` from raw `.ico` bytes, choosing the
  image closest to `want` px wide (null handle on failure).
- `set_window_icon(hwnd, ico)` – sets a window's big and small icons.

```rust,no_run
winkit::enable_per_monitor_dpi();
let _scale = winkit::dpi_scale();
let _icon = winkit::from_ico(&[], 32);
```
