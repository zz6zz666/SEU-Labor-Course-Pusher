// Package wizard serves the settings page over loopback HTTP. A native WebView2
// window (see internal/appwindow) renders the page, and its window.seuWizard API
// is backed by /api/* endpoints, so the original wizard.html is reused unchanged.
package wizard

import (
	"context"
	"encoding/json"
	"fmt"
	"net"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/logging"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/web"
)

type Status struct {
	FirstRunCompleted     bool   `json:"firstRunCompleted"`
	CredentialsConfigured bool   `json:"credentialsConfigured"`
	PushplusConfigured    bool   `json:"pushplusConfigured"`
	WindowsNotifyEnabled  bool   `json:"windowsNotifyEnabled"`
	AuthState             string `json:"authState"`
	WatcherMessage        string `json:"watcherMessage"`
	LastSuccessAt         string `json:"lastSuccessAt"`
	AutoStartEnabled      bool   `json:"autoStartEnabled"`
	AutoSelectEnabled     bool   `json:"autoSelectEnabled"`
	FiltersConfigured     bool   `json:"filtersConfigured"`
	ConfigPath            string `json:"configPath"`
	DataDir               string `json:"dataDir"`
	Version               string `json:"version"`
}

type ConfigView struct {
	Username             string   `json:"username"`
	Password             string   `json:"password"`
	PushplusToken        string   `json:"pushplusToken"`
	PushplusEnabled      bool     `json:"pushplusEnabled"`
	WindowsNotifyEnabled bool     `json:"windowsNotifyEnabled"`
	AutoLaunchAtLogin    bool     `json:"autoLaunchAtLogin"`
	AutoSelect           bool     `json:"autoSelect"`
	Locations            []string `json:"locations"`
	Categories           []string `json:"categories"`
}

type Actions struct {
	Status       func() Status
	Config       func() ConfigView
	Verify       func(context.Context) (authState, reason string)
	SaveAccount  func(username, password string) error
	SaveFilters  func(locations, categories []string) error
	SaveNotify   func(token string, pushplusEnabled, windowsEnabled bool) error
	SetBehavior  func(autoStart, autoSelect *bool) error
	OpenLogin    func()
	OpenCourse   func()
	OpenConfig   func()
	OpenLogs     func()
	OpenExternal func(url string) error
}

type Server struct {
	actions Actions
	log     *logging.Logger
	ln      net.Listener
	http    *http.Server
	onClose func()
	mu      sync.Mutex
}

// Open starts the loopback settings API. It does not open any window itself:
// the native WebView2 window consumes these endpoints. Serving the legacy HTML
// page is still available as a debugging fallback.
func Open(actions Actions, log *logging.Logger) (*Server, error) {
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return nil, err
	}
	s := &Server{actions: actions, log: log, ln: ln}
	s.http = &http.Server{Handler: s.routes()}
	go func() { _ = s.http.Serve(ln) }()
	return s, nil
}

func (s *Server) URL() string { return fmt.Sprintf("http://%s/", s.ln.Addr().String()) }

// SetOnClose registers a callback fired exactly once when the server closes.
// The native window uses it to close itself when the page requests shutdown.
func (s *Server) SetOnClose(fn func()) {
	s.mu.Lock()
	s.onClose = fn
	s.mu.Unlock()
}

func (s *Server) Close() error {
	s.mu.Lock()
	httpSrv := s.http
	s.http = nil
	onClose := s.onClose
	s.onClose = nil
	s.mu.Unlock()

	if httpSrv != nil {
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		_ = httpSrv.Shutdown(ctx)
	}
	if onClose != nil {
		onClose()
	}
	return nil
}

func (s *Server) routes() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("/", s.handlePage)
	mux.HandleFunc("/api/status", s.handleStatus)
	mux.HandleFunc("/api/config", s.handleConfig)
	mux.HandleFunc("/api/verify", s.handleVerify)
	mux.HandleFunc("/api/save-account", s.handleSaveAccount)
	mux.HandleFunc("/api/save-filters", s.handleSaveFilters)
	mux.HandleFunc("/api/save-notify", s.handleSaveNotify)
	mux.HandleFunc("/api/set-behavior", s.handleSetBehavior)
	mux.HandleFunc("/api/open-login", s.handleSimple(s.actions.OpenLogin))
	mux.HandleFunc("/api/open-course", s.handleSimple(s.actions.OpenCourse))
	mux.HandleFunc("/api/open-config", s.handleSimple(s.actions.OpenConfig))
	mux.HandleFunc("/api/open-logs", s.handleSimple(s.actions.OpenLogs))
	mux.HandleFunc("/api/open-external", s.handleOpenExternal)
	mux.HandleFunc("/api/close", s.handleClose)
	return mux
}

