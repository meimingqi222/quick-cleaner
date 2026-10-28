// Package core implements UI-independent application behavior.
package core

import (
	"context"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"sort"
)

// Usage measures logical file bytes, not allocated or reclaimable space.
type Usage struct {
	Path  string
	Bytes int64
	Files int64
}

type DiskResult struct {
	Usage
	Entries []Usage
	Skipped int64
}

func isAllowedSystemAncestor(p string) bool {
	clean := filepath.Clean(p)
	switch runtime.GOOS {
	case "darwin":
		return clean == "/var" || clean == "/tmp" || clean == "/etc"
	case "linux":
		return clean == "/bin" || clean == "/sbin" || clean == "/lib" || clean == "/lib64"
	default:
		return false
	}
}

// ScanDirectory never follows symbolic links. Inaccessible entries are counted
// explicitly so a partial inventory cannot masquerade as a complete one.
// The callback runs on the caller's goroutine.
func ScanDirectory(ctx context.Context, root string, progress func(Usage)) (DiskResult, error) {
	if err := ctx.Err(); err != nil {
		return DiskResult{}, err
	}
	root, err := filepath.Abs(root)
	if err != nil {
		return DiskResult{}, err
	}
	result := DiskResult{Usage: Usage{Path: root}}
	// WalkDir rejects a link at the root, but would follow links in its ancestors.
	for p := root; ; p = filepath.Dir(p) {
		info, err := os.Lstat(p)
		if err != nil {
			return result, err
		}
		if info.Mode()&os.ModeSymlink != 0 {
			if p != root && isAllowedSystemAncestor(p) {
				targetInfo, statErr := os.Stat(p)
				if statErr != nil || !targetInfo.IsDir() {
					return result, fmt.Errorf("not a plain directory: %s", p)
				}
				if filepath.Dir(p) == p {
					break
				}
				continue
			}
			return result, fmt.Errorf("not a plain directory: %s", p)
		}
		if !info.IsDir() {
			return result, fmt.Errorf("not a plain directory: %s", p)
		}
		if filepath.Dir(p) == p {
			break
		}
	}
	entries := make(map[string]*Usage)
	err = filepath.WalkDir(root, func(path string, entry fs.DirEntry, walkErr error) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		if walkErr != nil {
			if path == root {
				return walkErr
			}
			result.Skipped++
			return nil
		}
		if path == root {
			return nil
		}
		if entry.Type()&os.ModeSymlink != 0 {
			result.Skipped++
			return nil
		}
		rel, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}
		first := rel
		for parent := filepath.Dir(first); parent != "."; parent = filepath.Dir(first) {
			first = parent
		}
		bucket := entries[first]
		if bucket == nil {
			bucket = &Usage{Path: filepath.Join(root, first)}
			entries[first] = bucket
		}
		if entry.IsDir() {
			return nil
		}
		info, err := entry.Info()
		if err != nil {
			result.Skipped++
			return nil
		}
		if !info.Mode().IsRegular() {
			result.Skipped++
			return nil
		}
		bucket.Bytes += info.Size()
		bucket.Files++
		result.Bytes += info.Size()
		result.Files++
		if progress != nil && result.Files%256 == 0 {
			progress(result.Usage)
		}
		return nil
	})
	for _, entry := range entries {
		result.Entries = append(result.Entries, *entry)
	}
	sort.Slice(result.Entries, func(i, j int) bool {
		a, b := result.Entries[i], result.Entries[j]
		if a.Bytes != b.Bytes {
			return a.Bytes > b.Bytes
		}
		return a.Path < b.Path
	})
	if progress != nil {
		progress(result.Usage)
	}
	return result, err
}
