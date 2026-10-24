package core

import (
	"os"
	"path/filepath"
	"strings"
	"time"
)

var sqliteSuffixes = []string{"-wal", "-shm", "-journal"}

func sqliteASCIILower(name string) string {
	return strings.Map(func(r rune) rune {
		if r >= 'A' && r <= 'Z' {
			return r + ('a' - 'A')
		}
		return r
	}, name)
}

func SQLiteFamilyKey(name string) (string, bool) {
	name = sqliteASCIILower(name)
	for _, suffix := range sqliteSuffixes {
		if strings.HasSuffix(name, suffix) {
			return strings.TrimSuffix(name, suffix), true
		}
	}
	for _, ext := range []string{".db", ".sqlite", ".sqlite3", ".otc"} {
		if strings.HasSuffix(name, ext) {
			return name, true
		}
	}
	return "", false
}

func IsSQLiteCompanionName(name string) bool {
	name = sqliteASCIILower(name)
	for _, suffix := range sqliteSuffixes {
		if strings.HasSuffix(name, suffix) {
			return true
		}
	}
	return false
}

func HoldsLiveDatabase(path string) bool {
	entries, err := os.ReadDir(path)
	if err != nil {
		return true
	}
	for _, entry := range entries {
		if _, ok := SQLiteFamilyKey(entry.Name()); ok {
			return true
		}
	}
	return false
}

// IsLiveDatabase is a deletion gate, not a file-format detector. Only stale
// families with positively established vacancy can bypass the shape guard.
func IsLiveDatabase(path string) bool { return liveDatabaseWithProbe(path, IsOpen) }

func liveDatabaseWithProbe(path string, probe func(string) (bool, bool)) bool {
	md, err := os.Lstat(path)
	if err != nil {
		return true
	}
	if md.Mode()&os.ModeSymlink != 0 {
		return false
	}
	if md.IsDir() {
		if !HoldsLiveDatabase(path) {
			return false
		}
	} else {
		key, ok := SQLiteFamilyKey(filepath.Base(path))
		if !ok {
			return false
		}
		entries, err := os.ReadDir(filepath.Dir(path))
		if err != nil {
			return true
		}
		count := 0
		for _, entry := range entries {
			if k, ok := SQLiteFamilyKey(entry.Name()); ok && k == key {
				count++
			}
		}
		if count < 2 {
			return false
		}
	}
	if !newestDatabaseWriteIsStale(path, md) {
		return true
	}
	open, known := probe(path)
	return !known || open
}

func newestDatabaseWriteIsStale(path string, md os.FileInfo) bool {
	newest := md.ModTime()
	dir := path
	if !md.IsDir() {
		dir = filepath.Dir(path)
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		return false
	}
	key, _ := SQLiteFamilyKey(filepath.Base(path))
	for _, entry := range entries {
		k, ok := SQLiteFamilyKey(entry.Name())
		if !ok || (!md.IsDir() && k != key) {
			continue
		}
		info, err := os.Stat(filepath.Join(dir, entry.Name()))
		if err != nil {
			return false
		}
		if info.ModTime().After(newest) {
			newest = info.ModTime()
		}
	}
	return time.Since(newest) >= time.Hour
}
