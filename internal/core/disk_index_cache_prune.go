package core

import (
	"encoding/hex"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// PruneOrphans removes retired base/delta/coverage groups. The returned count
// is the number of removed base files, including any whose companion cleanup
// subsequently failed (such failures are returned, not reported as success).
// The active mount's files are never removed. Unknown devices are not coverage.
func (c *DiskIndexCache) PruneOrphans(keep VolumeID) (int, error) {
	diskIndexWrites.Lock()
	defer diskIndexWrites.Unlock()
	if !filepath.IsAbs(keep.Mount) || filepath.Clean(keep.Mount) != keep.Mount || strings.ContainsRune(keep.Mount, 0) {
		return 0, cacheInvalid("invalid active mount")
	}
	rootInfo, err := os.Lstat(c.root)
	if errors.Is(err, os.ErrNotExist) {
		return 0, nil
	}
	if err != nil {
		return 0, err
	}
	if !rootInfo.IsDir() || platform.DevEnvStorageIsReparse(rootInfo) || !diskIndexPhysicalDirectory(c.root) {
		return 0, cacheInvalid("cache root is redirected")
	}
	dir, err := os.OpenRoot(c.root)
	if err != nil {
		return 0, err
	}
	defer dir.Close()
	opened, err := dir.Stat(".")
	if err != nil {
		return 0, err
	}
	if !os.SameFile(rootInfo, opened) {
		return 0, cacheInvalid("cache root changed during prune")
	}
	listing, err := dir.Open(".")
	if err != nil {
		return 0, err
	}
	defer listing.Close()
	entries, err := listing.ReadDir(-1)
	if err != nil {
		return 0, err
	}
	now := time.Now()
	removed := 0
	var failures []error
	for _, entry := range entries {
		name := entry.Name()
		baseName := strings.TrimSuffix(strings.TrimSuffix(name, ".tmp"), ".delta.bin")
		if strings.HasSuffix(name, ".delta.bin") {
			baseName += ".bin"
		}
		_, isIndex := diskIndexMountFromName(baseName)
		if isIndex || diskIndexTemporaryName(name) {
			if info, e := dir.Lstat(name); e == nil && info.Mode().IsRegular() && !platform.DevEnvStorageIsReparse(info) {
				if f, e := dir.Open(name); e == nil {
					if opened, e := f.Stat(); e == nil && os.SameFile(info, opened) {
						_ = f.Chmod(0600)
					}
					_ = f.Close()
				}
			}
		}
		if diskIndexTemporaryName(name) {
			info, err := dir.Lstat(name)
			if err == nil && info.Mode().IsRegular() && !platform.DevEnvStorageIsReparse(info) && now.Sub(info.ModTime()) > 30*24*time.Hour {
				current, e := dir.Lstat(name)
				if e == nil && os.SameFile(current, info) && current.Mode().IsRegular() {
					e = dir.Remove(name)
				}
				if e != nil {
					failures = append(failures, e)
				}
			}
			continue
		}
		mount, ok := diskIndexMountFromName(name)
		if !ok || mount == keep.Mount {
			continue
		}
		path := filepath.Join(c.root, name)
		info, err := dir.Lstat(name)
		if err != nil {
			failures = append(failures, err)
			continue
		}
		if !info.Mode().IsRegular() || platform.DevEnvStorageIsReparse(info) {
			continue
		}
		stale := now.Sub(info.ModTime()) > 30*24*time.Hour
		covered := mount != keep.Mount && pathWithin(mount, keep.Mount) && diskIndexSamePhysicalDevice(mount, keep.Mount)
		if !stale && !covered {
			continue
		}
		deltaName := strings.TrimSuffix(name, ".bin") + ".delta.bin"
		group := []struct {
			path string
			info os.FileInfo
		}{{path, info}}
		safe := true
		for _, companion := range entries {
			n := companion.Name()
			if n != deltaName && !diskIndexCoverageNameFor(n, name) && !diskIndexCoverageNameFor(n, deltaName) {
				continue
			}
			p := filepath.Join(c.root, n)
			i, e := dir.Lstat(n)
			if e != nil {
				failures = append(failures, e)
				safe = false
				break
			}
			if !i.Mode().IsRegular() || platform.DevEnvStorageIsReparse(i) {
				safe = false
				break
			}
			group = append(group, struct {
				path string
				info os.FileInfo
			}{p, i})
		}
		if !safe {
			continue
		}
		// Recheck identity before unlinking; never open or recursively traverse
		// a cache entry. Unlink cannot follow a replacement symlink to its target.
		for i, file := range group {
			current, e := dir.Lstat(filepath.Base(file.path))
			if e == nil && (!current.Mode().IsRegular() || platform.DevEnvStorageIsReparse(current) || !os.SameFile(current, file.info)) {
				e = cacheInvalid("cache changed during prune")
			}
			if e == nil {
				e = dir.Remove(filepath.Base(file.path))
			}
			if e != nil {
				failures = append(failures, e)
				break
			}
			if i == 0 {
				removed++
				delete(diskIndexWrites.watermarks, path)
			}
		}
	}
	return removed, errors.Join(failures...)
}

func diskIndexMountFromName(name string) (string, bool) {
	if !strings.HasPrefix(name, "scan-index-") || !strings.HasSuffix(name, ".bin") {
		return "", false
	}
	key := strings.TrimSuffix(strings.TrimPrefix(name, "scan-index-"), ".bin")
	if key == "" || len(key)%2 != 0 {
		return "", false
	}
	b, err := hex.DecodeString(key)
	if err != nil || hex.EncodeToString(b) != key || !utf8.Valid(b) {
		return "", false
	}
	mount := string(b)
	return mount, filepath.IsAbs(mount) && filepath.Clean(mount) == mount && !strings.ContainsRune(mount, 0)
}

func diskIndexCoverageNameFor(name, indexName string) bool {
	key, ok := strings.CutPrefix(name, indexName+".coverage-")
	if !ok {
		return false
	}
	key, ok = strings.CutSuffix(key, ".json")
	if !ok || len(key) != 16 {
		return false
	}
	b, err := hex.DecodeString(key)
	return err == nil && hex.EncodeToString(b) == key
}

func diskIndexSamePhysicalDevice(a, b string) bool {
	if !diskIndexPhysicalDirectory(a) || !diskIndexPhysicalDirectory(b) {
		return false
	}
	_, first, ok := platform.DevEnvStorageFileInfo(a)
	if !ok {
		return false
	}
	_, second, ok := platform.DevEnvStorageFileInfo(b)
	return ok && first[0] == second[0]
}

func diskIndexPhysicalDirectory(path string) bool {
	if !filepath.IsAbs(path) || filepath.Clean(path) != path {
		return false
	}
	for cur := path; ; cur = filepath.Dir(cur) {
		info, err := os.Lstat(cur)
		if err != nil || !info.IsDir() || platform.DevEnvStorageIsReparse(info) {
			return false
		}
		if filepath.Dir(cur) == cur {
			return true
		}
	}
}

func diskIndexTemporaryName(name string) bool {
	if strings.HasPrefix(name, ".scan-index-") && strings.HasSuffix(name, ".tmp") {
		key := strings.TrimSuffix(strings.TrimPrefix(name, ".scan-index-"), ".tmp")
		return key != "" && strings.Trim(key, "0123456789") == ""
	}
	base := strings.TrimSuffix(name, ".tmp")
	if stem, ok := strings.CutSuffix(base, ".delta.bin"); ok {
		base = stem + ".bin"
	}
	_, ok := diskIndexMountFromName(base)
	return strings.HasSuffix(name, ".tmp") && ok
}
