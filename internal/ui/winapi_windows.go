//go:build windows

package ui

import (
	"unsafe"

	"golang.org/x/sys/windows"
)

var (
	user32   = windows.NewLazySystemDLL("user32.dll")
	gdi32    = windows.NewLazySystemDLL("gdi32.dll")
	shell32  = windows.NewLazySystemDLL("shell32.dll")
	dwmapi   = windows.NewLazySystemDLL("dwmapi.dll")
	kernel32 = windows.NewLazySystemDLL("kernel32.dll")

	pRegisterClassExW              = user32.NewProc("RegisterClassExW")
	pCreateWindowExW               = user32.NewProc("CreateWindowExW")
	pDefWindowProcW                = user32.NewProc("DefWindowProcW")
	pGetMessageW                   = user32.NewProc("GetMessageW")
	pTranslateMessage              = user32.NewProc("TranslateMessage")
	pDispatchMessageW              = user32.NewProc("DispatchMessageW")
	pPostQuitMessage               = user32.NewProc("PostQuitMessage")
	pPostMessageW                  = user32.NewProc("PostMessageW")
	pShowWindow                    = user32.NewProc("ShowWindow")
	pSetForegroundWindow           = user32.NewProc("SetForegroundWindow")
	pGetCursorPos                  = user32.NewProc("GetCursorPos")
	pGetClientRect                 = user32.NewProc("GetClientRect")
	pInvalidateRect                = user32.NewProc("InvalidateRect")
	pBeginPaint                    = user32.NewProc("BeginPaint")
	pEndPaint                      = user32.NewProc("EndPaint")
	pSetWindowPos                  = user32.NewProc("SetWindowPos")
	pDestroyWindow                 = user32.NewProc("DestroyWindow")
	pLoadImageW                    = user32.NewProc("LoadImageW")
	pCreateIconFromResourceEx      = user32.NewProc("CreateIconFromResourceEx")
	pGetSystemMetrics              = user32.NewProc("GetSystemMetrics")
	pLoadCursorW                   = user32.NewProc("LoadCursorW")
	pSetCursor                     = user32.NewProc("SetCursor")
	pGetDC                         = user32.NewProc("GetDC")
	pReleaseDC                     = user32.NewProc("ReleaseDC")
	pSetWindowRgn                  = user32.NewProc("SetWindowRgn")
	pTrackMouseEvent               = user32.NewProc("TrackMouseEvent")
	pGetDpiForWindow               = user32.NewProc("GetDpiForWindow")
	pSetWindowCompositionAttribute = user32.NewProc("SetWindowCompositionAttribute")
	pMonitorFromPoint              = user32.NewProc("MonitorFromPoint")
	pGetMonitorInfoW               = user32.NewProc("GetMonitorInfoW")

	pShellNotifyIconW           = shell32.NewProc("Shell_NotifyIconW")
	pRegisterWindowMessageW     = user32.NewProc("RegisterWindowMessageW")
	pSetLayeredWindowAttributes = user32.NewProc("SetLayeredWindowAttributes")

	pGetModuleHandleW = kernel32.NewProc("GetModuleHandleW")
	pGetTickCount     = kernel32.NewProc("GetTickCount")

	pDwmSetWindowAttribute = dwmapi.NewProc("DwmSetWindowAttribute")

	pCreateCompatibleDC     = gdi32.NewProc("CreateCompatibleDC")
	pCreateCompatibleBitmap = gdi32.NewProc("CreateCompatibleBitmap")
	pSelectObject           = gdi32.NewProc("SelectObject")
	pDeleteObject           = gdi32.NewProc("DeleteObject")
	pDeleteDC               = gdi32.NewProc("DeleteDC")
	pCreateSolidBrush       = gdi32.NewProc("CreateSolidBrush")
	pCreatePen              = gdi32.NewProc("CreatePen")
	pRoundRect              = gdi32.NewProc("RoundRect")
	pBitBlt                 = gdi32.NewProc("BitBlt")
	pSetBkMode              = gdi32.NewProc("SetBkMode")
	pSetTextColor           = gdi32.NewProc("SetTextColor")
	pCreateFontW            = gdi32.NewProc("CreateFontW")
	pGetTextExtentPoint32W  = gdi32.NewProc("GetTextExtentPoint32W")

	// FillRect and DrawText live in user32, not gdi32.
	pFillRect  = user32.NewProc("FillRect")
	pDrawTextW = user32.NewProc("DrawTextW")
)

