package notify

import (
	"context"

	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/logging"
)

type Channel interface {
	Name() string
	Send(ctx context.Context, e Event) error
}

// Dispatcher fans an event out to every channel. A failing channel never
// blocks the others.
type Dispatcher struct {
	log      *logging.Logger
	channels []Channel
}

func NewDispatcher(log *logging.Logger) *Dispatcher {
	return &Dispatcher{log: log}
}

func (d *Dispatcher) Add(c Channel) { d.channels = append(d.channels, c) }

func (d *Dispatcher) Dispatch(ctx context.Context, e Event) {
	for _, c := range d.channels {
		if err := c.Send(ctx, e); err != nil {
			d.log.Warn("通知通道失败", c.Name(), err)
		}
	}
}
