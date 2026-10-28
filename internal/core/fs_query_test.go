package core

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestQueryFilterPort(t *testing.T) {
	f := NewQueryFilter(nil).WithSizeRange(100, 1000).WithExtensions([]string{"PNG", "jpg"})
	for _, tc := range []struct {
		path string
		size uint64
		want bool
	}{{"test.png", 500, true}, {"photo.JPG", 100, true}, {"test.txt", 500, false}, {"test.png", 50, false}, {"test.png", 2000, false}} {
		if f.MatchesFile(tc.path, tc.size) != tc.want {
			t.Fatal(tc)
		}
	}
}
func TestFSIndexLiveQueriesPort(t *testing.T) {
	root := t.TempDir()
	write := func(rel string) {
		path := filepath.Join(root, rel)
		if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte("hello world 123"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	write("file_a.txt")
	write("file_b.txt")
	write("node_modules/ignored.txt")
	write("$Recycle.Bin/ignored.txt")
	write("nested/third.jpg")
	ctx := context.Background()
	engine := NewFSIndexEngine(nil)
	f := NewQueryFilter([]string{root})
	files := engine.QueryFiles(ctx, f)
	if len(files) != 3 {
		t.Fatal(files)
	}
	if got := engine.QueryFiles(ctx, f.WithMaxDepth(1)); len(got) != 2 {
		t.Fatal(got)
	}
	if got := engine.QueryFiles(ctx, f.WithExtensions([]string{"JPG"})); len(got) != 1 || got[0].Mtime == 0 {
		t.Fatal(got)
	}
	if got := engine.QueryFiles(ctx, f.WithIncludeDirs(true)); len(got) != 5 {
		t.Fatal(got)
	}
	measure, ok := engine.MeasurePath(ctx, root)
	if !ok || measure.Files != 5 || measure.NewestMtime == 0 {
		t.Fatal(measure, ok)
	}
	if buckets := engine.QueryDuplicateBuckets(ctx, []string{root}, 1); len(buckets[15]) != 3 {
		t.Fatal(buckets)
	}
	if files := engine.QueryLargeFiles(ctx, []string{root}, 1, 1); len(files) != 1 {
		t.Fatal(files)
	}
	if files := engine.QueryLargeFiles(ctx, []string{root}, 1, 0); len(files) != 0 {
		t.Fatal(files)
	}
	ctx, cancel := context.WithCancel(ctx)
	cancel()
	if len(engine.QueryFiles(ctx, f)) != 0 {
		t.Fatal("cancel query")
	}
	if _, ok := engine.MeasurePath(ctx, root); ok {
		t.Fatal("cancel measurement")
	}
}

func TestFSIndexMissingAndBlockedRootsFallBack(t *testing.T) {
	root := t.TempDir()
	indexed := filepath.Join(root, "indexed")
	missing := filepath.Join(root, "not_in_index")
	blocked := filepath.Join(indexed, "cloud")
	for _, p := range []string{indexed, missing, blocked} {
		if err := os.MkdirAll(p, 0700); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(filepath.Join(missing, "live.txt"), []byte("live"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(blocked, "cloud.txt"), []byte("cloud"), 0600); err != nil {
		t.Fatal(err)
	}
	tree := NewSizeTree(VolumeFromMountPoint(indexed), []TreeIndexEntry{{0, indexed, true, 0, true, 0}, {0, "cached.txt", false, 123, true, 42}})
	engine := NewFSIndexEngine(tree)
	ctx := context.Background()
	files := engine.QueryFiles(ctx, NewQueryFilter([]string{indexed, missing}))
	if len(files) != 2 || files[0].Mtime != 42 {
		t.Fatal(files)
	}
	if m, ok := engine.MeasurePath(ctx, missing); !ok || m.Files != 1 {
		t.Fatal(m, ok)
	}
	tree.SetBlockedSubtrees([]string{blocked})
	files = engine.QueryFiles(ctx, NewQueryFilter([]string{indexed}))
	if len(files) != 1 || files[0].Path != filepath.Join(blocked, "cloud.txt") {
		t.Fatal(files)
	}
	if m, ok := engine.MeasurePath(ctx, indexed); !ok || m.Files != 1 || m.Size == 123 {
		t.Fatal(m, ok)
	}
}

func TestFSIndexSkipsSymlinks(t *testing.T) {
	root := t.TempDir()
	target := filepath.Join(root, "real")
	if err := os.WriteFile(target, []byte("data"), 0600); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(root, "link")
	if err := os.Symlink(target, link); err != nil {
		t.Skip(err)
	}
	engine := NewFSIndexEngine(nil)
	if files := engine.QueryFiles(context.Background(), NewQueryFilter([]string{root})); len(files) != 1 {
		t.Fatal(files)
	}
	if _, ok := engine.MeasurePath(context.Background(), link); ok {
		t.Fatal("measured symlink")
	}
}

func TestFSIndexEmptyIndexIsNotAbsenceAndMeasurementSkipsDesktopINI(t *testing.T) {
	root := t.TempDir()
	for _, name := range []string{"live.txt", "DESKTOP.INI"} {
		if err := os.WriteFile(filepath.Join(root, name), []byte("data"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	engine := NewFSIndexEngine(NewSizeTree(VolumeFromMountPoint(root), nil))
	if got := engine.QueryFiles(context.Background(), NewQueryFilter([]string{root})); len(got) != 2 {
		t.Fatal(got)
	}
	if got, ok := NewFSIndexEngine(nil).MeasurePath(context.Background(), root); !ok || got.Files != 1 {
		t.Fatal(got, ok)
	}
}
