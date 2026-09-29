// Package state persists long-lived runtime state (pushed ids, auth state,
// counters). Writes are atomic via a temp file + rename.
package state

import (
	"bytes"
	"encoding/json"
	"os"
	"sync"
	"time"
)

var utf8BOM = []byte{0xEF, 0xBB, 0xBF}

func now() string { return time.Now().UTC().Format(time.RFC3339) }

type AuthState string

const (
	AuthUnknown AuthState = "unknown"
	AuthValid   AuthState = "valid"
	AuthExpired AuthState = "expired"
)

type State struct {
	PushedUniqueIDs     []string  `json:"pushedUniqueIds"`
	AutoHandledIDs      []string  `json:"autoHandledIds"`
	AuthState           AuthState `json:"authState"`
	FirstRunCompleted   bool      `json:"firstRunCompleted"`
	LastRunAt           string    `json:"lastRunAt"`
	LastSuccessAt       string    `json:"lastSuccessAt"`
	LastAuthFailureAt   string    `json:"lastAuthFailureAt"`
	ConsecutiveFailures int       `json:"consecutiveFailures"`
	LastSummaryDate     string    `json:"lastSummaryDate"`
}

type Store struct {
	mu   sync.Mutex
	s    State
	path string
}

func Open(path string) (*Store, error) {
	s := State{
		PushedUniqueIDs: []string{},
		AutoHandledIDs:  []string{},
		AuthState:       AuthUnknown,
	}
	raw, err := os.ReadFile(path)
	if err == nil {
		if err := json.Unmarshal(bytes.TrimPrefix(raw, utf8BOM), &s); err != nil {
			return nil, err
		}
	} else if !os.IsNotExist(err) {
		return nil, err
	}
	if s.PushedUniqueIDs == nil {
		s.PushedUniqueIDs = []string{}
	}
	if s.AutoHandledIDs == nil {
		s.AutoHandledIDs = []string{}
	}
	st := &Store{s: s, path: path}
	return st, st.saveLocked()
}

func (st *Store) Get() State {
	st.mu.Lock()
	defer st.mu.Unlock()
	return st.s
}

// Update applies fn and persists the result.
func (st *Store) Update(fn func(*State)) error {
	st.mu.Lock()
	defer st.mu.Unlock()
	fn(&st.s)
	return st.saveLocked()
}

func (st *Store) AddPushed(ids []string) error {
	return st.Update(func(s *State) { s.PushedUniqueIDs = union(s.PushedUniqueIDs, ids) })
}

func (st *Store) SetPushed(ids []string) error {
	return st.Update(func(s *State) {
		if !equal(s.PushedUniqueIDs, ids) {
			s.PushedUniqueIDs = ids
		}
	})
}

func (st *Store) AddAutoHandled(ids []string) error {
	return st.Update(func(s *State) { s.AutoHandledIDs = union(s.AutoHandledIDs, ids) })
}

func (st *Store) SetAutoHandled(ids []string) error {
	return st.Update(func(s *State) {
		if !equal(s.AutoHandledIDs, ids) {
			s.AutoHandledIDs = ids
		}
	})
}

func (st *Store) SetAuthState(a AuthState) error {
	return st.Update(func(s *State) {
		s.AuthState = a
		if a == AuthExpired {
			s.LastAuthFailureAt = now()
		}
	})
}

func (st *Store) saveLocked() error {
	raw, err := json.MarshalIndent(st.s, "", "  ")
	if err != nil {
		return err
	}
	tmp := st.path + ".tmp"
	if err := os.WriteFile(tmp, append(raw, '\n'), 0o600); err != nil {
		return err
	}
	return os.Rename(tmp, st.path)
}

func union(a, b []string) []string {
	set := make(map[string]struct{}, len(a)+len(b))
	for _, v := range a {
		set[v] = struct{}{}
	}
	for _, v := range b {
		set[v] = struct{}{}
	}
	out := make([]string, 0, len(set))
	for v := range set {
		out = append(out, v)
	}
	return out
}

func equal(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}
