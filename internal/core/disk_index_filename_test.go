package core

import (
	"errors"
	"path/filepath"
	"runtime"
	"testing"
)

func TestDiskIndexCacheNativeFilenameRoundTrip(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("backslash is a path separator on Windows")
	}
	c, v, _ := diskCacheFixture(t)
	name := `G:\out\pdf\template_1.pdf`
	tree := NewSizeTree(v, []TreeIndexEntry{
		{Used: true, IsDir: true, Name: "/"},
		{Used: true, Name: name, Size: 4096},
	})
	if _, err := c.Save(v, tree, 1); err != nil {
		t.Fatalf("legal native filename rejected during save: %v", err)
	}
	loaded, err := c.Load(v)
	if err != nil {
		t.Fatalf("legal native filename rejected during load: %v", err)
	}
	path := filepath.Join(v.Mount, name)
	idx, ok := loaded.Scan.Tree.FindNodeByPath(path)
	if !ok || loaded.Scan.Tree.EntryName(idx) != name || loaded.Scan.Tree.SizeOf(idx) != 4096 {
		t.Fatal("backslash filename was split or lost")
	}
	if !loaded.Scan.Tree.UpsertFileWithMtime(path, 8192, 10) {
		t.Fatal("updating backslash filename failed")
	}
	appended := filepath.Join(v.Mount, `HKEY_CURRENT_USER\Software\example`)
	if !loaded.Scan.Tree.UpsertFile(appended, 1024) {
		t.Fatal("appending backslash filename failed")
	}
	if loaded.Scan.Tree.base != nil {
		if _, _, _, ok := loaded.Scan.Tree.validateOverrides(1, 1); !ok {
			t.Fatal("legal native filename rejected by incremental fast path")
		}
	}
	if _, err := c.PersistIncremental(v, loaded.Scan.Tree, 2); err != nil {
		t.Fatalf("legal native filename rejected during incremental save: %v", err)
	}
	reloaded, err := c.Load(v)
	if err != nil || !reloaded.DeltaApplied {
		t.Fatalf("incremental index failed to reload: %v", err)
	}
	for path, want := range map[string]uint64{path: 8192, appended: 1024} {
		idx, ok := reloaded.Scan.Tree.FindNodeByPath(path)
		if !ok || reloaded.Scan.Tree.SizeOf(idx) != want {
			t.Fatalf("incremental filename lost: %q", path)
		}
	}
}

func TestDiskIndexCacheLoadsExistingNativeFilename(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("backslash is a path separator on Windows")
	}
	c, v, tree := diskCacheFixture(t)
	if _, err := c.Save(v, tree, 1); err != nil {
		t.Fatal(err)
	}
	base, _ := c.Paths(v)
	raw := diskCacheRead(t, base)
	entry := int(cacheU32(raw, 84)) + 3*24
	name := int(cacheU32(raw, 80)) + int(cacheU32(raw, entry+4)) + 2
	raw[name+5] = '\\' // index.js -> index\js, as an already-persisted Unix index.
	if err := publishDiskIndexCovered(base, raw, tree, 0, 1, false); err != nil {
		t.Fatal(err)
	}
	loaded, err := c.Load(v)
	if err != nil {
		t.Fatalf("existing Unix index rejected: %v", err)
	}
	if _, ok := loaded.Scan.Tree.FindNodeByPath(filepath.Join(v.Mount, "project", "node_modules", `index\js`)); !ok {
		t.Fatal("existing native filename not preserved")
	}
}

func TestDiskIndexCacheRejectsUnsafeNativeComponents(t *testing.T) {
	names := []string{"", ".", "..", "../escape", "a/b", "a\x00b"}
	if runtime.GOOS == "windows" {
		names = append(names, `a\b`, `..\escape`)
	}
	for _, name := range names {
		t.Run(name, func(t *testing.T) {
			c, v, _ := diskCacheFixture(t)
			tree := NewSizeTree(v, []TreeIndexEntry{{Used: true, IsDir: true, Name: "/"}, {Used: true, Name: name, Size: 1}})
			if _, err := c.Save(v, tree, 1); !errors.Is(err, ErrDiskIndexCorrupt) {
				t.Fatalf("unsafe name %q accepted: %v", name, err)
			}
		})
	}
}
