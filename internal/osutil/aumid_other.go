//go:build !windows

package osutil

func SetAppUserModelID(string) error { return nil }
