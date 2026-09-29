// Package ui provides the resident tray icon and its menu.
package ui

// Actions are the callbacks the tray menu invokes. The status and toggle
// getters are evaluated when the menu is about to be shown, so labels and
// check marks always reflect current state.
type Actions struct {
	StatusText       func() string
	OnOpenCourse     func()
	OnLogin          func()
	OnFetchNow       func()
	OnOpenWizard     func()
	OnOpenConfig     func()
	OnOpenLogs       func()
	IsAutoStart      func() bool
	ToggleAutoStart  func()
	IsAutoSelect     func() bool
	ToggleAutoSelect func()
	OnQuit           func()
}
