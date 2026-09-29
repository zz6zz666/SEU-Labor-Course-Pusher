//go:build !windows

package autostart

func IsEnabled() bool         { return false }
func Enable() error           { return nil }
func Disable() error          { return nil }
func Apply(enabled bool) bool { return false }
