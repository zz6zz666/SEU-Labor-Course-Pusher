//go:build windows

package osutil

import (
	"golang.org/x/sys/windows"
)

// DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 is the pseudo-handle -4.
const dpiAwarenessContextPerMonitorAwareV2 = ^uintptr(3)

var (
	user32                            = windows.NewLazySystemDLL("user32.dll")
	procSetProcessDpiAwarenessContext = user32.NewProc("SetProcessDpiAwarenessContext")
	procSetProcessDPIAware            = user32.NewProc("SetProcessDPIAware")
	procGetDpiForSystem               = user32.NewProc("GetDpiForSystem")

	shcore                     = windows.NewLazySystemDLL("shcore.dll")
	procSetProcessDpiAwareness = shcore.NewProc("SetProcessDpiAwareness")
)

// EnablePerMonitorDPI opts the process into Per-Monitor V2 DPI awareness so
// windows (notably the WebView2 settings window) render crisply on high-DPI
// displays instead of being bitmap-stretched. It must be called before any
// window is created; it is a no-op when awareness is already declared.
func EnablePerMonitorDPI() {
	if procSetProcessDpiAwarenessContext.Find() == nil {
		if r, _, _ := procSetProcessDpiAwarenessContext.Call(dpiAwarenessContextPerMonitorAwareV2); r != 0 {
			return
		}
	}
	// PROCESS_PER_MONITOR_DPI_AWARE = 2 (Windows 8.1+)
	if procSetProcessDpiAwareness.Find() == nil {
		if r, _, _ := procSetProcessDpiAwareness.Call(2); r == 0 {
			return
		}
	}
	if procSetProcessDPIAware.Find() == nil {
		_, _, _ = procSetProcessDPIAware.Call()
	}
}

// DPIScale reports the primary display scale factor (1.0 == 96 DPI).
func DPIScale() float64 {
	if procGetDpiForSystem.Find() == nil {
		if r, _, _ := procGetDpiForSystem.Call(); r >= 96 {
			return float64(r) / 96.0
		}
	}
	return 1.0
}
