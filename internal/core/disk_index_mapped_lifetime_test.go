//go:build darwin || linux

package core

import (
	"errors"
	"os"
	"runtime"
	"slices"
	"testing"
	"time"
)

// Keep the os.File reachable separately: its own GC cleanup must not make a
// leaking mapping owner appear to have released its resources.
func abandonedMappedFile(t *testing.T, cache *DiskIndexCache, v VolumeID) *os.File {
	t.Helper()
	loaded, err := cache.Load(v)
	if err != nil {
		t.Fatal(err)
	}
	return loaded.Scan.Tree.base.mapped.file
}

func waitMappedFileClosed(t *testing.T, file *os.File) {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		runtime.GC()
		if _, err := file.Stat(); errors.Is(err, os.ErrClosed) {
			return
		} else if err != nil {
			t.Fatal(err)
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatal("unreachable mapping owner did not close its still-reachable file")
}

func TestMappedOwnerReleasesResourcesAfterLastReader(t *testing.T) {
	cache, v, tree := diskCacheFixture(t)
	if _, err := cache.Save(v, tree, 1); err != nil {
		t.Fatal(err)
	}
	file := abandonedMappedFile(t, cache, v)
	defer file.Close()
	waitMappedFileClosed(t, file)
}

func retainedMappedChildren(t *testing.T, cache *DiskIndexCache, v VolumeID) (treeChildView, *os.File) {
	t.Helper()
	loaded, err := cache.Load(v)
	if err != nil {
		t.Fatal(err)
	}
	return loaded.Scan.Tree.childList(0), loaded.Scan.Tree.base.mapped.file
}

func exerciseRetainedMappedChildren(t *testing.T, cache *DiskIndexCache, v VolumeID) *os.File {
	t.Helper()
	view, file := retainedMappedChildren(t, cache, v)
	expected := view.copy()
	if len(expected) == 0 || view.data == nil {
		t.Fatal("fixture needs mapped children")
	}
	for range 8 {
		runtime.GC()
		time.Sleep(time.Millisecond)
		if _, err := file.Stat(); err != nil {
			t.Fatal("child reader lost its mapping owner", err)
		}
		if !slices.Equal(view.copy(), expected) {
			t.Fatal("retained child view changed")
		}
		var actual []uint32
		for child := range view.all {
			actual = append(actual, child)
		}
		if !slices.Equal(actual, expected) {
			t.Fatal("retained child iterator changed")
		}
	}
	return file
}

func TestMappedChildReaderKeepsOwnerUntilReleased(t *testing.T) {
	cache, v, tree := diskCacheFixture(t)
	if _, err := cache.Save(v, tree, 1); err != nil {
		t.Fatal(err)
	}
	file := exerciseRetainedMappedChildren(t, cache, v)
	defer file.Close()
	waitMappedFileClosed(t, file)
}
