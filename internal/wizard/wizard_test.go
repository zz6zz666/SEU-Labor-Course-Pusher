package wizard

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"strings"
	"testing"
	"time"
)

func TestServerServesPageAndAPI(t *testing.T) {
	actions := Actions{
		Status:       func() Status { return Status{AuthState: "valid", Version: "test"} },
		Config:       func() ConfigView { return ConfigView{Username: "u"} },
		Verify:       func(context.Context) (string, string) { return "valid", "ok" },
		SaveAccount:  func(string, string) error { return nil },
		SaveFilters:  func([]string, []string) error { return nil },
		SaveNotify:   func(string, bool, bool) error { return nil },
		SetBehavior:  func(*bool, *bool) error { return nil },
		OpenLogin:    func() {},
		OpenCourse:   func() {},
		OpenConfig:   func() {},
		OpenLogs:     func() {},
		OpenExternal: func(string) error { return nil },
	}

	s, err := Open(actions, nil)
	if err != nil {
		t.Fatalf("open: %v", err)
	}
	defer s.Close()

	client := &http.Client{Timeout: 5 * time.Second}

	page := get(t, client, s.URL())
	if !strings.Contains(page, "window.seuWizard") {
		t.Error("shim not injected")
	}
	if !strings.Contains(page, "connect-src 'self'") {
		t.Error("CSP not patched for connect-src")
	}

	var status Status
	mustJSON(t, client, s.URL()+"api/status", &status)
	if status.AuthState != "valid" || status.Version != "test" {
		t.Errorf("status = %+v", status)
	}

	var verify struct {
		State  string `json:"state"`
		Reason string `json:"reason"`
	}
	mustJSON(t, client, s.URL()+"api/verify", &verify)
	if verify.State != "valid" {
		t.Errorf("verify = %+v", verify)
	}

	resp, err := client.Post(s.URL()+"api/save-account", "application/json", strings.NewReader(`{"username":"a","password":"b"}`))
	if err != nil {
		t.Fatalf("post: %v", err)
	}
	defer resp.Body.Close()
	var saved struct {
		OK bool `json:"ok"`
	}
	_ = json.NewDecoder(resp.Body).Decode(&saved)
	if !saved.OK {
		t.Error("save-account did not return ok")
	}
}

func get(t *testing.T, client *http.Client, url string) string {
	t.Helper()
	resp, err := client.Get(url)
	if err != nil {
		t.Fatalf("get %s: %v", url, err)
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	return string(body)
}

func mustJSON(t *testing.T, client *http.Client, url string, v any) {
	t.Helper()
	resp, err := client.Get(url)
	if err != nil {
		t.Fatalf("get %s: %v", url, err)
	}
	defer resp.Body.Close()
	if err := json.NewDecoder(resp.Body).Decode(v); err != nil {
		t.Fatalf("decode %s: %v", url, err)
	}
}
