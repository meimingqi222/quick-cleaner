// Linux support is only for non-destructive migration tests and the headless
// gallery. These paths do not constitute a production Linux cleaner adapter.
package platform

import "os"

func UserCacheDir() (string, bool) {
	p, err := os.UserCacheDir()
	return p, err == nil && p != ""
}

func UserDataDir() (string, bool) {
	if p := os.Getenv("XDG_DATA_HOME"); p != "" {
		return p, true
	}
	home, ok := UserHome()
	if !ok {
		return "", false
	}
	return home + "/.local/share", true
}
