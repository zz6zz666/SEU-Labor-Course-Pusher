// Package notify formats events and fans them out to channels (PushPlus,
// desktop toast). Markdown is for remote channels, Body for the toast.
package notify

import (
	"fmt"
	"strings"
	"time"

	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/site"
)

type EventType string

const (
	NewCourse    EventType = "newCourse"
	AuthExpired  EventType = "authExpired"
	RuntimeError EventType = "runtimeError"
	DailySummary EventType = "dailySummary"
	AutoSelect   EventType = "autoSelect"
)

type Event struct {
	Type     EventType
	Title    string
	Markdown string
	Body     string
}

func timestamp() string { return time.Now().Format("2006-01-02 15:04:05") }

func formatCoursesMarkdown(courses []site.Course, title string) string {
	var b strings.Builder
	if title != "" {
		fmt.Fprintf(&b, "## %s\n\n", title)
	}
	b.WriteString("| 序号 | 项目名称 | 项目类别 | 实施时间 | 开课地点 | 选课情况 | 教师 |\n")
	b.WriteString("|------|----------|----------|----------|----------|----------|------|\n")
	for _, c := range courses {
		fmt.Fprintf(&b, "| %s | %s | %s | %s | %s | %s | %s |\n",
			c.Seq, c.Name, c.Category, c.Time, c.Location, c.Enrollment, c.Teacher)
	}
	fmt.Fprintf(&b, "\n提取时间：%s", timestamp())
	return b.String()
}

func coursePreview(courses []site.Course) string {
	var b strings.Builder
	for i, c := range courses {
		if i == 3 {
			fmt.Fprintf(&b, "\n…另有 %d 门", len(courses)-3)
			break
		}
		if i > 0 {
			b.WriteByte('\n')
		}
		fmt.Fprintf(&b, "· %s｜%s｜%s", c.Name, c.Time, c.Location)
	}
	return b.String()
}

func BuildNewCourse(courses []site.Course) Event {
	return Event{
		Type:     NewCourse,
		Title:    fmt.Sprintf("劳动教育新课程 · %d 门", len(courses)),
		Markdown: formatCoursesMarkdown(courses, fmt.Sprintf("发现 %d 门新课程", len(courses))),
		Body:     coursePreview(courses) + "\n\n点击在内置浏览器中打开选课页",
	}
}

func BuildAutoSelect(courses []site.Course) Event {
	return Event{
		Type:     AutoSelect,
		Title:    fmt.Sprintf("已自动选课 · %d 门", len(courses)),
		Markdown: formatCoursesMarkdown(courses, fmt.Sprintf("已自动选课 %d 门", len(courses))),
		Body:     coursePreview(courses) + "\n\n自动选课已提交。点击在内置浏览器中打开选课页核对。",
	}
}

func BuildAutoSelectFailure(courses []site.Course, reasons []string) Event {
	var detail strings.Builder
	for i, c := range courses {
		if i == 3 {
			break
		}
		if i > 0 {
			detail.WriteByte('\n')
		}
		reason := "未知原因"
		if i < len(reasons) && reasons[i] != "" {
			reason = reasons[i]
		}
		fmt.Fprintf(&detail, "· %s｜%s：%s", c.Name, c.Time, reason)
	}
	return Event{
		Type:  AutoSelect,
		Title: fmt.Sprintf("自动选课失败 · %d 门", len(courses)),
		Markdown: formatCoursesMarkdown(courses, fmt.Sprintf("自动选课失败 %d 门", len(courses))) +
			"\n\n**失败原因**：\n" + detail.String() +
			"\n\n程序会在后续每轮继续重试,直到成功或课程失效。",
		Body: detail.String() + "\n\n程序会继续重试。点击在内核浏览器中打开选课页",
	}
}

// BuildAuthExpired is sent only after unattended re-login has failed, so it
// really does require a human.
func BuildAuthExpired(reason string) Event {
	return Event{
		Type:  AuthExpired,
		Title: "登录失效 · 需要手动登录",
		Markdown: "## 统一身份认证登录已失效\n\n" +
			fmt.Sprintf("**原因**：%s\n\n", reason) +
			"自动重新登录未能完成(可能需要短信/图形验证码)。\n\n" +
			fmt.Sprintf("**时间**：%s\n\n", timestamp()) +
			"点击本机通知将打开登录页。",
		Body: fmt.Sprintf("自动登录失败(%s),点击打开登录页", reason),
	}
}

func BuildRuntimeError(detail string) Event {
	return Event{
		Type:  RuntimeError,
		Title: "监控运行异常",
		Markdown: "## 监控运行异常\n\n" +
			fmt.Sprintf("**详情**：%s\n\n", detail) +
			fmt.Sprintf("**时间**：%s\n\n", timestamp()) +
			"已连续失败若干次,程序会自动退避重试。",
		Body: detail + "\n点击打开日志目录",
	}
}

type SummaryStats struct {
	Date              string
	Ticks             int
	Successes         int
	PushedNew         int
	CurrentValidCount int
	AuthState         string
	TrackedCount      int
}

func BuildDailySummary(s SummaryStats) Event {
	authText := map[string]string{"valid": "正常", "expired": "已失效"}[s.AuthState]
	if authText == "" {
		authText = "未知"
	}
	return Event{
		Type:  DailySummary,
		Title: fmt.Sprintf("运行汇总 · %s", s.Date),
		Markdown: fmt.Sprintf("## 每日运行汇总(%s)\n\n", s.Date) +
			"| 指标 | 数值 |\n|---|---|\n" +
			fmt.Sprintf("| 抓取次数 | %d |\n", s.Ticks) +
			fmt.Sprintf("| 成功次数 | %d |\n", s.Successes) +
			fmt.Sprintf("| 新课程推送 | %d |\n", s.PushedNew) +
			fmt.Sprintf("| 当前可选课程 | %d |\n", s.CurrentValidCount) +
			fmt.Sprintf("| 登录态 | %s |\n", authText) +
			fmt.Sprintf("| 已记录课程 | %d |\n\n", s.TrackedCount) +
			fmt.Sprintf("统计时间：%s", timestamp()),
		Body: fmt.Sprintf("抓取 %d 次 / 成功 %d 次\n新课程推送 %d 门,当前可选 %d 门\n登录态:%s",
			s.Ticks, s.Successes, s.PushedNew, s.CurrentValidCount, authText),
	}
}
