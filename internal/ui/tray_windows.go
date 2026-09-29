//go:build windows

package ui

import (
	"errors"
	"runtime"
	"unsafe"

	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/assets"
	"golang.org/x/sys/windows"
)

const iconResourceID = 32512

var (
	errTrayWindow = errors.New("创建托盘消息窗口失败")
	currentTray   *Tray
)

// Tray is the notification-area icon and its Fluent popup menu.
type Tray struct {
	actions Actions

	hwnd      windows.Handle
	icon      windows.Handle
	callback  uint32
	taskbarID uint32
	added     bool
	menu      *menu
	stopped   bool
}

// NewTray prepares the tray icon and menu. It must be called on the goroutine
// that will later call Run (the message loop thread).
func NewTray(actions Actions, _ string) (*Tray, error) {
	runtime.LockOSThread()

	t := &Tray{actions: actions}
	hInst := getModuleHandle()

	cx, _, _ := pGetSystemMetrics.Call(smCXSmIcon)
	cy, _, _ := pGetSystemMetrics.Call(smCYSmIcon)
	t.icon = createIconFromICO(assets.IconICO, int(cx))
	if t.icon == 0 {
		h, _, _ := pLoadImageW.Call(uintptr(hInst), uintptr(iconResourceID), imageIcon, cx, cy, 0)
		t.icon = windows.Handle(h)
	}

	t.callback = wmApp + 1
	tid, _, _ := pRegisterWindowMessageW.Call(uintptr(unsafe.Pointer(utf16("TaskbarCreated"))))
	t.taskbarID = uint32(tid)

	cls := utf16("seuLaborTrayMsg")
	wc := wndClassExW{
		CbSize:        uint32(unsafe.Sizeof(wndClassExW{})),
		HInstance:     hInst,
		HCursor:       loadArrowCursor(),
		LpfnWndProc:   windows.NewCallback(trayWndProc),
		LpszClassName: cls,
	}
	if atom, _, err := pRegisterClassExW.Call(uintptr(unsafe.Pointer(&wc))); atom == 0 {
		runtime.UnlockOSThread()
		return nil, errors.Join(errTrayWindow, err)
	}

	hwnd, _, err := pCreateWindowExW.Call(
		0,
		uintptr(unsafe.Pointer(cls)),
		uintptr(unsafe.Pointer(utf16("SEU 劳动教育课程监控"))),
		0, 0, 0, 0, 0,
		0, 0,
		uintptr(hInst),
		0,
	)
	if hwnd == 0 {
		runtime.UnlockOSThread()
		return nil, errors.Join(errTrayWindow, err)
	}
	t.hwnd = windows.Handle(hwnd)

	m, err := newMenu(t)
	if err != nil {
		_, _, _ = pDestroyWindow.Call(hwnd)
		runtime.UnlockOSThread()
		return nil, err
	}
	t.menu = m

	currentTray = t
	if err := t.addIcon(); err != nil {
		_, _, _ = pDestroyWindow.Call(hwnd)
		runtime.UnlockOSThread()
		return nil, err
	}
	return t, nil
}

func (t *Tray) addIcon() error {
	nid := notifyIconData{
		CbSize:           uint32(unsafe.Sizeof(notifyIconData{})),
		HWnd:             t.hwnd,
		UID:              1,
		UFlags:           nifMessage | nifIcon | nifTip,
		UCallbackMessage: t.callback,
		HIcon:            t.icon,
	}
	copyUTF16(nid.SzTip[:], t.actions.StatusText())

	r, _, err := pShellNotifyIconW.Call(nimAdd, uintptr(unsafe.Pointer(&nid)))
	if r == 0 {
		return err
	}
	t.added = true
	return nil
}

func (t *Tray) removeIcon() {
	if !t.added {
		return
	}
	nid := notifyIconData{
		CbSize: uint32(unsafe.Sizeof(notifyIconData{})),
		HWnd:   t.hwnd,
		UID:    1,
	}
	_, _, _ = pShellNotifyIconW.Call(nimDelete, uintptr(unsafe.Pointer(&nid)))
	t.added = false
}

// Update refreshes the tooltip. Safe to call from any goroutine.
func (t *Tray) Update() {
	nid := notifyIconData{
		CbSize: uint32(unsafe.Sizeof(notifyIconData{})),
		HWnd:   t.hwnd,
		UID:    1,
		UFlags: nifTip,
	}
	copyUTF16(nid.SzTip[:], t.actions.StatusText())
	_, _, _ = pShellNotifyIconW.Call(nimModify, uintptr(unsafe.Pointer(&nid)))
}

// Run blocks on the tray message loop; must run on the main goroutine.
func (t *Tray) Run() error {
	var m msg
	for {
		r, _, _ := pGetMessageW.Call(uintptr(unsafe.Pointer(&m)), 0, 0, 0)
		if int32(r) <= 0 {
			return nil
		}
		_, _, _ = pTranslateMessage.Call(uintptr(unsafe.Pointer(&m)))
		_, _, _ = pDispatchMessageW.Call(uintptr(unsafe.Pointer(&m)))
	}
}

// Stop removes the icon and quits the message loop.
func (t *Tray) Stop() {
	if t.stopped {
		return
	}
	t.stopped = true
	t.removeIcon()
	_, _, _ = pPostQuitMessage.Call(0)
}

// --- tray message window procedure ---

func trayWndProc(hwnd uintptr, m uint32, wp, lp uintptr) uintptr {
	t := currentTray
	if t == nil {
		r, _, _ := pDefWindowProcW.Call(hwnd, uintptr(m), wp, lp)
		return r
	}
	switch m {
	case t.callback:
		switch uint32(lp) {
		case wmLButtonUp:
			t.menu.hide()
			go t.actions.OnOpenCourse()
		case wmRButtonUp, wmContextMenu:
			t.menu.show()
		}
		return 0
	case wmClose:
		_, _, _ = pDestroyWindow.Call(hwnd)
		return 0
	case wmDestroy:
		_, _, _ = pPostQuitMessage.Call(0)
		return 0
	default:
		if t.taskbarID != 0 && uint32(m) == t.taskbarID {
			// Explorer restarted: re-add the icon.
			t.added = false
			_ = t.addIcon()
			return 0
		}
	}
	r, _, _ := pDefWindowProcW.Call(hwnd, uintptr(m), wp, lp)
	return r
}

func copyUTF16(dst []uint16, s string) {
	u, err := windows.UTF16FromString(s)
	if err != nil {
		return
	}
	n := copy(dst[:len(dst)-1], u)
	dst[n] = 0
}
