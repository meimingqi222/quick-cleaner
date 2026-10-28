package platform

import "path/filepath"

// UserCacheDir returns ~/Library/Caches, as dirs::cache_dir does on macOS.
func UserCacheDir() (string, bool) {
	home, ok := UserHome()
	if !ok {
		return "", false
	}
	return filepath.Join(home, "Library", "Caches"), true
}

// UserDataDir returns ~/Library/Application Support.
func UserDataDir() (string, bool) {
	home, ok := UserHome()
	if !ok {
		return "", false
	}
	return filepath.Join(home, "Library", "Application Support"), true
}
