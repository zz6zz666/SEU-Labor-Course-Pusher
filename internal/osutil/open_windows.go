//go:build windows

// Package osutil opens files, folders and URLs with the shell.
package osutil

import (
	"errors"
	"os/exec"
	"regexp"
)

var httpURL = regexp.MustCompile(`^https?://`)

func OpenTarget(target string) error {
	return exec.Command("rundll32", "url.dll,FileProtocolHandler", target).Start()
}

func OpenFolder(dir string) error {
	return exec.Command("explorer", dir).Start()
}

func OpenURL(url string) error {
	if !httpURL.MatchString(url) {
		return errors.New("拒绝打开非 http(s) 链接")
	}
	return OpenTarget(url)
}
