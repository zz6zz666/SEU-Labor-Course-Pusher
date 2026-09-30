# winkit

Small, dependency-light Win32 helpers shared by the rest of this workspace.

- `dpi_scale()` – the system's DPI scale factor.
- `from_ico(bytes, want)` – builds an `HICON` from raw `.ico` bytes, choosing the
  image closest to `want` px wide (null handle on failure).
- `set_window_icon(hwnd, ico)` – sets a window's big and small icons.

```rust,no_run
let _scale = winkit::dpi_scale();
let _icon = winkit::from_ico(&[], 32);
```
