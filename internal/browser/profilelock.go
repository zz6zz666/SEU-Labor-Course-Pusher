package browser

import "sync"

var profileLocks = struct {
	sync.Mutex
	m map[string]*sync.Mutex
}{m: map[string]*sync.Mutex{}}

func profileMutex(dir string) *sync.Mutex {
	profileLocks.Lock()
	defer profileLocks.Unlock()
	m := profileLocks.m[dir]
	if m == nil {
		m = &sync.Mutex{}
		profileLocks.m[dir] = m
	}
	return m
}

// acquireProfile locks the profile dir for the lifetime of a browser, so two
// windows never share one profile concurrently. It returns the release func.
func acquireProfile(dir string) func() {
	if dir == "" {
		return func() {}
	}
	m := profileMutex(dir)
	m.Lock()
	return m.Unlock
}
