// Package logging writes leveled, daily-rotated log files.
//
// Every message passes through redact() so credentials and tokens can never
// reach the log even if a caller forgets.
package logging

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"sync"
	"time"
)

type Level int

const (
	Debug Level = iota
	Info
	Warn
	Error
)

func ParseLevel(s string) Level {
	switch s {
	case "debug":
		return Debug
	case "warn":
		return Warn
	case "error":
		return Error
	default:
		return Info
	}
}

type Logger struct{ tag string }

var (
	mu            sync.Mutex
	minLevel      = Info
	logDir        string
	retentionDays = 7
	currentDate   string
	currentPath   string
)

func Init(dir string, level Level, retention int) {
	mu.Lock()
	defer mu.Unlock()
	logDir = dir
	minLevel = level
	retentionDays = retention
	cleanupLocked()
}

// Configure applies a hot-reloaded level and retention.
func Configure(level Level, retention int) {
	mu.Lock()
	defer mu.Unlock()
	minLevel = level
	retentionDays = retention
	cleanupLocked()
}

func New(tag string) *Logger { return &Logger{tag: tag} }

func (l *Logger) Debug(args ...any) { write(Debug, l.tag, args) }
func (l *Logger) Info(args ...any)  { write(Info, l.tag, args) }
func (l *Logger) Warn(args ...any)  { write(Warn, l.tag, args) }
func (l *Logger) Error(args ...any) { write(Error, l.tag, args) }

var (
	redactKV  = regexp.MustCompile(`(?i)("?(?:password|pwd|token)"?\s*[:=]\s*")[^"]*(")`)
	redactVar = regexp.MustCompile(`(?i)(Password|Token|__RequestVerificationToken)([=:]\s*)[^\s&,"}]+`)
)

func redact(s string) string {
	s = redactKV.ReplaceAllString(s, "$1***$2")
	return redactVar.ReplaceAllString(s, "$1$2***")
}

func write(level Level, tag string, args []any) {
	if level < minLevel {
		return
	}
	msg := redact(format(args))
	line := fmt.Sprintf("[%s] [%s] [%s] %s\n", time.Now().Format(time.RFC3339), level, tag, msg)

	mu.Lock()
	defer mu.Unlock()
	ensureFileLocked()
	if currentPath != "" {
		if f, err := os.OpenFile(currentPath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o600); err == nil {
			_, _ = f.WriteString(line)
			_ = f.Close()
		}
	}
	os.Stderr.WriteString(line)
}

func (l Level) String() string {
	switch l {
	case Debug:
		return "DEBUG"
	case Warn:
		return "WARN"
	case Error:
		return "ERROR"
	default:
		return "INFO"
	}
}

func format(args []any) string {
	parts := make([]string, len(args))
	for i, a := range args {
		parts[i] = fmt.Sprint(a)
	}
	return strings.Join(parts, " ")
}

func ensureFileLocked() {
	date := time.Now().Format("2006-01-02")
	if date == currentDate && currentPath != "" {
		return
	}
	currentDate = date
	currentPath = filepath.Join(logDir, "daemon-"+date+".log")
	cleanupLocked()
}

func cleanupLocked() {
	entries, err := os.ReadDir(logDir)
	if err != nil {
		return
	}
	cutoff := time.Now().AddDate(0, 0, -retentionDays)
	for _, e := range entries {
		name := e.Name()
		if !strings.HasPrefix(name, "daemon-") || !strings.HasSuffix(name, ".log") {
			continue
		}
		if info, err := e.Info(); err == nil && info.ModTime().Before(cutoff) {
			_ = os.Remove(filepath.Join(logDir, name))
		}
	}
}
