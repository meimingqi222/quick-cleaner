// Package platform supplies operating-system adapters for QuickCleaner.
package platform

import "os"

type originalUser struct {
	home, sid       string
	hasHome, hasSID bool
}

// Match the Rust argv walk exactly: last complete occurrence wins, a flag
// consumes even another flag as its value, and equals syntax is not recognized.
func parseOriginalUser(args []string) (u originalUser) {
	for i := 1; i < len(args); i++ {
		if i+1 >= len(args) {
			continue
		}
		switch args[i] {
		case "--orig-user-home":
			u.home, u.hasHome = args[i+1], true
			i++
		case "--orig-user-sid":
			u.sid, u.hasSID = args[i+1], true
			i++
		}
	}
	return
}

func originalHome(u originalUser, fallback func() (string, bool)) (string, bool) {
	if !u.hasHome {
		return fallback()
	}
	// Rust's Path::exists accepts files too. Do not tighten or broaden this
	// contract, and never substitute the administrator's profile on failure.
	if _, err := os.Stat(u.home); err != nil {
		return "", false
	}
	return u.home, true
}
