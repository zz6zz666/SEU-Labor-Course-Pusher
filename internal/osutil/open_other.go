//go:build !windows

package osutil

import (
	"errors"
	"os/exec"
	"regexp"
	"runtime"
)

var httpURL = regexp.MustCompile(`^https?://`)

func opener() string {
	if runtime.GOOS == "darwin" {
		return "open"
	}
	return "xdg-open"
}

func OpenTarget(target string) error { return exec.Command(opener(), target).Start() }
func OpenFolder(dir string) error    { return exec.Command(opener(), dir).Start() }

func OpenURL(url string) error {
	if !httpURL.MatchString(url) {
		return errors.New("拒绝打开非 http(s) 链接")
	}
	return OpenTarget(url)
}
