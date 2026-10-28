package core

import (
	"github.com/meimingqi222/quick-cleaner/internal/platform"
	"path/filepath"
	"strings"
	"sync"
)

type whitelistEntry struct{ norm, display string }

var whitelist struct {
	sync.RWMutex
	entries []whitelistEntry
}

func expandWhitelistHome(s string) (string, bool) {
	if s == "~" || strings.HasPrefix(s, "~/") {
		h, ok := platform.UserHome()
		if !ok {
			return "", false
		}
		if s == "~" {
			return h, true
		}
		rest := s[2:]
		if filepath.IsAbs(rest) {
			return rest, true
		}
		if !strings.HasSuffix(h, string(filepath.Separator)) {
			h += string(filepath.Separator)
		}
		return h + rest, true
	}
	return s, true
}

// ReloadWhitelist preserves reload duplicates and display spelling, as the persisted Rust format does.
func ReloadWhitelist(raw []string) bool {
	entries := make([]whitelistEntry, 0, len(raw))
	ok := true
	for _, s := range raw {
		expanded, valid := expandWhitelistHome(s)
		if !valid {
			ok = false
			continue
		}
		entries = append(entries, whitelistEntry{Norm(expanded), expanded})
	}
	whitelist.Lock()
	whitelist.entries = entries
	whitelist.Unlock()
	return ok
}
func whitelistSnapshot() []string {
	out := make([]string, len(whitelist.entries))
	for i, e := range whitelist.entries {
		out[i] = e.display
	}
	return out
}
func AddWhitelist(path string) ([]string, bool) {
	s, ok := expandWhitelistHome(path)
	if !ok {
		return nil, false
	}
	n := Norm(s)
	whitelist.Lock()
	defer whitelist.Unlock()
	for _, e := range whitelist.entries {
		if e.norm == n {
			return whitelistSnapshot(), true
		}
	}
	whitelist.entries = append(whitelist.entries, whitelistEntry{n, s})
	return whitelistSnapshot(), true
}
func RemoveWhitelist(path string) ([]string, bool) {
	s, ok := expandWhitelistHome(path)
	if !ok {
		return nil, false
	}
	n := Norm(s)
	whitelist.Lock()
	defer whitelist.Unlock()
	out := whitelist.entries[:0]
	for _, e := range whitelist.entries {
		if e.norm != n {
			out = append(out, e)
		}
	}
	whitelist.entries = out
	return whitelistSnapshot(), true
}
func CurrentWhitelist() []string {
	whitelist.RLock()
	defer whitelist.RUnlock()
	return whitelistSnapshot()
}
func IsWhitelisted(path string) bool {
	n := Norm(path)
	whitelist.RLock()
	defer whitelist.RUnlock()
	for _, e := range whitelist.entries {
		if atOrUnder(n, e.norm) {
			return true
		}
	}
	return false
}
func HasWhitelistEntryUnder(path string) bool {
	n := Norm(path)
	whitelist.RLock()
	defer whitelist.RUnlock()
	for _, e := range whitelist.entries {
		if atOrUnder(e.norm, n) {
			return true
		}
	}
	return false
}
