package platform

import "path/filepath"

// DownloadsDir follows the macOS user-directory convention.
func DownloadsDir() (string, bool) {
	home, ok := UserHome()
	if !ok || !filepath.IsAbs(home) {
		return "", false
	}
	return filepath.Join(home, "Downloads"), true
}
