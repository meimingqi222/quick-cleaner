package platform

import (
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"
)

// DownloadsDir reads XDG's registered download directory without executing shell
// text. Like dirs::download_dir, an unconfigured folder is unknown on Linux.
func DownloadsDir() (string, bool) {
	home, ok := UserHome()
	if !ok || !filepath.IsAbs(home) {
		return "", false
	}
	config := os.Getenv("XDG_CONFIG_HOME")
	if !filepath.IsAbs(config) {
		config = filepath.Join(home, ".config")
	}
	f, err := os.Open(filepath.Join(config, "user-dirs.dirs"))
	if err != nil {
		return "", false
	}
	defer f.Close()
	data, err := io.ReadAll(io.LimitReader(f, 65537))
	if err != nil || len(data) > 65536 {
		return "", false
	}
	for line := range strings.SplitSeq(string(data), "\n") {
		key, value, found := strings.Cut(strings.TrimSpace(line), "=")
		if !found || strings.TrimSpace(key) != "XDG_DOWNLOAD_DIR" {
			continue
		}
		path, err := strconv.Unquote(strings.TrimSpace(value))
		if err != nil {
			return "", false
		}
		if strings.HasPrefix(path, "$HOME/") {
			path = filepath.Join(home, strings.TrimPrefix(path, "$HOME/"))
		}
		if filepath.IsAbs(path) && !strings.ContainsAny(path, "$`\x00") {
			return filepath.Clean(path), true
		}
		return "", false
	}
	return "", false
}
