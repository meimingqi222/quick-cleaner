//go:build darwin || linux

package platform

import (
	"os"
	"os/user"
)

// SystemLanguageTag returns the first nonempty POSIX locale, without normalization.
func SystemLanguageTag() string {
	for _, key := range []string{"LC_ALL", "LC_MESSAGES", "LANG"} {
		if value := os.Getenv(key); value != "" {
			return value
		}
	}
	return ""
}

// UserHome returns the current user's home, not an inferred cleanup target.
func UserHome() (string, bool) {
	home, err := os.UserHomeDir()
	if err == nil && home != "" {
		return home, true
	}
	// dirs::home_dir falls back to the account database when HOME is absent.
	account, err := user.Current()
	if err != nil || account.HomeDir == "" {
		return "", false
	}
	return account.HomeDir, true
}

// UserTempDir preserves the platform temporary-directory convention.
func UserTempDir() (string, bool) { return os.TempDir(), true }

// UserSID is unavailable on POSIX systems.
func UserSID() (string, bool) { return "", false }
