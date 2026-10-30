package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func TestCacheModeRunsRepeatedLoadsWithoutChangingSource(t *testing.T) {
	root, err := os.MkdirTemp("", "qc-bench-volume-")
	if err != nil {
		t.Fatal(err)
	}
	defer os.RemoveAll(root)
	cache, err := core.NewDiskIndexCache(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	volume := core.VolumeFromMountPoint(root)
	tree := core.NewSizeTree(volume, []core.TreeIndexEntry{{Name: "root", Used: true, IsDir: true}, {Parent: 0, Name: "payload", Used: true, Size: 7}})
	if _, err = cache.Save(volume, tree, 1); err != nil {
		t.Fatal(err)
	}
	base, _ := cache.Paths(volume)
	before, err := os.ReadFile(base)
	if err != nil {
		t.Fatal(err)
	}
	var out bytes.Buffer
	if err = run([]string{"-root", root, "-cache-dir", cache.Root(), "-rounds", "2", "-discover=false", "-json"}, &out); err != nil {
		t.Fatal(err)
	}
	decoder := json.NewDecoder(&out)
	for i := 1; i <= 2; i++ {
		var entry result
		if err = decoder.Decode(&entry); err != nil {
			t.Fatal(err)
		}
		if entry.Round != i || entry.Mode != "cache" || entry.Files != 1 || entry.Dirs != 1 || entry.Load.Milliseconds <= 0 {
			t.Fatalf("round result: %+v", entry)
		}
	}
	after, err := os.ReadFile(base)
	if err != nil || !bytes.Equal(before, after) {
		t.Fatal("benchmark modified the source cache", err)
	}
}

func TestSnapshotCopiesOnlySelectedVolumeAsPrivateFiles(t *testing.T) {
	source, err := core.NewDiskIndexCache(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	volume := core.VolumeFromMountPoint(string(filepath.Separator))
	base, delta := source.Paths(volume)
	names := []string{filepath.Base(base), filepath.Base(delta), filepath.Base(base) + ".coverage-0123456789abcdef.json"}
	for _, name := range append(append([]string{}, names...), "unrelated.json", "scan-index-unrelated.bin") {
		if err = os.WriteFile(filepath.Join(source.Root(), name), []byte("private fixture"), 0644); err != nil {
			t.Fatal(err)
		}
	}
	destination := t.TempDir()
	if err = copyCacheGroup(source, destination, volume); err != nil {
		t.Fatal(err)
	}
	files, err := os.ReadDir(destination)
	if err != nil || len(files) != len(names) {
		t.Fatalf("snapshot files: %d %v", len(files), err)
	}
	for _, name := range names {
		path := filepath.Join(destination, name)
		data, err := os.ReadFile(path)
		if err != nil || string(data) != "private fixture" {
			t.Fatal("snapshot content mismatch", err)
		}
		info, err := os.Stat(path)
		if err != nil {
			t.Fatal(err)
		}
		if runtime.GOOS != "windows" && info.Mode().Perm() != 0600 {
			t.Fatal("snapshot is not private", info.Mode())
		}
	}
}

func TestSnapshotRejectsRedirectedCacheFile(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("symlink privilege required")
	}
	source, err := core.NewDiskIndexCache(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	base, _ := source.Paths(core.VolumeFromMountPoint(string(filepath.Separator)))
	outside := filepath.Join(t.TempDir(), "private")
	if err = os.WriteFile(outside, []byte("do not follow"), 0600); err != nil {
		t.Fatal(err)
	}
	if err = os.Symlink(outside, base); err != nil {
		t.Fatal(err)
	}
	if err = copyCacheGroup(source, t.TempDir(), core.VolumeFromMountPoint(string(filepath.Separator))); err == nil {
		t.Fatal("snapshot followed a redirected cache file")
	}
}

func TestProductDiscoveryIncludesUncoveredLiveBranch(t *testing.T) {
	root, err := os.MkdirTemp("", "qc-bench-live-")
	if err != nil {
		t.Fatal(err)
	}
	defer os.RemoveAll(root)
	root, err = filepath.EvalSymlinks(root)
	if err != nil {
		t.Fatal(err)
	}
	project := filepath.Join(root, "work", "unknown")
	modules := filepath.Join(project, "node_modules")
	if err := os.MkdirAll(modules, 0700); err != nil {
		t.Fatal(err)
	}
	for path, data := range map[string]string{filepath.Join(project, "package.json"): `{}`, filepath.Join(modules, "payload"): "data"} {
		if err := os.WriteFile(path, []byte(data), 0600); err != nil {
			t.Fatal(err)
		}
	}
	cache, err := core.NewDiskIndexCache(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	volume := core.VolumeFromMountPoint(root)
	tree := core.NewSizeTree(volume, []core.TreeIndexEntry{{Name: "root", Used: true, IsDir: true}, {Name: "work", Used: true, IsDir: true}})
	tree.SetBlockedSubtrees([]string{project})
	if _, err := cache.Save(volume, tree, 1); err != nil {
		t.Fatal(err)
	}
	var out bytes.Buffer
	if err := run([]string{"-root", root, "-cache-dir", cache.Root(), "-rounds", "1", "-product-discovery", "-json"}, &out); err != nil {
		t.Fatal(err)
	}
	var entry result
	if err := json.NewDecoder(&out).Decode(&entry); err != nil {
		t.Fatal(err)
	}
	if entry.Items != 1 || entry.Fingerprint == "" {
		t.Fatalf("product plan missed live fallback: %+v", entry)
	}
}
