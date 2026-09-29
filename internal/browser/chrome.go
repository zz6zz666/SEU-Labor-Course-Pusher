package browser

import (
	"context"
	"encoding/json"
	"net/http"
	"time"

	"github.com/chromedp/cdproto/cdp"
	"github.com/chromedp/cdproto/network"
	"github.com/chromedp/cdproto/runtime"
	"github.com/chromedp/cdproto/storage"
	"github.com/chromedp/chromedp"
)

// awaitPromise makes Evaluate wait for an async expression to resolve.
func awaitPromise(p *runtime.EvaluateParams) *runtime.EvaluateParams {
	return p.WithAwaitPromise(true)
}

// Chrome drives the system Chromium browser through CDP. It never bundles an
// engine: the executable is Edge/Chrome that is already installed.
type Chrome struct {
	allocCancel context.CancelFunc
	ctx         context.Context
	cancel      context.CancelFunc
	unlock      func()
}

type LaunchOptions struct {
	Visible    bool
	ProfileDir string
	ExecPath   string
	// AppURL, when set, opens the browser in app mode (chromeless window),
	// started maximized so interactive pages fill the screen.
	AppURL string
}

func Launch(parent context.Context, opts LaunchOptions) (*Chrome, error) {
	unlock := acquireProfile(opts.ProfileDir)
	success := false
	defer func() {
		if !success {
			unlock()
		}
	}()

	execPath := opts.ExecPath
	if execPath == "" {
		found, err := FindExecPath()
		if err != nil {
			return nil, err
		}
		execPath = found
	}
	if opts.ProfileDir != "" {
		if err := prepareProfile(opts.ProfileDir); err != nil {
			return nil, err
		}
	}

	allocOpts := append(chromedp.DefaultExecAllocatorOptions[:],
		chromedp.ExecPath(execPath),
		chromedp.UserDataDir(opts.ProfileDir),
		chromedp.Flag("headless", !opts.Visible),
		chromedp.Flag("enable-automation", false),
		chromedp.Flag("no-first-run", true),
		chromedp.Flag("no-default-browser-check", true),
		chromedp.Flag("disable-gpu", true),
		chromedp.Flag("disable-dev-shm-usage", true),
		// The course page uses timers/AJAX; keep it running when occluded.
		chromedp.Flag("disable-background-timer-throttling", true),
		chromedp.Flag("disable-renderer-backgrounding", true),
		chromedp.Flag("disable-backgrounding-occluded-windows", true),
	)
	if opts.AppURL != "" {
		allocOpts = append(allocOpts,
			chromedp.Flag("app", opts.AppURL),
			chromedp.Flag("start-maximized", true),
		)
	}

	allocCtx, allocCancel := chromedp.NewExecAllocator(parent, allocOpts...)
	ctx, cancel := chromedp.NewContext(allocCtx)
	if err := chromedp.Run(ctx); err != nil {
		cancel()
		allocCancel()
		return nil, err
	}
	success = true
	return &Chrome{allocCancel: allocCancel, ctx: ctx, cancel: cancel, unlock: unlock}, nil
}

func (c *Chrome) Navigate(url string) error {
	ctx, cancel := context.WithTimeout(c.ctx, 60*time.Second)
	defer cancel()
	return chromedp.Run(ctx, chromedp.Navigate(url))
}

func (c *Chrome) Eval(script string) (json.RawMessage, error) {
	ctx, cancel := context.WithTimeout(c.ctx, 60*time.Second)
	defer cancel()
	var raw json.RawMessage
	if err := chromedp.Run(ctx, chromedp.Evaluate(script, &raw, awaitPromise)); err != nil {
		return nil, err
	}
	return raw, nil
}

func (c *Chrome) AllCookies() ([]*http.Cookie, error) {
	ctx, cancel := context.WithTimeout(c.ctx, 30*time.Second)
	defer cancel()

	var out []*http.Cookie
	err := chromedp.Run(ctx, chromedp.ActionFunc(func(ctx context.Context) error {
		cookies, err := storage.GetCookies().Do(ctx)
		if err != nil {
			return err
		}
		for _, ck := range cookies {
			out = append(out, toHTTPCookie(ck))
		}
		return nil
	}))
	if err != nil {
		return nil, err
	}
	return out, nil
}

func (c *Chrome) SetCookies(cookies []*http.Cookie) error {
	params := make([]*network.CookieParam, 0, len(cookies))
	for _, ck := range cookies {
		p := &network.CookieParam{
			Name:     ck.Name,
			Value:    ck.Value,
			Domain:   ck.Domain,
			Path:     ck.Path,
			Secure:   ck.Secure,
			HTTPOnly: ck.HttpOnly,
		}
		if !ck.Expires.IsZero() {
			e := cdp.TimeSinceEpoch(ck.Expires)
			p.Expires = &e
		}
		params = append(params, p)
	}
	ctx, cancel := context.WithTimeout(c.ctx, 30*time.Second)
	defer cancel()
	return chromedp.Run(ctx, network.SetCookies(params))
}

func (c *Chrome) Close() error {
	c.cancel()
	c.allocCancel()
	if c.unlock != nil {
		c.unlock()
		c.unlock = nil
	}
	return nil
}

func toHTTPCookie(ck *network.Cookie) *http.Cookie {
	// Session cookies (Expires unset) keep a zero time so we persist them.
	var expires time.Time
	if !ck.Session && ck.Expires > 0 {
		expires = time.Unix(int64(ck.Expires), 0)
	}
	return &http.Cookie{
		Name:     ck.Name,
		Value:    ck.Value,
		Domain:   ck.Domain,
		Path:     ck.Path,
		Expires:  expires,
		Secure:   ck.Secure,
		HttpOnly: ck.HTTPOnly,
	}
}
