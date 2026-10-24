package core

import (
	"os"
	"path/filepath"
	"testing"
)

func devStorageWrite(t *testing.T, path string, bytes int) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, make([]byte, bytes), 0o600); err != nil {
		t.Fatal(err)
	}
}

func TestMeasureBatchStorageEmptyMissing(t *testing.T) {
	root := t.TempDir()
	for _, dirs := range [][]string{nil, {}, {root, filepath.Join(root, "missing")}} {
		total, perDir := MeasureBatchStorage(dirs)
		if total != (AssetStorageSize{}) || len(perDir) != len(dirs) {
			t.Fatalf("%v: total=%+v perDir=%+v", dirs, total, perDir)
		}
		for _, size := range perDir {
			if size != (AssetStorageSize{}) {
				t.Fatalf("nonempty size: %+v", size)
			}
		}
	}
	file := filepath.Join(root, "file")
	devStorageWrite(t, file, 10)
	total, perDir := MeasureBatchStorage([]string{file})
	if total != (AssetStorageSize{}) || perDir[0] != total {
		t.Fatalf("file root must not be scanned: %+v %v", total, perDir)
	}
}

func TestMeasureBatchStorageHardlinks(t *testing.T) {
	root := t.TempDir()
	a, b := filepath.Join(root, "a"), filepath.Join(root, "b")
	devStorageWrite(t, filepath.Join(a, "exclusive"), 100)
	devStorageWrite(t, filepath.Join(a, "shared"), 200)
	devStorageWrite(t, filepath.Join(b, "exclusive"), 50)
	for _, link := range []string{filepath.Join(a, "shared-again"), filepath.Join(b, "shared")} {
		if err := os.Link(filepath.Join(a, "shared"), link); err != nil {
			t.Fatal(err)
		}
	}
	total, perDir := MeasureBatchStorage([]string{a, b})
	if want := (AssetStorageSize{350, 150, 3}); total != want {
		t.Fatalf("total=%+v want=%+v", total, want)
	}
	for i, want := range []AssetStorageSize{{500, 100, 3}, {250, 50, 2}} {
		if perDir[i] != want {
			t.Fatalf("directory %d=%+v want=%+v", i, perDir[i], want)
		}
	}
}

func TestMeasureBatchStorageOverlappingRootsAndOrder(t *testing.T) {
	root := t.TempDir()
	nested := filepath.Join(root, "nested")
	devStorageWrite(t, filepath.Join(root, "file"), 10)
	devStorageWrite(t, filepath.Join(nested, "file"), 20)
	dirs := []string{nested, root, nested, filepath.Join(root, "missing")}
	total, perDir := MeasureBatchStorage(dirs)
	if want := (AssetStorageSize{30, 30, 2}); total != want {
		t.Fatalf("total=%+v want=%+v", total, want)
	}
	for i, want := range []AssetStorageSize{{20, 20, 1}, {30, 30, 2}, {20, 20, 1}, {}} {
		if perDir[i] != want {
			t.Fatalf("directory %d=%+v want=%+v", i, perDir[i], want)
		}
	}
}

func TestMeasureBatchStorageSymlinksExcluded(t *testing.T) {
	root, outside := t.TempDir(), t.TempDir()
	devStorageWrite(t, filepath.Join(root, "file"), 10)
	devStorageWrite(t, filepath.Join(outside, "file"), 20)
	for name, target := range map[string]string{"dir-link": outside, "file-link": filepath.Join(outside, "file"), "loop": root, "broken": filepath.Join(outside, "missing")} {
		if err := os.Symlink(target, filepath.Join(root, name)); err != nil {
			t.Skipf("symlinks unavailable: %v", err)
		}
	}
	total, perDir := MeasureBatchStorage([]string{root, filepath.Join(root, "dir-link")})
	if want := (AssetStorageSize{10, 10, 1}); total != want || perDir[0] != want || perDir[1] != (AssetStorageSize{}) {
		t.Fatalf("total=%+v perDir=%+v", total, perDir)
	}
}

func TestMeasureBatchStorageDepthBoundary(t *testing.T) {
	root := t.TempDir()
	dir := root
	devStorageWrite(t, filepath.Join(dir, "file"), 1)
	for depth := 1; depth <= 33; depth++ {
		dir = filepath.Join(dir, "d")
		devStorageWrite(t, filepath.Join(dir, "file"), depth+1)
	}
	total, perDir := MeasureBatchStorage([]string{root})
	if want := (AssetStorageSize{561, 561, 33}); total != want || perDir[0] != want {
		t.Fatalf("depths 0..32 included, depth 33 excluded: total=%+v perDir=%+v", total, perDir)
	}
}
