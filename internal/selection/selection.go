// Package selection submits course selections (选课) and cancellations (退选)
// over plain HTTP, without any browser.
//
// The course page is server-rendered, so an authenticated GET yields everything
// needed: the antiforgery token (a hidden input) and each row's IDs. The site's
// own changeAjax.postAntiForgery is a plain form POST carrying that token, so a
// bare request is equivalent to clicking the button (minus the confirm dialog).
package selection

import (
	"context"
	"encoding/json"
	"fmt"
	"net/url"
	"regexp"
	"strings"

	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/session"
)

type Status string

const (
	Selected  Status = "selected"
	Cancelled Status = "cancelled"
	Already   Status = "already"
	NotFound  Status = "notFound"
	Failed    Status = "failed"
)

// Target identifies a course row by its stable (SJItemID, SJItemKaiKeID) pair.
type Target struct {
	UniqueID string
	Name     string
	ItemID   string
	KaiKeID  string
}

type Outcome struct {
	Target  Target
	Status  Status
	Message string
}

// Runner performs select/cancel against an authenticated HTTP client.
type Runner struct {
	Client *session.Client
}

const (
	selectPath = "/SJItemKaiKe/XuanKe/StudentXuanKe"
	cancelPath = "/SJItemKaiKe/XuanKe/StudentCancelXuanKe"
)

var (
	reToken = regexp.MustCompile(`name="__RequestVerificationToken"[^>]*value="([^"]+)"`)
	reRow   = regexp.MustCompile(`(?s)<tr[^>]*c--tr.*?</tr>`)
	reID    = regexp.MustCompile(`data-name="ID"[^>]*data-value="([^"]*)"`)
)

// Select submits course selections.
func (r *Runner) Select(ctx context.Context, targets []Target) ([]Outcome, error) {
	if len(targets) == 0 {
		return nil, nil
	}
	html, token, err := r.coursePage(ctx)
	if err != nil {
		return nil, err
	}

	out := make([]Outcome, 0, len(targets))
	for _, t := range targets {
		row := rowFor(html, t)
		if row == "" {
			out = append(out, Outcome{Target: t, Status: NotFound, Message: "页面上找不到对应行"})
			continue
		}
		button := buttonTag(row, "StudentXuanKe")
		if button == "" {
			out = append(out, Outcome{Target: t, Status: NotFound, Message: "找不到选课按钮"})
			continue
		}
		if disabled(button) {
			out = append(out, Outcome{Target: t, Status: Already, Message: "按钮不可用(已选或已满)"})
			continue
		}
		ok, msg, err := r.post(ctx, selectPath, url.Values{
			"SJItemID":                   {t.ItemID},
			"SJItemKaiKeID":              {t.KaiKeID},
			"__RequestVerificationToken": {token},
		})
		out = append(out, outcomeOf(t, ok, msg, err, Selected))
	}
	return out, nil
}

// Cancel withdraws course selections. Unlike selection it also needs the row's
// record ID, taken from the same page.
func (r *Runner) Cancel(ctx context.Context, targets []Target) ([]Outcome, error) {
	if len(targets) == 0 {
		return nil, nil
	}
	html, token, err := r.coursePage(ctx)
	if err != nil {
		return nil, err
	}

	out := make([]Outcome, 0, len(targets))
	for _, t := range targets {
		row := rowFor(html, t)
		if row == "" {
			out = append(out, Outcome{Target: t, Status: NotFound, Message: "页面上找不到对应行"})
			continue
		}
		button := buttonTag(row, "StudentCancelXuanKe")
		if button == "" {
			out = append(out, Outcome{Target: t, Status: NotFound, Message: "找不到取消选课按钮(可能未选该课)"})
			continue
		}
		if disabled(button) {
			out = append(out, Outcome{Target: t, Status: Already, Message: "取消选课按钮不可用"})
			continue
		}
		id := ""
		if m := reID.FindStringSubmatch(row); m != nil {
			id = m[1]
		}
		if id == "" {
			out = append(out, Outcome{Target: t, Status: Failed, Message: "缺少选课记录 ID"})
			continue
		}
		ok, msg, err := r.post(ctx, cancelPath, url.Values{
			"ID":                         {id},
			"SJItemID":                   {t.ItemID},
			"SJItemKaiKeID":              {t.KaiKeID},
			"__RequestVerificationToken": {token},
		})
		out = append(out, outcomeOf(t, ok, msg, err, Cancelled))
	}
	return out, nil
}

func (r *Runner) coursePage(ctx context.Context) (html, token string, err error) {
	resp, err := r.Client.Get(ctx, session.CoursePage)
	if err != nil {
		return "", "", err
	}
	if resp.StatusCode != 200 {
		return "", "", fmt.Errorf("选课页返回 %d", resp.StatusCode)
	}
	m := reToken.FindStringSubmatch(resp.Body)
	if m == nil {
		return "", "", fmt.Errorf("选课页缺少请求令牌(登录态可能已失效)")
	}
	return resp.Body, m[1], nil
}

func (r *Runner) post(ctx context.Context, path string, form url.Values) (bool, string, error) {
	resp, err := r.Client.PostForm(ctx, siteURL(path), form)
	if err != nil {
		return false, "", err
	}
	var res struct {
		Success bool   `json:"Success"`
		Message string `json:"Message"`
	}
	if err := json.Unmarshal([]byte(resp.Body), &res); err != nil {
		return false, "", fmt.Errorf("响应不是 JSON: %s", firstLine(resp.Body))
	}
	return res.Success, res.Message, nil
}

func outcomeOf(t Target, ok bool, msg string, err error, success Status) Outcome {
	if err != nil {
		return Outcome{Target: t, Status: Failed, Message: err.Error()}
	}
	if ok {
		return Outcome{Target: t, Status: success, Message: msg}
	}
	return Outcome{Target: t, Status: Failed, Message: msg}
}

func rowFor(html string, t Target) string {
	item := `data-name="SJItemID" data-value="` + t.ItemID + `"`
	kai := `data-name="SJItemKaiKeID" data-value="` + t.KaiKeID + `"`
	for _, row := range reRow.FindAllString(html, -1) {
		if strings.Contains(row, item) && strings.Contains(row, kai) {
			return row
		}
	}
	return ""
}

// buttonTag returns the opening tag of the button with the given data-command.
func buttonTag(row, command string) string {
	marker := `data-command="` + command + `"`
	i := strings.Index(row, marker)
	if i < 0 {
		return ""
	}
	start := strings.LastIndex(row[:i], "<")
	end := strings.Index(row[i:], ">")
	if start < 0 || end < 0 {
		return ""
	}
	return row[start : i+end+1]
}

func disabled(tag string) bool {
	return strings.Contains(tag, "disabled") || strings.Contains(tag, "c--lock")
}

func siteURL(path string) string {
	u, err := url.Parse(session.CoursePage)
	if err != nil {
		return path
	}
	return u.Scheme + "://" + u.Host + path
}

func firstLine(s string) string {
	if i := strings.IndexAny(s, "\r\n"); i >= 0 {
		s = s[:i]
	}
	if len(s) > 200 {
		s = s[:200]
	}
	return s
}
