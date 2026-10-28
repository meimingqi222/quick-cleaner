package platform

import (
	"context"
	"io/fs"
	"path/filepath"
	"strings"
)

// A shortcut is launch evidence only. Its parent directory and any arguments
// provide no uninstall or recursive deletion authority.
type ShortcutAppEvidence struct{ Shortcut, Target string }

func readAppShortcuts(ctx context.Context, roots []string, read func(string) (string, bool)) []ShortcutAppEvidence {
	var out []ShortcutAppEvidence
	for _, root := range roots {
		seen := 0
		_ = filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
			if ctx.Err() != nil {
				return ctx.Err()
			}
			if err != nil {
				return nil
			}
			rel, err := filepath.Rel(root, path)
			if err != nil {
				return nil
			}
			if rel == "." {
				return nil
			}
			seen++
			if seen > 4000 {
				return fs.SkipAll
			}
			depth := strings.Count(rel, string(filepath.Separator)) + 1
			if entry.IsDir() {
				if depth >= 4 {
					return fs.SkipDir
				}
				return nil
			}
			if !entry.Type().IsRegular() || !strings.EqualFold(filepath.Ext(path), ".lnk") {
				return nil
			}
			if target, ok := read(path); ok {
				out = append(out, ShortcutAppEvidence{path, target})
			}
			return nil
		})
		if ctx.Err() != nil {
			return nil
		}
	}
	return out
}
