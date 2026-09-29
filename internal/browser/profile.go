package browser

import (
	"encoding/json"
	"os"
	"path/filepath"
)

// profileName is the display name given to our dedicated Edge/Chrome profile,
// so the browser does not label it as "unSpecified".
const profileName = "SEU劳动教育助手"

// prepareProfile ensures the user-data directory exists and carries a display
// name. Chromium stores profile names in Local State, which it preserves on
// later runs.
func prepareProfile(dir string) error {
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return err
	}

	path := filepath.Join(dir, "Local State")
	var root map[string]any
	switch raw, err := os.ReadFile(path); {
	case err == nil:
		_ = json.Unmarshal(raw, &root)
	case os.IsNotExist(err):
	default:
		return err
	}
	if root == nil {
		root = map[string]any{}
	}

	profile := obj(root, "profile")
	cache := obj(profile, "info_cache")
	def := obj(cache, "Default")
	if name, _ := def["name"].(string); name == "" {
		def["name"] = profileName
	}
	if _, ok := profile["last_used"]; !ok {
		profile["last_used"] = "Default"
	}

	out, err := json.Marshal(root)
	if err != nil {
		return err
	}
	tmp := path + ".tmp"
	if err := os.WriteFile(tmp, out, 0o600); err != nil {
		return err
	}
	return os.Rename(tmp, path)
}

func obj(parent map[string]any, key string) map[string]any {
	if child, ok := parent[key].(map[string]any); ok {
		return child
	}
	child := map[string]any{}
	parent[key] = child
	return child
}
