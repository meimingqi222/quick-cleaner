package platform

import (
	"context"
	"os"
	"path/filepath"
	"slices"
	"strings"
)

func ValidBundleID(id string) bool {
	if len(id) < 3 || !strings.Contains(id, ".") {
		return false
	}
	for _, c := range []byte(id) {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '.' || c == '-' || c == '_') {
			return false
		}
	}
	return true
}

func macSupplementOwners(ctx context.Context, owners []string, installed map[string]struct{}, trash string, lookup func(string) ([]string, bool), read func(string) *string) (map[string]struct{}, bool) {
	if resolved, err := filepath.EvalSymlinks(trash); err == nil {
		trash = resolved
	}
	queries := map[string]struct{}{}
	removed := map[string]struct{}{}
	for _, owner := range owners {
		parts := strings.Split(owner, ".")
		for depth := 2; depth <= len(parts); depth++ {
			queries[strings.Join(parts[:depth], ".")] = struct{}{}
		}
	}
	for id := range queries {
		if ctx.Err() != nil {
			return nil, false
		}
		paths, ok := lookup(id)
		if !ok {
			return nil, false
		}
		slices.Sort(paths)
		paths = slices.Compact(paths)
		for _, path := range paths {
			plist := filepath.Join(path, "Contents", "Info.plist")
			info, err := os.Stat(plist)
			if os.IsNotExist(err) {
				continue
			}
			if err != nil || !info.Mode().IsRegular() {
				return nil, false
			}
			actual := read(plist)
			if actual == nil || !strings.EqualFold(*actual, id) {
				return nil, false
			}
			resolved, err := filepath.EvalSymlinks(path)
			if err != nil {
				return nil, false
			}
			rel, err := filepath.Rel(trash, resolved)
			key := strings.ToLower(*actual)
			if err == nil && rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
				removed[key] = struct{}{}
			} else {
				installed[key] = struct{}{}
			}
		}
	}
	return removed, ctx.Err() == nil
}

func macTrashedBundleIDs(ctx context.Context, home string, read func(string) *string) map[string]struct{} {
	ids := map[string]struct{}{}
	entries, _ := os.ReadDir(filepath.Join(home, ".Trash"))
	for _, entry := range entries {
		if ctx.Err() != nil {
			return nil
		}
		if !entry.IsDir() || filepath.Ext(entry.Name()) != ".app" {
			continue
		}
		if id := read(filepath.Join(home, ".Trash", entry.Name(), "Contents", "Info.plist")); id != nil && ValidBundleID(*id) {
			ids[strings.ToLower(*id)] = struct{}{}
		}
	}
	return ids
}