const (
	wsPopup        = 0x80000000
	wsExToolWindow = 0x00000080
	wsExTopmost    = 0x00000008
	wsExLayered    = 0x00080000
	wsExNoActivate = 0x08000000
	csDropShadow   = 0x00020000
	lwaAlpha       = 0x00000002

	swHide           = 0
	swShowNoActivate = 4

	swpNoSize     = 0x0001
	swpNoMove     = 0x0002
	swpNoZOrder   = 0x0004
	swpNoActivate = 0x0010
	swpShowWindow = 0x0040

	wmDestroy     = 0x0002
	wmClose       = 0x0010
	wmPaint       = 0x000F
	wmEraseBkgnd  = 0x0014
	wmActivate    = 0x0006
	wmKillFocus   = 0x0008
	wmKeyDown     = 0x0100
	wmMouseMove   = 0x0200
	wmLButtonUp   = 0x0202
	wmRButtonUp   = 0x0205
	wmMouseLeave  = 0x02A3
	wmSetCursor   = 0x0020
	wmContextMenu = 0x007B
	wmApp         = 0x8000
	waInactive    = 0

	htClient = 1
	idcArrow = 32512

	smCXSmIcon = 49
	smCYSmIcon = 50

	imageIcon     = 1
	lrDefaultSize = 0x0040
	lrShared      = 0x8000

	nimAdd    = 0x00000000
	nimModify = 0x00000001
	nimDelete = 0x00000002

	nifMessage = 0x00000001
	nifIcon    = 0x00000002
	nifTip     = 0x00000004

	transparent  = 1
	dtLeft       = 0x00000000
	dtCenter     = 0x00000001
	dtRight      = 0x00000002
	dtVCenter    = 0x00000004
	dtSingleLine = 0x00000020
	dtNoPrefix   = 0x00000800
	srcCopy      = 0x00CC0020
	psSolid      = 0
	fwNormal     = 400
	fwSemiBold   = 600

	tmeLeave = 0x00000002

	dwmwaWindowCornerPreference = 33
	dwmvCornerRoundSmall        = 3
	dwmwaBorderColor            = 34

	wcaAccentPolicy         = 19
	accentEnableAcrylicBlur = 4

	monitorDefaultToNearest = 2
)

type point struct{ X, Y int32 }

type rect struct{ Left, Top, Right, Bottom int32 }

type msg struct {
	Hwnd    windows.Handle
	Message uint32
	WParam  uintptr
	LParam  uintptr
	Time    uint32
	Pt      point
}

type wndClassExW struct {
	CbSize        uint32
	Style         uint32
	LpfnWndProc   uintptr
	CbClsExtra    int32
	CbWndExtra    int32
	HInstance     windows.Handle
	HIcon         windows.Handle
	HCursor       windows.Handle
	HbrBackground windows.Handle
	LpszMenuName  *uint16
	LpszClassName *uint16
	HIconSm       windows.Handle
}

type notifyIconData struct {
	CbSize           uint32
	HWnd             windows.Handle
	UID              uint32
	UFlags           uint32
	UCallbackMessage uint32
	HIcon            windows.Handle
	SzTip            [128]uint16
	DwState          uint32
	DwStateMask      uint32
	SzInfo           [256]uint16
	UVersion         uint32
	SzInfoTitle      [64]uint16
	DwInfoFlags      uint32
	GuidItem         windows.GUID
	HBalloonIcon     windows.Handle
}

type monitorInfo struct {
	CbSize    uint32
	RcMonitor rect
	RcWork    rect
	DwFlags   uint32
}

type trackMouseEvent struct {
	CbSize      uint32
	DwFlags     uint32
	HwndTrack   windows.Handle
	DwHoverTime uint32
}

type accentPolicy struct {
	AccentState   uint32
	AccentFlags   uint32
	GradientColor uint32
	AnimationID   uint32
}

type winCompAttrData struct {
	Attribute  uint32
	Data       unsafe.Pointer
	SizeOfData uintptr
}

// clampToWorkArea keeps a w x h popup inside the monitor's work area.
func clampToWorkArea(x, y, w, h int, pt point) (int, int) {
	packed := uintptr(uint32(pt.X)) | uintptr(uint32(pt.Y))<<32
	mon, _, _ := pMonitorFromPoint.Call(packed, monitorDefaultToNearest)
	if mon == 0 {
		return x, y
	}
	var mi monitorInfo
	mi.CbSize = uint32(unsafe.Sizeof(monitorInfo{}))
	if r, _, _ := pGetMonitorInfoW.Call(mon, uintptr(unsafe.Pointer(&mi))); r == 0 {
		return x, y
	}
	wa := mi.RcWork
	if x+w > int(wa.Right) {
		x = int(wa.Right) - w
	}
	if y+h > int(wa.Bottom) {
		y = int(wa.Bottom) - h
	}
	if x < int(wa.Left) {
		x = int(wa.Left)
	}
	if y < int(wa.Top) {
		y = int(wa.Top)
	}
	return x, y
}

