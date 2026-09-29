// Package watcher is the polling state machine: fetch, classify, parse, diff,
// notify, back off. It relies on HTTP only; the course list is server-rendered.
package watcher

import (
	"context"
	"fmt"
	"math"
	"math/rand"
	"strconv"
	"time"

	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/config"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/logging"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/notify"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/selection"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/session"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/site"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/state"
)

// AutoSelector submits selections for eligible courses (over HTTP).
type AutoSelector interface {
	Select(ctx context.Context, targets []selection.Target) ([]selection.Outcome, error)
}

type Status struct {
	LastVerdict   string
	LastMessage   string
	CurrentValid  int
	PushedToday   int
	Failures      int
	NextRun       time.Time
	LastSuccessAt string
}

type Watcher struct {
	store    *config.Store
	state    *state.Store
	client   *session.Client
	notify   *notify.Dispatcher
	log      *logging.Logger
	selector AutoSelector

	failures      int
	authAlerted   bool
	stats         dayStats
	trigger       chan struct{}
	onAuthExpired func(reason string)

	status Status
}

type dayStats struct {
	date      string
	ticks     int
	successes int
	pushedNew int
}

func New(store *config.Store, st *state.Store, client *session.Client, n *notify.Dispatcher, log *logging.Logger, selector AutoSelector) *Watcher {
	return &Watcher{
		store:    store,
		state:    st,
		client:   client,
		notify:   n,
		log:      log,
		selector: selector,
		stats:    dayStats{date: today()},
		trigger:  make(chan struct{}, 1),
		status:   Status{LastVerdict: "idle", LastMessage: "等待首次抓取"},
	}
}

// SetOnAuthExpired registers a callback invoked once per expiry episode. The
// caller owns recovery (silent re-login, then manual fallback); the watcher
// itself sends no notification, so a successful silent login stays silent.
func (w *Watcher) SetOnAuthExpired(fn func(reason string)) { w.onAuthExpired = fn }

// TriggerNow asks Run to poll immediately.
func (w *Watcher) TriggerNow() {
	select {
	case w.trigger <- struct{}{}:
	default:
	}
}

// Run polls until ctx is cancelled.
func (w *Watcher) Run(ctx context.Context) {
	w.log.Info("监控循环启动")
	for {
		w.Tick(ctx)
		w.maybeSummary(ctx)

		delay := w.nextDelay()
		w.status.NextRun = time.Now().Add(delay)
		w.log.Debug("下次抓取", delay.Round(time.Second))

		select {
		case <-ctx.Done():
			w.log.Info("监控循环停止")
			return
		case <-w.trigger:
			w.failures = 0
		case <-time.After(delay):
		}
	}
}

func (w *Watcher) Status() Status { return w.status }

func (w *Watcher) Tick(ctx context.Context) {
	w.rollover()
	w.stats.ticks++
	w.status.LastVerdict = "idle"
	w.status.LastMessage = "正在抓取…"

	probe, err := site.Check(ctx, w.client)
	if err != nil {
		w.fail(ctx, "请求失败: "+err.Error())
		return
	}

	switch probe.Verdict {
	case site.VerdictCourses:
		w.handleCourses(ctx, probe.HTML)
	case site.VerdictLogin:
		w.handleAuthExpired(ctx, probe.Reason)
	default:
		w.fail(ctx, "无法判定: "+probe.Reason)
	}
}

