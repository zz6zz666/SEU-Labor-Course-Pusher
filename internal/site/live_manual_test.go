package site

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/session"
)

// TestLiveCourses prints the current course list using a saved session.
//
// Run with:  $env:SEU_LIVE_DIR="C:\...\data"; go test ./internal/site -run TestLiveCourses -v
func TestLiveCourses(t *testing.T) {
	dir := os.Getenv("SEU_LIVE_DIR")
	if dir == "" {
		t.Skip("set SEU_LIVE_DIR to the app data dir with cookies.json")
	}

	jar, err := session.LoadJar(filepath.Join(dir, "cookies.json"))
	if err != nil {
		t.Fatalf("load jar: %v", err)
	}
	client := session.NewClient(jar, 20*time.Second)

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	probe, _ := Check(ctx, client)
	t.Logf("verdict=%v reason=%s", probe.Verdict, probe.Reason)
	if err := os.WriteFile(filepath.Join(dir, "course.html"), []byte(probe.HTML), 0o644); err != nil {
		t.Logf("dump html: %v", err)
	}
	if probe.Verdict == VerdictCourses {
		res := Parse(probe.HTML, FilterOptions{})
		t.Logf("rows=%d", res.RowCount)
		for _, c := range res.Courses {
			t.Logf("  %-24s | %-8s | %-12s | full=%v expired=%v | id=%s/%s",
				c.Name, c.Category, c.Location, c.Full, c.Expired, c.ItemID, c.KaiKeID)
		}
	}
}
