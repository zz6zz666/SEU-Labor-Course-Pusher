// Package config defines the on-disk configuration schema and a store that
// loads, validates and persists it.
package config

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"sync"
)

var utf8BOM = []byte{0xEF, 0xBB, 0xBF}

type Credentials struct {
	Username string `json:"username"`
	Password string `json:"password"`
}

// Filters: Locations is a keyword whitelist (substring), Categories is a
// blacklist (exact match).
type Filters struct {
	Locations  []string `json:"locations"`
	Categories []string `json:"categories"`
}

type Schedule struct {
	RefreshIntervalMS     int     `json:"refreshIntervalMs"`
	JitterRatio           float64 `json:"jitterRatio"`
	DailySummaryHour      *int    `json:"dailySummaryHour"`
	FailureAlertThreshold int     `json:"failureAlertThreshold"`
	MaxBackoffMS          int     `json:"maxBackoffMs"`
}

type PushPlus struct {
	Enabled bool   `json:"enabled"`
	Token   string `json:"token"`
	Title   string `json:"title"`
}

type WindowsNotify struct {
	Enabled            bool `json:"enabled"`
	OpenBrowserOnClick bool `json:"openBrowserOnClick"`
}

type Push struct {
	PushPlus PushPlus      `json:"pushplus"`
	Windows  WindowsNotify `json:"windows"`
}

type Behavior struct {
	AutoLogin             bool `json:"autoLogin"`
	AutoOpenOnAuthFailure bool `json:"autoOpenOnAuthFailure"`
	AutoLaunchAtLogin     bool `json:"autoLaunchAtLogin"`
	AutoSelect            bool `json:"autoSelect"`
}

type Logging struct {
	Level         string `json:"level"`
	RetentionDays int    `json:"retentionDays"`
}

type Config struct {
	Credentials Credentials `json:"credentials"`
	Filters     Filters     `json:"filters"`
	Schedule    Schedule    `json:"schedule"`
	Push        Push        `json:"push"`
	Behavior    Behavior    `json:"behavior"`
	Logging     Logging     `json:"logging"`
}

func intPtr(v int) *int { return &v }

func Default() Config {
	return Config{
		Schedule: Schedule{
			RefreshIntervalMS:     180_000,
			JitterRatio:           0.1,
			DailySummaryHour:      intPtr(21),
			FailureAlertThreshold: 3,
			MaxBackoffMS:          30 * 60_000,
		},
		Push: Push{
			PushPlus: PushPlus{Enabled: true, Title: "劳动教育课程推送"},
			Windows:  WindowsNotify{Enabled: true, OpenBrowserOnClick: true},
		},
		Behavior: Behavior{AutoLogin: true, AutoOpenOnAuthFailure: true},
		Logging:  Logging{Level: "info", RetentionDays: 7},
	}
}

var validLevels = map[string]bool{"debug": true, "info": true, "warn": true, "error": true}

func (c *Config) Normalize() []string {
	var warnings []string
	if !validLevels[c.Logging.Level] {
		warnings = append(warnings, fmt.Sprintf("logging.level 非法(%q),已重置为 info", c.Logging.Level))
		c.Logging.Level = "info"
	}
	if c.Logging.RetentionDays < 1 {
		warnings = append(warnings, "logging.retentionDays 非法,已重置为 7")
		c.Logging.RetentionDays = 7
	}
	if c.Schedule.RefreshIntervalMS < 30_000 {
		warnings = append(warnings, "schedule.refreshIntervalMs 不得小于 30000,已重置为 180000")
		c.Schedule.RefreshIntervalMS = 180_000
	}
	if c.Schedule.JitterRatio < 0 || c.Schedule.JitterRatio > 0.5 {
		warnings = append(warnings, "schedule.jitterRatio 应在 0~0.5 之间,已重置为 0.1")
		c.Schedule.JitterRatio = 0.1
	}
	if c.Schedule.DailySummaryHour != nil && (*c.Schedule.DailySummaryHour < 0 || *c.Schedule.DailySummaryHour > 23) {
		warnings = append(warnings, "schedule.dailySummaryHour 应为 0~23 或 null,已重置为 21")
		c.Schedule.DailySummaryHour = intPtr(21)
	}
	if c.Filters.Locations == nil {
		c.Filters.Locations = []string{}
	}
	if c.Filters.Categories == nil {
		c.Filters.Categories = []string{}
	}
	return warnings
}

func (c Config) HasCredentials() bool {
	return c.Credentials.Username != "" && c.Credentials.Password != ""
}

// SafeSummary returns a credential-free view suitable for logging.
func (c Config) SafeSummary() map[string]any {
	return map[string]any{
		"usernameConfigured":      c.Credentials.Username != "",
		"locations":               c.Filters.Locations,
		"categories":              c.Filters.Categories,
		"refreshIntervalMs":       c.Schedule.RefreshIntervalMS,
		"dailySummaryHour":        c.Schedule.DailySummaryHour,
		"pushplusEnabled":         c.Push.PushPlus.Enabled,
		"pushplusTokenConfigured": c.Push.PushPlus.Token != "",
		"windowsNotifyEnabled":    c.Push.Windows.Enabled,
		"autoLogin":               c.Behavior.AutoLogin,
		"autoSelect":              c.Behavior.AutoSelect,
		"logLevel":                c.Logging.Level,
	}
}

// Store holds the active configuration and persists mutations to disk.
type Store struct {
	mu   sync.RWMutex
	cfg  Config
	path string
}

// Open loads config from path, writing defaults when the file does not exist.
// Validation warnings are returned for logging; they never abort startup.
func Open(path string) (*Store, []string, error) {
	if _, err := os.Stat(path); os.IsNotExist(err) {
		cfg := Default()
		if err := writeFile(path, cfg); err != nil {
			return nil, nil, err
		}
		return &Store{cfg: cfg, path: path}, []string{"未找到 config.json,已写入默认配置"}, nil
	}

	raw, err := os.ReadFile(path)
	if err != nil {
		return nil, nil, err
	}
	cfg := Default()
	// Strip a UTF-8 BOM: Windows editors such as Notepad add one.
	if err := json.Unmarshal(bytes.TrimPrefix(raw, utf8BOM), &cfg); err != nil {
		return nil, nil, fmt.Errorf("config.json 不是合法 JSON: %w", err)
	}
	warnings := cfg.Normalize()
	return &Store{cfg: cfg, path: path}, warnings, nil
}

func (s *Store) Get() Config {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return s.cfg
}

// Update applies fn to a copy of the config and persists it.
func (s *Store) Update(fn func(*Config)) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	next := s.cfg
	fn(&next)
	next.Normalize()
	if err := writeFile(s.path, next); err != nil {
		return err
	}
	s.cfg = next
	return nil
}

// Reload re-reads the config file and reports whether it changed.
func (s *Store) Reload() (changed bool, warnings []string, err error) {
	raw, err := os.ReadFile(s.path)
	if err != nil {
		return false, nil, err
	}
	next := Default()
	if err := json.Unmarshal(bytes.TrimPrefix(raw, utf8BOM), &next); err != nil {
		return false, nil, err
	}
	warnings = next.Normalize()

	s.mu.Lock()
	defer s.mu.Unlock()
	before, _ := json.Marshal(s.cfg)
	after, _ := json.Marshal(next)
	if bytes.Equal(before, after) {
		return false, nil, nil
	}
	s.cfg = next
	return true, warnings, nil
}

func writeFile(path string, cfg Config) error {
	raw, err := json.MarshalIndent(cfg, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(path, append(raw, '\n'), 0o600)
}
