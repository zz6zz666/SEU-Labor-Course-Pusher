//go:build windows

// Package singleinstance ensures only one resident process runs, and lets a
// second launch ask the running one to show its wizard.
package singleinstance

import (
	"errors"

	"golang.org/x/sys/windows"
)

const (
	mutexName = `Local\SEULaborPusher`
	eventName = `Local\SEULaborPusher_ShowWizard`
)

type Instance struct {
	mutex windows.Handle
	event windows.Handle
}

// Acquire takes the single-instance lock. already=true means another instance
// owns it (and the caller should signal and exit).
func Acquire() (inst *Instance, already bool, err error) {
	name, err := windows.UTF16PtrFromString(mutexName)
	if err != nil {
		return nil, false, err
	}
	mutex, err := windows.CreateMutex(nil, false, name)
	if err != nil {
		if errors.Is(err, windows.ERROR_ALREADY_EXISTS) {
			return nil, true, nil
		}
		return nil, false, err
	}

	evName, err := windows.UTF16PtrFromString(eventName)
	if err != nil {
		windows.CloseHandle(mutex)
		return nil, false, err
	}
	event, err := windows.CreateEvent(nil, 0, 0, evName)
	if err != nil && !errors.Is(err, windows.ERROR_ALREADY_EXISTS) {
		windows.CloseHandle(mutex)
		return nil, false, err
	}
	return &Instance{mutex: mutex, event: event}, false, nil
}

// SignalExisting wakes the running instance's wizard request.
func SignalExisting() error {
	name, err := windows.UTF16PtrFromString(eventName)
	if err != nil {
		return err
	}
	event, err := windows.OpenEvent(windows.EVENT_MODIFY_STATE, false, name)
	if err != nil {
		return err
	}
	defer windows.CloseHandle(event)
	return windows.SetEvent(event)
}

// Wait blocks until another process signals (auto-reset event).
func (i *Instance) Wait() error {
	_, err := windows.WaitForSingleObject(i.event, windows.INFINITE)
	return err
}

func (i *Instance) Release() {
	if i.event != 0 {
		windows.CloseHandle(i.event)
	}
	if i.mutex != 0 {
		windows.CloseHandle(i.mutex)
	}
}
