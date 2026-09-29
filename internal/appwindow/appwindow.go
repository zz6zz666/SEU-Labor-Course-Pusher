//go:build windows

// Package appwindow hosts the settings UI in a native Win32 window backed by the
// system Microsoft Edge WebView2 runtime. This reuses the Edge component that
// ships with Windows, so no browser engine is bundled and no .NET runtime is
// required.
package appwindow

import (
	"errors"
	"sync"

	webview2 "github.com/jchv/go-webview2"
)

// ErrUnavailable reports that the WebView2 runtime could not be created (it is
// normally present on Windows 11 and on Windows 10 machines with Edge).
var ErrUnavailable = errors.New("WebView2 运行时不可用")

type Options struct {
	Title     string
	Width     int
	Height    int
	MinWidth  int
	MinHeight int
	IconID    uint
	DataPath  string
}

// Window wraps a WebView2-hosted native window. Open must be called on the UI
// thread; Run then pumps that thread's message loop.
type Window struct {
	wv    webview2.WebView
	done  chan struct{}
	close sync.Once
}

// Open creates the window and starts loading url. It must be called on the same
// OS thread that will later call Run.
func Open(url string, opts Options) (*Window, error) {
	w := webview2.NewWithOptions(webview2.WebViewOptions{
		Debug:     false,
		AutoFocus: true,
		DataPath:  opts.DataPath,
		WindowOptions: webview2.WindowOptions{
			Title:  opts.Title,
			Width:  uint(opts.Width),
			Height: uint(opts.Height),
			IconId: opts.IconID,
			Center: true,
		},
	})
	if w == nil {
		return nil, ErrUnavailable
	}
	w.SetSize(opts.Width, opts.Height, webview2.HintNone)
	if opts.MinWidth > 0 && opts.MinHeight > 0 {
		w.SetSize(opts.MinWidth, opts.MinHeight, webview2.HintMin)
	}
	w.Navigate(url)
	return &Window{wv: w, done: make(chan struct{})}, nil
}

// Run pumps the window message loop until the window closes. It blocks and must
// run on the same OS thread as Open.
func (w *Window) Run() {
	w.wv.Run()
	close(w.done)
}

// Close asks the window to close. It is safe to call from any goroutine and is
// idempotent.
func (w *Window) Close() {
	w.close.Do(func() { w.wv.Destroy() })
}

// Done is closed once Run has returned.
func (w *Window) Done() <-chan struct{} { return w.done }