func (w *Watcher) handleCourses(ctx context.Context, pageHTML string) {
	cfg := w.store.Get()
	result := site.Parse(pageHTML, site.FilterOptions{
		Locations:  cfg.Filters.Locations,
		Categories: cfg.Filters.Categories,
	})

	if !result.TableFound {
		w.fail(ctx, "页面可达但解析不到课程表格")
		return
	}

	w.failures = 0
	w.authAlerted = false
	_ = w.state.SetAuthState(state.AuthValid)

	reconciled := site.ReconcilePushed(result.Courses, w.state.Get().PushedUniqueIDs)
	_ = w.state.SetPushed(reconciled)
	pushedSet := make(map[string]struct{}, len(reconciled))
	for _, id := range reconciled {
		pushedSet[id] = struct{}{}
	}

	newCourses := site.DiffNew(result.Courses, pushedSet)
	if len(newCourses) > 0 {
		w.log.Info("发现新课程", len(newCourses))
		w.notify.Dispatch(ctx, notify.BuildNewCourse(newCourses))
		_ = w.state.AddPushed(idsOf(newCourses))
		w.stats.pushedNew += len(newCourses)
	}

	selectNote := ""
	if cfg.Behavior.AutoSelect && w.selector != nil {
		selectNote = w.runAutoSelect(ctx, eligibleCourses(result.Courses), result.Courses)
	}

	now := time.Now().UTC().Format(time.RFC3339)
	w.stats.successes++
	_ = w.state.Update(func(s *state.State) {
		s.LastRunAt = now
		s.LastSuccessAt = now
		s.ConsecutiveFailures = 0
	})
	w.status.CurrentValid = countEligible(result.Courses)
	w.status.PushedToday = w.stats.pushedNew
	w.status.Failures = 0
	w.status.LastVerdict = "courses"
	w.status.LastSuccessAt = now
	w.status.LastMessage = "抓取成功 · 共 " + strconv.Itoa(result.RowCount) + " 条,符合条件 " + strconv.Itoa(w.status.CurrentValid) + " 门" + selectNote
	w.log.Info(w.status.LastMessage)
}

// runAutoSelect attempts the eligible courses that have not been handled yet.
func (w *Watcher) runAutoSelect(ctx context.Context, eligible []site.Course, all []site.Course) string {
	allIDs := make(map[string]struct{}, len(all))
	for _, c := range all {
		allIDs[c.UniqueID()] = struct{}{}
	}
	handled := make(map[string]struct{})
	kept := make([]string, 0, len(w.state.Get().AutoHandledIDs))
	for _, id := range w.state.Get().AutoHandledIDs {
		if _, ok := allIDs[id]; ok {
			kept = append(kept, id)
			handled[id] = struct{}{}
		}
	}
	_ = w.state.SetAutoHandled(kept)

	var targets []selection.Target
	byID := make(map[string]site.Course, len(eligible))
	for _, c := range eligible {
		byID[c.UniqueID()] = c
		if c.ItemID == "" || c.KaiKeID == "" {
			continue
		}
		if _, done := handled[c.UniqueID()]; done {
			continue
		}
		targets = append(targets, selection.Target{
			UniqueID: c.UniqueID(), Name: c.Name, ItemID: c.ItemID, KaiKeID: c.KaiKeID,
		})
	}
	if len(targets) == 0 {
		return ""
	}

	w.log.Info("自动选课尝试", len(targets))
	outcomes, err := w.selector.Select(ctx, targets)
	if err != nil {
		w.log.Error("自动选课异常:", err)
		return " · 自动选课异常"
	}

	var selected, already, failed []selection.Outcome
	for _, o := range outcomes {
		switch o.Status {
		case selection.Selected:
			selected = append(selected, o)
		case selection.Already:
			already = append(already, o)
		default:
			failed = append(failed, o)
		}
	}

	if len(selected) > 0 {
		_ = w.state.AddAutoHandled(outcomeIDs(selected))
		w.log.Info("自动选课成功", len(selected))
		w.notify.Dispatch(ctx, notify.BuildAutoSelect(coursesOf(selected, byID)))
	}
	if len(already) > 0 {
		_ = w.state.AddAutoHandled(outcomeIDs(already))
	}
	if len(failed) > 0 {
		w.log.Warn("自动选课失败", len(failed))
		w.notify.Dispatch(ctx, notify.BuildAutoSelectFailure(coursesOf(failed, byID), outcomeMessages(failed)))
	}
	return fmt.Sprintf(" · 自动选课 成功%d/已选%d/失败%d", len(selected), len(already), len(failed))
}

func outcomeIDs(outcomes []selection.Outcome) []string {
	ids := make([]string, len(outcomes))
	for i, o := range outcomes {
		ids[i] = o.Target.UniqueID
	}
	return ids
}