func (s *Server) handlePage(w http.ResponseWriter, r *http.Request) {
	if r.URL.Path != "/" {
		http.NotFound(w, r)
		return
	}
	page := strings.Replace(web.WizardHTML, cspOriginal, cspPatched, 1)
	page = strings.Replace(page, "<head>", "<head>\n"+shim, 1)
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	_, _ = w.Write([]byte(page))
}

func (s *Server) handleStatus(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, s.actions.Status())
}

func (s *Server) handleConfig(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, s.actions.Config())
}

func (s *Server) handleVerify(w http.ResponseWriter, r *http.Request) {
	authState, reason := s.actions.Verify(r.Context())
	writeJSON(w, map[string]string{"state": authState, "reason": reason})
}

func (s *Server) handleSaveAccount(w http.ResponseWriter, r *http.Request) {
	var body struct {
		Username string `json:"username"`
		Password string `json:"password"`
	}
	if !decode(w, r, &body) {
		return
	}
	s.result(w, s.actions.SaveAccount(body.Username, body.Password))
}

func (s *Server) handleSaveFilters(w http.ResponseWriter, r *http.Request) {
	var body struct {
		Locations  []string `json:"locations"`
		Categories []string `json:"categories"`
	}
	if !decode(w, r, &body) {
		return
	}
	s.result(w, s.actions.SaveFilters(body.Locations, body.Categories))
}

func (s *Server) handleSaveNotify(w http.ResponseWriter, r *http.Request) {
	var body struct {
		PushplusToken        string `json:"pushplusToken"`
		PushplusEnabled      bool   `json:"pushplusEnabled"`
		WindowsNotifyEnabled bool   `json:"windowsNotifyEnabled"`
	}
	if !decode(w, r, &body) {
		return
	}
	s.result(w, s.actions.SaveNotify(body.PushplusToken, body.PushplusEnabled, body.WindowsNotifyEnabled))
}

func (s *Server) handleSetBehavior(w http.ResponseWriter, r *http.Request) {
	var body struct {
		AutoStart  *bool `json:"autoStart"`
		AutoSelect *bool `json:"autoSelect"`
	}
	if !decode(w, r, &body) {
		return
	}
	s.result(w, s.actions.SetBehavior(body.AutoStart, body.AutoSelect))
}

func (s *Server) handleOpenExternal(w http.ResponseWriter, r *http.Request) {
	var body struct {
		URL string `json:"url"`
	}
	if !decode(w, r, &body) {
		return
	}
	s.result(w, s.actions.OpenExternal(body.URL))
}

func (s *Server) handleClose(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, map[string]bool{"ok": true})
	go func() {
		time.Sleep(150 * time.Millisecond)
		_ = s.Close()
	}()
}

func (s *Server) handleSimple(fn func()) http.HandlerFunc {
	return func(w http.ResponseWriter, _ *http.Request) {
		if fn != nil {
			fn()
		}
		writeJSON(w, map[string]bool{"ok": true})
	}
}

func (s *Server) result(w http.ResponseWriter, err error) {
	if err != nil {
		w.WriteHeader(http.StatusBadRequest)
		writeJSON(w, map[string]any{"ok": false, "error": err.Error()})
		return
	}
	writeJSON(w, map[string]bool{"ok": true})
}

func decode(w http.ResponseWriter, r *http.Request, v any) bool {
	if err := json.NewDecoder(r.Body).Decode(v); err != nil {
		w.WriteHeader(http.StatusBadRequest)
		writeJSON(w, map[string]any{"ok": false, "error": "请求格式错误"})
		return false
	}
	return true
}

func writeJSON(w http.ResponseWriter, v any) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	_ = json.NewEncoder(w).Encode(v)
}

const cspOriginal = "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline';"
const cspPatched = "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'; img-src 'self' data:;"

const shim = `<script>
(function () {
  async function call(path, body) {
    const opt = { method: body === undefined ? 'GET' : 'POST', headers: {} };
    if (body !== undefined) { opt.headers['Content-Type'] = 'application/json'; opt.body = JSON.stringify(body); }
    const res = await fetch(path, opt);
    const text = await res.text();
    let data = null;
    try { data = text ? JSON.parse(text) : null; } catch (e) { data = null; }
    if (!res.ok) { throw new Error((data && data.error) || ('HTTP ' + res.status)); }
    return data;
  }
  window.seuWizard = {
    verify: () => call('/api/verify', {}),
    getStatus: () => call('/api/status'),
    getConfig: () => call('/api/config'),
    saveAccount: (d) => call('/api/save-account', d),
    saveFilters: (d) => call('/api/save-filters', d),
    saveNotify: (d) => call('/api/save-notify', d),
    setBehavior: (d) => call('/api/set-behavior', d),
    openLogin: () => call('/api/open-login', {}),
    openCourse: () => call('/api/open-course', {}),
    openConfig: () => call('/api/open-config', {}),
    openLogs: () => call('/api/open-logs', {}),
    openExternal: (url) => call('/api/open-external', { url: url }),
    close: () => call('/api/close', {})
  };
})();
</script>
`
