//go:build windows

package notify

import (
	"context"

	"github.com/gen2brain/beeep"

	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/assets"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/config"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/logging"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/osutil"
)

// Toast is the Windows desktop notification channel.
type Toast struct {
	store *config.Store
	log   *logging.Logger
}

func NewToast(store *config.Store, log *logging.Logger) *Toast {
	// The toast AppID is shown as the notification's source.
	beeep.AppName = osutil.AppDisplayName
	return &Toast{store: store, log: log}
}

func (t *Toast) Name() string { return "toast" }

func (t *Toast) Send(_ context.Context, e Event) error {
	if !t.store.Get().Push.Windows.Enabled {
		return nil
	}
	return beeep.Notify(e.Title, e.Body, assets.IconPNG)
}
