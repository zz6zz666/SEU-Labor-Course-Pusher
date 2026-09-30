package site

import (
	"context"
	"net/url"
	"strings"

	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/session"
)

type Verdict int

const (
	VerdictUnknown Verdict = iota
	VerdictCourses
	VerdictLogin
)

// Probe answers the login question by asking the server, not by guessing from
// heartbeats: a course table means a valid session, a redirect to the auth
// host means it expired.
type Probe struct {
	Verdict Verdict
	Reason  string
	HTML    string
}

var loginMarkers = []string{
	"casLogin",
	"needCaptcha",
	"统一身份认证",
	"CaptchaDeText",
	"CaptchaInputText",
	"input-username-pc",
	"AuthServer/Login",
}

func Check(ctx context.Context, client *session.Client) (Probe, error) {
	res, err := client.Get(ctx, session.CoursePage)
	if err != nil {
		return Probe{Verdict: VerdictUnknown, Reason: "请求失败: " + err.Error()}, err
	}
	if hasCourseTable(res.Body) {
		return Probe{Verdict: VerdictCourses, Reason: "返回选课表格", HTML: res.Body}, nil
	}
	if redirectedToAuth(res.FinalURL) {
		return Probe{Verdict: VerdictLogin, Reason: "被重定向到统一身份认证", HTML: res.Body}, nil
	}
	if looksLikeLoginPage(res.Body) {
		return Probe{Verdict: VerdictLogin, Reason: "正文含登录页特征", HTML: res.Body}, nil
	}
	return Probe{Verdict: VerdictUnknown, Reason: "无法判定(" + res.FinalURL + ")"}, nil
}

func hasCourseTable(pageHTML string) bool { return strings.Contains(pageHTML, courseTableID) }

func looksLikeLoginPage(pageHTML string) bool {
	for _, m := range loginMarkers {
		if strings.Contains(pageHTML, m) {
			return true
		}
	}
	return false
}

func redirectedToAuth(finalURL string) bool {
	u, err := url.Parse(finalURL)
	if err != nil {
		return false
	}
	return u.Hostname() == "auth.seu.edu.cn" || strings.Contains(u.Path, "AuthServer/Login")
}
