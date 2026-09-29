package session

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/url"
	"os"
	"sort"
	"strings"
	"sync"
	"time"
)

// storedCookie retains session cookies (Expires zero) as well, which the
// standard net/http cookie jar drops. Persisting them is required: the site
// binds the login to a non-expiring session cookie.
type storedCookie struct {
	Name     string `json:"name"`
	Value    string `json:"value"`
	Domain   string `json:"domain"`
	Path     string `json:"path"`
	Expires  int64  `json:"expires"`
	Secure   bool   `json:"secure"`
	HTTPOnly bool   `json:"httpOnly"`
}

// Jar is a cookie jar that implements http.CookieJar and persists to disk.
type Jar struct {
	mu      sync.Mutex
	cookies []storedCookie
	path    string
}

// EmptyJar returns a non-persisting jar (Save writes nowhere).
func EmptyJar() *Jar { return &Jar{} }

func LoadJar(path string) (*Jar, error) {
	j := &Jar{path: path}
	raw, err := os.ReadFile(path)
	if err == nil {
		if err := json.Unmarshal(bytes.TrimPrefix(raw, []byte{0xEF, 0xBB, 0xBF}), &j.cookies); err != nil {
			return nil, err
		}
	} else if !os.IsNotExist(err) {
		return nil, err
	}
	j.purgeExpiredLocked()
	return j, nil
}

func (j *Jar) SetCookies(u *url.URL, cookies []*http.Cookie) {
	j.mu.Lock()
	defer j.mu.Unlock()
	for _, c := range cookies {
		domain := c.Domain
		if domain == "" {
			domain = u.Hostname()
		}
		domain = strings.TrimPrefix(domain, ".")
		path := c.Path
		if path == "" {
			path = "/"
		}
		if c.MaxAge < 0 {
			j.removeLocked(c.Name, domain, path)
			continue
		}
		var expires int64
		if !c.Expires.IsZero() {
			expires = c.Expires.Unix()
		}
		sc := storedCookie{
			Name:     c.Name,
			Value:    c.Value,
			Domain:   domain,
			Path:     path,
			Expires:  expires,
			Secure:   c.Secure,
			HTTPOnly: c.HttpOnly,
		}
		j.upsertLocked(sc)
	}
}

func (j *Jar) Cookies(u *url.URL) []*http.Cookie {
	j.mu.Lock()
	defer j.mu.Unlock()
	now := time.Now().Unix()
	var out []*http.Cookie
	for _, c := range j.cookies {
		if c.Expires != 0 && c.Expires < now {
			continue
		}
		if !domainMatch(u.Hostname(), c.Domain) || !pathMatch(u.Path, c.Path) {
			continue
		}
		out = append(out, &http.Cookie{Name: c.Name, Value: c.Value, Domain: c.Domain, Path: c.Path})
	}
	return out
}

func (j *Jar) Save() error {
	j.mu.Lock()
	defer j.mu.Unlock()
	if j.path == "" {
		return nil
	}
	j.purgeExpiredLocked()
	sort.Slice(j.cookies, func(a, b int) bool {
		if j.cookies[a].Domain != j.cookies[b].Domain {
			return j.cookies[a].Domain < j.cookies[b].Domain
		}
		return j.cookies[a].Name < j.cookies[b].Name
	})
	raw, err := json.MarshalIndent(j.cookies, "", "  ")
	if err != nil {
		return err
	}
	tmp := j.path + ".tmp"
	if err := os.WriteFile(tmp, append(raw, '\n'), 0o600); err != nil {
		return err
	}
	return os.Rename(tmp, j.path)
}

// All returns a copy of every cookie, including session cookies.
func (j *Jar) All() []*http.Cookie {
	j.mu.Lock()
	defer j.mu.Unlock()
	out := make([]*http.Cookie, 0, len(j.cookies))
	for _, c := range j.cookies {
		var expires time.Time
		if c.Expires != 0 {
			expires = time.Unix(c.Expires, 0)
		}
		out = append(out, &http.Cookie{
			Name: c.Name, Value: c.Value, Domain: c.Domain, Path: c.Path,
			Expires: expires, Secure: c.Secure, HttpOnly: c.HTTPOnly,
		})
	}
	return out
}

func (j *Jar) IsEmpty() bool {
	j.mu.Lock()
	defer j.mu.Unlock()
	return len(j.cookies) == 0
}

func (j *Jar) Clear() {
	j.mu.Lock()
	defer j.mu.Unlock()
	j.cookies = nil
}

func (j *Jar) upsertLocked(c storedCookie) {
	for i := range j.cookies {
		if j.cookies[i].Name == c.Name && j.cookies[i].Domain == c.Domain && j.cookies[i].Path == c.Path {
			j.cookies[i] = c
			return
		}
	}
	j.cookies = append(j.cookies, c)
}

func (j *Jar) removeLocked(name, domain, path string) {
	out := j.cookies[:0]
	for _, c := range j.cookies {
		if c.Name == name && c.Domain == domain && c.Path == path {
			continue
		}
		out = append(out, c)
	}
	j.cookies = out
}

func (j *Jar) purgeExpiredLocked() {
	now := time.Now().Unix()
	out := j.cookies[:0]
	for _, c := range j.cookies {
		if c.Expires != 0 && c.Expires < now {
			continue
		}
		out = append(out, c)
	}
	j.cookies = out
}

func domainMatch(host, domain string) bool {
	return host == domain || strings.HasSuffix(host, "."+domain)
}

func pathMatch(reqPath, cookiePath string) bool {
	if cookiePath == "" || cookiePath == "/" {
		return true
	}
	return strings.HasPrefix(reqPath, cookiePath)
}
