package notify

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"time"

	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/config"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/logging"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/session"
)

// PushPlus sends WeChat notifications via pushplus.plus.
type PushPlus struct {
	Store  *config.Store
	Log    *logging.Logger
	Client *http.Client
}

func NewPushPlus(store *config.Store, log *logging.Logger) *PushPlus {
	return &PushPlus{Store: store, Log: log, Client: &http.Client{Timeout: 15 * time.Second}}
}

func (p *PushPlus) Name() string { return "pushplus" }

func (p *PushPlus) Send(ctx context.Context, e Event) error {
	cfg := p.Store.Get().Push.PushPlus
	if !cfg.Enabled {
		return nil
	}
	if cfg.Token == "" {
		return fmt.Errorf("未配置 PushPlus Token")
	}

	payload, err := json.Marshal(map[string]string{
		"token":    cfg.Token,
		"title":    e.Title,
		"content":  e.Markdown,
		"template": "markdown",
	})
	if err != nil {
		return err
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, session.PushPlus, bytes.NewReader(payload))
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", "application/json; charset=utf-8")

	resp, err := p.Client.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)

	var parsed struct {
		Code int    `json:"code"`
		Msg  string `json:"msg"`
	}
	if err := json.Unmarshal(body, &parsed); err != nil {
		return fmt.Errorf("PushPlus 响应解析失败: %w", err)
	}
	if parsed.Code != 200 {
		return fmt.Errorf("PushPlus 拒绝: code=%d msg=%s", parsed.Code, parsed.Msg)
	}
	p.Log.Info("微信推送成功", e.Type)
	return nil
}
