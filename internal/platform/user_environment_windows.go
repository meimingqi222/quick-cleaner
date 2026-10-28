package platform

import (
	"os"
	"os/user"
	"path/filepath"
	"sync"
	"syscall"
)

var environmentOnce sync.Once
var environmentHome, environmentSID string
var environmentHomeOK, environmentSIDOK bool

func loadUserEnvironment() {
	environmentOnce.Do(func() {
		u := parseOriginalUser(os.Args)
		environmentHome, environmentHomeOK = originalHome(u, func() (string, bool) {
			p, err := os.UserHomeDir()
			if err == nil && p != "" {
				return p, true
			}
			account, err := user.Current()
			if err != nil || account.HomeDir == "" {
				return "", false
			}
			return account.HomeDir, true
		})
		if u.hasSID {
			environmentSID, environmentSIDOK = u.sid, true
			return
		}
		// Query the process token, not a username lookup (which can identify a
		// different account under cross-account elevation).
		var token syscall.Token
		process, err := syscall.GetCurrentProcess()
		if err != nil {
			return
		}
		if syscall.OpenProcessToken(process, syscall.TOKEN_QUERY, &token) != nil {
			return
		}
		defer token.Close()
		user, err := token.GetTokenUser()
		if err != nil {
			return
		}
		environmentSID, err = user.User.Sid.String()
		environmentSIDOK = err == nil
	})
}

// UserHome is unavailable when an explicitly supplied original home no longer
// exists. Falling back to the elevated account would target the wrong user.
func UserHome() (string, bool) { loadUserEnvironment(); return environmentHome, environmentHomeOK }

func userSubdirectory(parts ...string) (string, bool) {
	home, ok := UserHome()
	if !ok {
		return "", false
	}
	return filepath.Join(append([]string{home}, parts...)...), true
}

// UserCacheDir derives Local AppData from the trusted home, never APPDATA.
func UserCacheDir() (string, bool) { return userSubdirectory("AppData", "Local") }

// UserDataDir derives Roaming AppData from the trusted home.
func UserDataDir() (string, bool) { return userSubdirectory("AppData", "Roaming") }

// UserTempDir returns the expected user temp path even if it does not exist.
func UserTempDir() (string, bool) { return userSubdirectory("AppData", "Local", "Temp") }

// UserSID returns the supplied original SID verbatim, or the process token SID.
func UserSID() (string, bool) { loadUserEnvironment(); return environmentSID, environmentSIDOK }

var languageKernel = syscall.NewLazyDLL("kernel32.dll")
var userUILanguage = languageKernel.NewProc("GetUserDefaultUILanguage")
var systemUILanguage = languageKernel.NewProc("GetSystemDefaultUILanguage")

func uiLanguageTag(user, system func() uintptr) string {
	id := user()
	if id == 0 {
		id = system()
	}
	if id&0x3ff == 0x04 {
		return "zh"
	}
	return "en"
}

// SystemLanguageTag uses display language, not regional-format or locale guesses.
func SystemLanguageTag() string {
	return uiLanguageTag(func() uintptr { id, _, _ := userUILanguage.Call(); return id },
		func() uintptr { id, _, _ := systemUILanguage.Call(); return id })
}
