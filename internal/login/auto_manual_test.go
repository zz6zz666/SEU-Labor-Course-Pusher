package login

import (
	"context"
	"net/url"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/config"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/session"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/site"
)

// TestManualSilentLoginHTTP performs a real browser-free login and verifies the
// resulting session. Credentials come from the live config and are never logged.
func TestManualSilentLoginHTTP(t *testing.T) {
	dir := os.Getenv("SEU_LIVE_DIR")
	if dir == "" {
		t.Skip("set SEU_LIVE_DIR to the app data dir")
	}
	store, _, err := config.Open(filepath.Join(dir, "config.json"))
	if err != nil {
		t.Fatalf("config: %v", err)
	}
	cred := store.Get().Credentials
	if cred.Username == "" || cred.Password == "" {
		t.Skip("no credentials configured")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()

	res := AutoRun(ctx, AutoOptions{Username: cred.Username, Password: cred.Password})
	t.Logf("outcome=%s detail=%s", res.Outcome, res.Detail)
	if res.Outcome != AutoSuccess {
		t.Fatalf("silent login failed: %s", res.Detail)
	}

	jar := session.EmptyJar()
	u, _ := url.Parse(session.CoursePage)
	jar.SetCookies(u, res.Cookies)
	probe, _ := site.Check(ctx, session.NewClient(jar, 20*time.Second))
	t.Logf("verdict=%v reason=%s", probe.Verdict, probe.Reason)
	if probe.Verdict != site.VerdictCourses {
		t.Fatalf("session not valid after login")
	}
}
