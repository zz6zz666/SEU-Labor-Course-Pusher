// Package session owns the HTTP client, the persistent cookie jar and the
// site endpoints. The login is bound to a session cookie, so the whole jar
// (including non-expiring cookies) must be kept.
package session

import (
	"bytes"
	"context"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

// The site rejects some requests unless a desktop Chrome UA is presented.
const UserAgent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36"

var (
	CoursePage = "https://labor.seu.edu.cn/SJItemKaiKe/XuanKe/Index"
	CasLogin   = "https://auth.seu.edu.cn/dist/#/dist/main/login?service=https://labor.seu.edu.cn/UnifiedAuth/CASLogin"
	LocalLogin = "https://labor.seu.edu.cn/AuthServer/Login"
	PushPlus   = "https://www.pushplus.plus/send"
)

type Response struct {
	StatusCode int
	Body       string
	FinalURL   string
}

func (r *Response) OK() bool { return r.StatusCode >= 200 && r.StatusCode < 400 }

type Client struct {
	HTTP *http.Client
	Jar  *Jar
}

func New(jarPath string, timeout time.Duration) (*Client, error) {
	jar, err := LoadJar(jarPath)
	if err != nil {
		return nil, err
	}
	return NewClient(jar, timeout), nil
}

func NewClient(jar *Jar, timeout time.Duration) *Client {
	return &Client{
		HTTP: &http.Client{Jar: jar, Timeout: timeout},
		Jar:  jar,
	}
}

func (c *Client) Get(ctx context.Context, rawURL string) (*Response, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, rawURL, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("User-Agent", UserAgent)
	req.Header.Set("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
	req.Header.Set("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
	return c.do(req)
}

func (c *Client) PostForm(ctx context.Context, rawURL string, form url.Values) (*Response, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, rawURL, strings.NewReader(form.Encode()))
	if err != nil {
		return nil, err
	}
	req.Header.Set("User-Agent", UserAgent)
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	req.Header.Set("Accept", "application/json, text/plain, */*")
	return c.do(req)
}

func (c *Client) PostJSON(ctx context.Context, rawURL string, body []byte) (*Response, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, rawURL, bytes.NewReader(body))
	if err != nil {
		return nil, err
	}
	req.Header.Set("User-Agent", UserAgent)
	req.Header.Set("Content-Type", "application/json; charset=utf-8")
	req.Header.Set("Accept", "application/json, text/plain, */*")
	return c.do(req)
}

func (c *Client) do(req *http.Request) (*Response, error) {
	resp, err := c.HTTP.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	data, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, err
	}
	return &Response{
		StatusCode: resp.StatusCode,
		Body:       string(data),
		FinalURL:   resp.Request.URL.String(),
	}, nil
}
