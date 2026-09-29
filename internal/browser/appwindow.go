package browser

import (
	"os/exec"
)

// AppWindow is a Chromium window opened in "app mode": no tabs, no address
// bar, its own persistent profile. It gives the integrated feel of a built-in
// browser without bundling one.
type AppWindow struct {
	cmd *exec.Cmd
}

// OpenApp opens url as a standalone application window and returns a handle.
// windowWidth/Height of 0 use the browser default.
func OpenApp(url, profileDir string, windowWidth, windowHeight int) (*AppWindow, error) {
	exe, err := FindExecPath()
	if err != nil {
		return nil, err
	}
	if profileDir != "" {
		if err := prepareProfile(profileDir); err != nil {
			return nil, err
		}
	}

	args := []string{
		"--app=" + url,
		"--start-maximized",
		"--no-first-run",
		"--no-default-browser-check",
		"--disable-features=Translate,AutofillServerCommunication",
	}
	if profileDir != "" {
		args = append(args, "--user-data-dir="+profileDir)
	}

	cmd := exec.Command(exe, args...)
	if err := cmd.Start(); err != nil {
		return nil, err
	}
	return &AppWindow{cmd: cmd}, nil
}

// Wait blocks until the window is closed by the user.
func (w *AppWindow) Wait() error { return w.cmd.Wait() }

// Close terminates the window.
func (w *AppWindow) Close() error {
	if w.cmd.Process == nil {
		return nil
	}
	if err := w.cmd.Process.Kill(); err != nil {
		return err
	}
	return nil
}
