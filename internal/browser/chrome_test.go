package browser

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestPrepareProfileNamesDefault(t *testing.T) {
	dir := t.TempDir()
	if err := prepareProfile(dir); err != nil {
		t.Fatalf("prepareProfile: %v", err)
	}
	if got := profileDisplayName(t, dir); got != profileName {
		t.Fatalf("name = %q, want %q", got, profileName)
	}
	// Idempotent: a second call must keep the existing name.
	if err := prepareProfile(dir); err != nil {
		t.Fatalf("prepareProfile (second): %v", err)
	}
	if got := profileDisplayName(t, dir); got != profileName {
		t.Fatalf("name after second call = %q", got)
	}
}

func TestChromeLaunchEvalCookies(t *testing.T) {
	if _, err := FindExecPath(); err != nil {
		t.Skipf("no system browser: %v", err)
	}

	dir := t.TempDir()
	parent, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	defer cancel()

	b, err := Launch(parent, LaunchOptions{Visible: false, ProfileDir: dir})
	if err != nil {
		t.Fatalf("launch: %v", err)
	}
	defer b.Close()

	if err := b.Navigate("data:text/html,<title>t</title><h1 id=x>hi</h1>"); err != nil {
		t.Fatalf("navigate: %v", err)
	}

	raw, err := b.Eval(`document.getElementById('x').textContent`)
	if err != nil {
		t.Fatalf("eval: %v", err)
	}
	var got string
	if err := json.Unmarshal(raw, &got); err != nil {
		t.Fatalf("unmarshal %s: %v", raw, err)
	}
	if got != "hi" {
		t.Fatalf("eval returned %q, want %q", got, "hi")
	}

	if _, err := b.AllCookies(); err != nil {
		t.Fatalf("cookies: %v", err)
	}

	if err := b.Close(); err != nil {
		t.Fatalf("close: %v", err)
	}
	if got := profileDisplayName(t, dir); got != profileName {
		t.Errorf("profile name lost after Edge ran: %q, want %q", got, profileName)
	}
}

func profileDisplayName(t *testing.T, dir string) string {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join(dir, "Local State"))
	if err != nil {
		t.Fatalf("read Local State: %v", err)
	}
	var root struct {
		Profile struct {
			InfoCache map[string]struct {
				Name string `json:"name"`
			} `json:"info_cache"`
		} `json:"profile"`
	}
	if err := json.Unmarshal(raw, &root); err != nil {
		t.Fatalf("parse Local State: %v", err)
	}
	return root.Profile.InfoCache["Default"].Name
}
