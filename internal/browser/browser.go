// Package browser defines the seam for the only operations that need a real
// JS engine: interactive login (captcha) and viewing the course page. Polling,
// selection and cancellation are plain HTTP and never use it.
//
// The concrete engine is the system Edge/Chrome driven over CDP. The browser
// owns its own context; callers cancel by closing it.
package browser

import (
	"encoding/json"
	"errors"
	"net/http"
	"os"
	"path/filepath"
)

type Browser interface {
	// Navigate loads url and waits for the document to finish loading.
	Navigate(url string) error
	// Eval runs script in the page context and decodes its JSON result.
	Eval(script string) (json.RawMessage, error)
	// AllCookies returns every cookie in the profile, including session
	// cookies, so the HTTP client can reuse the login.
	AllCookies() ([]*http.Cookie, error)
	// SetCookies injects cookies into the profile. Required before navigating:
	// session cookies are not retained in the on-disk profile.
	SetCookies(cookies []*http.Cookie) error
	Close() error
}

// FindExecPath locates the system Chromium browser, preferring Edge (present
// on every supported Windows).
func FindExecPath() (string, error) {
	candidates := []string{
		filepath.Join(os.Getenv("ProgramFiles(x86)"), "Microsoft", "Edge", "Application", "msedge.exe"),
		filepath.Join(os.Getenv("ProgramFiles"), "Microsoft", "Edge", "Application", "msedge.exe"),
		filepath.Join(os.Getenv("LocalAppData"), "Microsoft", "Edge", "Application", "msedge.exe"),
		filepath.Join(os.Getenv("ProgramFiles"), "Google", "Chrome", "Application", "chrome.exe"),
		filepath.Join(os.Getenv("ProgramFiles(x86)"), "Google", "Chrome", "Application", "chrome.exe"),
		filepath.Join(os.Getenv("LocalAppData"), "Google", "Chrome", "Application", "chrome.exe"),
	}
	for _, p := range candidates {
		if p == "" {
			continue
		}
		if _, err := os.Stat(p); err == nil {
			return p, nil
		}
	}
	return "", errors.New("未找到系统 Edge 或 Chrome,无法打开登录窗口")
}