func outcomeMessages(outcomes []selection.Outcome) []string {
	msgs := make([]string, len(outcomes))
	for i, o := range outcomes {
		msgs[i] = o.Message
	}
	return msgs
}

func coursesOf(outcomes []selection.Outcome, byID map[string]site.Course) []site.Course {
	out := make([]site.Course, 0, len(outcomes))
	for _, o := range outcomes {
		if c, ok := byID[o.Target.UniqueID]; ok {
			out = append(out, c)
		}
	}
	return out
}

func eligibleCourses(courses []site.Course) []site.Course {
	var out []site.Course
	for _, c := range courses {
		if c.Eligible() {
			out = append(out, c)
		}
	}
	return out
}

func (w *Watcher) handleAuthExpired(ctx context.Context, reason string) {
	w.failures++
	w.log.Warn("登录失效:", reason)
	_ = w.state.SetAuthState(state.AuthExpired)
	_ = w.state.Update(func(s *state.State) { s.ConsecutiveFailures = w.failures })

	if !w.authAlerted {
		w.authAlerted = true
		if w.onAuthExpired != nil {
			w.onAuthExpired(reason)
		}
	}
	w.status.LastVerdict = "loginPage"
	w.status.Failures = w.failures
	w.status.LastMessage = "登录失效: " + reason
}

func (w *Watcher) fail(ctx context.Context, detail string) {
	w.failures++
	w.log.Warn(detail)
	_ = w.state.Update(func(s *state.State) { s.ConsecutiveFailures = w.failures })

	threshold := w.store.Get().Schedule.FailureAlertThreshold
	if w.failures == threshold {
		w.notify.Dispatch(ctx, notify.BuildRuntimeError(detail))
	}
	w.status.LastVerdict = "error"
	w.status.Failures = w.failures
	w.status.LastMessage = detail
}

func (w *Watcher) maybeSummary(ctx context.Context) {
	cfg := w.store.Get()
	if cfg.Schedule.DailySummaryHour == nil {
		return
	}
	todayStr := today()
	if w.state.Get().LastSummaryDate == todayStr {
		return
	}
	// Only send during the configured hour, so starting the app later in the
	// day does not backfill a meaningless zero-activity summary.
	if time.Now().Hour() != *cfg.Schedule.DailySummaryHour {
		return
	}

	auth := string(w.state.Get().AuthState)
	w.notify.Dispatch(ctx, notify.BuildDailySummary(notify.SummaryStats{
		Date:              todayStr,
		Ticks:             w.stats.ticks,
		Successes:         w.stats.successes,
		PushedNew:         w.stats.pushedNew,
		CurrentValidCount: w.status.CurrentValid,
		AuthState:         auth,
		TrackedCount:      len(w.state.Get().PushedUniqueIDs),
	}))
	_ = w.state.Update(func(s *state.State) { s.LastSummaryDate = todayStr })
}

func (w *Watcher) nextDelay() time.Duration {
	cfg := w.store.Get().Schedule
	base := float64(cfg.RefreshIntervalMS)
	if w.failures > 0 {
		base = math.Min(base*math.Pow(2, float64(w.failures)), float64(cfg.MaxBackoffMS))
	}
	jitter := 1 + (rand.Float64()*2-1)*cfg.JitterRatio
	delay := time.Duration(base * jitter * float64(time.Millisecond))
	if delay < 15*time.Second {
		delay = 15 * time.Second
	}
	return delay
}

func (w *Watcher) rollover() {
	if d := today(); d != w.stats.date {
		w.stats = dayStats{date: d}
	}
}

func countEligible(courses []site.Course) int {
	n := 0
	for _, c := range courses {
		if c.Eligible() {
			n++
		}
	}
	return n
}

func idsOf(courses []site.Course) []string {
	out := make([]string, len(courses))
	for i, c := range courses {
		out[i] = c.UniqueID()
	}
	return out
}

func today() string { return time.Now().Format("2006-01-02") }
