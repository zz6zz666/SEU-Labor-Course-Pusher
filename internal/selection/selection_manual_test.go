package selection

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/session"
)

func manualRunner(t *testing.T) (*Runner, context.Context, context.CancelFunc) {
	t.Helper()
	dir := os.Getenv("SEU_SELECT_TEST_DIR")
	if dir == "" {
		t.Skip("set SEU_SELECT_TEST_DIR to the app data dir with cookies.json")
	}
	jar, err := session.LoadJar(filepath.Join(dir, "cookies.json"))
	if err != nil {
		t.Fatalf("load jar: %v", err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	return &Runner{Client: session.NewClient(jar, 30*time.Second)}, ctx, cancel
}

// TestManualSelectNotFound checks the notFound path with a bogus target, so
// nothing is actually selected.
//
//	$env:SEU_SELECT_TEST_DIR="C:\...\data"
//	go test ./internal/selection -run TestManualSelectNotFound -v
func TestManualSelectNotFound(t *testing.T) {
	r, ctx, cancel := manualRunner(t)
	defer cancel()

	out, err := r.Select(ctx, []Target{{
		UniqueID: "selftest|nope",
		Name:     "__no_such_course__",
		ItemID:   "0",
		KaiKeID:  "0",
	}})
	if err != nil {
		t.Fatalf("select: %v", err)
	}
	t.Logf("outcome: status=%s message=%q", out[0].Status, out[0].Message)
	if out[0].Status != NotFound {
		t.Fatalf("status = %s, want notFound", out[0].Status)
	}
}

// TestManualSelectCancelRoundTrip really selects then cancels one course,
// leaving no lasting change. Requires SEU_SELECT_ITEM and SEU_SELECT_KAIKE.
//
//	$env:SEU_SELECT_TEST_DIR="C:\...\data"
//	$env:SEU_SELECT_ITEM="..."; $env:SEU_SELECT_KAIKE="..."
//	go test ./internal/selection -run TestManualSelectCancelRoundTrip -v
func TestManualSelectCancelRoundTrip(t *testing.T) {
	item := os.Getenv("SEU_SELECT_ITEM")
	kai := os.Getenv("SEU_SELECT_KAIKE")
	if item == "" || kai == "" {
		t.Skip("set SEU_SELECT_ITEM and SEU_SELECT_KAIKE to run the real round-trip")
	}
	r, ctx, cancel := manualRunner(t)
	defer cancel()

	target := Target{UniqueID: "selftest|" + kai, Name: "__selftest__", ItemID: item, KaiKeID: kai}

	sel, err := r.Select(ctx, []Target{target})
	if err != nil {
		t.Fatalf("select: %v", err)
	}
	t.Logf("select: status=%s message=%q", sel[0].Status, sel[0].Message)
	if sel[0].Status != Selected {
		t.Fatalf("select status = %s, want selected", sel[0].Status)
	}

	cxl, err := r.Cancel(ctx, []Target{target})
	if err != nil {
		t.Fatalf("cancel: %v", err)
	}
	t.Logf("cancel: status=%s message=%q", cxl[0].Status, cxl[0].Message)
	if cxl[0].Status != Cancelled {
		t.Fatalf("cancel status = %s, want cancelled (selection may still exist!)", cxl[0].Status)
	}
}
