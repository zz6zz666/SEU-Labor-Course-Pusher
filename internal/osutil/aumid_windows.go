//go:build windows

package osutil

import (
	"unsafe"

	"golang.org/x/sys/windows"
)

// SetAppUserModelID sets the process AppUserModelID, required for desktop
// toast notifications to be attributed to this app.
func SetAppUserModelID(id string) error {
	proc := windows.NewLazySystemDLL("shell32.dll").NewProc("SetCurrentProcessExplicitAppUserModelID")
	p, err := windows.UTF16PtrFromString(id)
	if err != nil {
		return err
	}
	if r, _, e := proc.Call(uintptr(unsafe.Pointer(p))); r != 0 {
		return e
	}
	return nil
}
