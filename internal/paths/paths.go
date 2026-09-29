// Package paths resolves runtime locations for data, config, logs and snapshots.
//
// Defaults target the per-user config directory; SEU_DAEMON_DATA_DIR and
// SEU_DAEMON_CONFIG override them (useful for development).
package paths

import (
	"os"
	"path/filepath"
)

const appDirName = "SEU劳动教育课程推送助手"

type Paths struct {
	DataDir     string
	ConfigPath  string
	LogDir      string
	SnapshotDir string
	StatePath   string
	CookiesPath string
}

func Resolve() (Paths, error) {
	dataDir := os.Getenv("SEU_DAEMON_DATA_DIR")
	if dataDir == "" {
		base, err := os.UserConfigDir()
		if err != nil {
			return Paths{}, err
		}
		dataDir = filepath.Join(base, appDirName)
	}

	configPath := os.Getenv("SEU_DAEMON_CONFIG")
	if configPath == "" {
		configPath = filepath.Join(dataDir, "config.json")
	}

	p := Paths{
		DataDir:     dataDir,
		ConfigPath:  configPath,
		LogDir:      filepath.Join(dataDir, "logs"),
		SnapshotDir: filepath.Join(dataDir, "snapshots"),
		StatePath:   filepath.Join(dataDir, "state.json"),
		CookiesPath: filepath.Join(dataDir, "cookies.json"),
	}
	for _, dir := range []string{p.DataDir, p.LogDir, p.SnapshotDir} {
		if err := os.MkdirAll(dir, 0o700); err != nil {
			return Paths{}, err
		}
	}
	return p, nil
}
