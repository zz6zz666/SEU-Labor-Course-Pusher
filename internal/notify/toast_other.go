//go:build !windows

package notify

import (
	"context"

	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/config"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/logging"
)

// Toast is a no-op desktop notification channel on non-Windows platforms.
type Toast struct {
	store *config.Store
	log   *logging.Logger
}

func NewToast(store *config.Store, log *logging.Logger) *Toast {
	return &Toast{store: store, log: log}
}

func (t *Toast) Name() string { return "toast" }

func (t *Toast) Send(_ context.Context, _ Event) error { return nil }
