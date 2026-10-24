package platform

import (
	"path/filepath"
	"strings"
	"syscall"
)

// DownloadsDir uses only the trusted user's hive, never the elevated account's
// HKCU. Missing identity or folder evidence is not permission to guess a root.
func DownloadsDir() (string, bool) {
	return collectDownloadsDir(UserSID, UserHome, enumFolderStrings)
}

func collectDownloadsDir(sid, home func() (string, bool), read func(syscall.Handle, string) []folderString) (string, bool) {
	base, ok := home()
	if !ok || !filepath.IsAbs(base) {
		return "", false
	}
	id, ok := sid()
	if !ok || id == "" {
		return "", false
	}
	for _, entry := range read(syscall.HKEY_USERS, id+`\`+shellFoldersKey) {
		if !asciiFolderEqual(entry.name, "{374DE290-123F-4565-9164-39C4925E467B}") {
			continue
		}
		path, ok := expandFolderProfile(entry.value, func() (string, bool) { return base, true })
		if ok && filepath.IsAbs(path) && !strings.Contains(path, "%") {
			return filepath.Clean(path), true
		}
	}
	return "", false
}