func getModuleHandle() windows.Handle {
	h, _, _ := pGetModuleHandleW.Call(0)
	return windows.Handle(h)
}

func utf16(s string) *uint16 {
	p, _ := windows.UTF16PtrFromString(s)
	return p
}

func setWindowCompositionAcrylic(hwnd windows.Handle, argb uint32) bool {
	policy := accentPolicy{
		AccentState:   accentEnableAcrylicBlur,
		GradientColor: argb,
	}
	data := winCompAttrData{
		Attribute:  wcaAccentPolicy,
		Data:       unsafe.Pointer(&policy),
		SizeOfData: unsafe.Sizeof(policy),
	}
	r, _, _ := pSetWindowCompositionAttribute.Call(uintptr(hwnd), uintptr(unsafe.Pointer(&data)))
	return r != 0
}

// arrowCursor is the shared IDC_ARROW system cursor.
var arrowCursor windows.Handle

func loadArrowCursor() windows.Handle {
	if arrowCursor == 0 {
		h, _, _ := pLoadCursorW.Call(0, idcArrow)
		arrowCursor = windows.Handle(h)
	}
	return arrowCursor
}

func setArrowCursor() {
	_, _, _ = pSetCursor.Call(uintptr(loadArrowCursor()))
}

func setRoundedCorners(hwnd windows.Handle) {
	v := int32(dwmvCornerRoundSmall)
	_, _, _ = pDwmSetWindowAttribute.Call(uintptr(hwnd), dwmwaWindowCornerPreference, uintptr(unsafe.Pointer(&v)), unsafe.Sizeof(v))
}

// setLayeredAlpha makes an (already WS_EX_LAYERED) window uniformly translucent.
func setLayeredAlpha(hwnd windows.Handle, alpha byte) {
	_, _, _ = pSetLayeredWindowAttributes.Call(uintptr(hwnd), 0, uintptr(alpha), lwaAlpha)
}

// setBorderColor sets the DWM 1px window border color (COLORREF 0x00BBGGRR).
func setBorderColor(hwnd windows.Handle, color uint32) {
	c := color
	_, _, _ = pDwmSetWindowAttribute.Call(uintptr(hwnd), dwmwaBorderColor, uintptr(unsafe.Pointer(&c)), unsafe.Sizeof(c))
}

func dpiForWindow(hwnd windows.Handle) int {
	r, _, _ := pGetDpiForWindow.Call(uintptr(hwnd))
	if r < 96 {
		return 96
	}
	return int(r)
}

func scaleFor(dpi int) float64 {
	if dpi <= 0 {
		return 1
	}
	return float64(dpi) / 96.0
}

// createIconFromICO builds an HICON from raw .ico bytes, choosing the image
// closest to the requested size. Returns 0 on failure.
func createIconFromICO(ico []byte, want int) windows.Handle {
	if len(ico) < 6 || ico[0] != 0 || ico[1] != 0 || ico[2] != 1 || ico[3] != 0 {
		return 0
	}
	count := int(uint16(ico[4]) | uint16(ico[5])<<8)
	bestOff, bestLen, bestScore := 0, 0, 1<<30
	for i := 0; i < count; i++ {
		base := 6 + i*16
		if base+16 > len(ico) {
			break
		}
		w := int(ico[base])
		if w == 0 {
			w = 256
		}
		off := int(uint32(ico[base+12]) | uint32(ico[base+13])<<8 | uint32(ico[base+14])<<16 | uint32(ico[base+15])<<24)
		ln := int(uint32(ico[base+8]) | uint32(ico[base+9])<<8 | uint32(ico[base+10])<<16 | uint32(ico[base+11])<<24)
		if off < 0 || ln <= 0 || off+ln > len(ico) {
			continue
		}
		score := w - want
		if score < 0 {
			score = -score
		}
		if score < bestScore {
			bestScore, bestOff, bestLen = score, off, ln
		}
	}
	if bestLen == 0 {
		return 0
	}
	h, _, _ := pCreateIconFromResourceEx.Call(
		uintptr(unsafe.Pointer(&ico[bestOff])),
		uintptr(bestLen),
		1, // fIcon
		0x00030000,
		0, 0, // default size
		0, // LR_DEFAULTCOLOR
	)
	return windows.Handle(h)
}
