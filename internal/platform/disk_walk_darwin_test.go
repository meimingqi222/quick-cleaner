//go:build darwin

package platform

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"
	"unsafe"

	"golang.org/x/sys/unix"
)

func TestDiskWalkNativeTempFixture(t *testing.T) {
	if unsafe.Sizeof(diskWalkAttrList{}) != 24 {
		t.Fatal("attrlist ABI size")
	}
	root := t.TempDir()
	if err := os.Mkdir(filepath.Join(root, "sub"), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "sub", "data"), []byte("abc"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Link(filepath.Join(root, "sub", "data"), filepath.Join(root, "hardlink")); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(root, filepath.Join(root, "loop")); err != nil {
		t.Fatal(err)
	}
	r, err := WalkMacVolume(context.Background(), VolumeFromMountPoint(root), nil)
	var st unix.Stat_t
	if e := unix.Lstat(filepath.Join(root, "sub", "data"), &st); e != nil {
		t.Fatal(e)
	}
	expected := uint64(st.Blocks) * 512
	if err != nil || len(r.Incomplete) != 0 || r.Counts.Files != 2 || r.Counts.AllocatedBytes != expected*2 || r.Counts.OtherEntries != 1 || len(r.Entries) != 5 {
		t.Fatal(r, err)
	}
	for _, e := range r.Entries {
		if e.Name == "data" && (!e.SizeKnown || e.Size != expected) {
			t.Fatal(e)
		}
	}
}

func TestDiskWalkNativeFallbackAndSoftBudget(t *testing.T) {
	root := t.TempDir()
	if err := os.WriteFile(filepath.Join(root, "data"), []byte("abc"), 0600); err != nil {
		t.Fatal(err)
	}
	f, err := os.Open(root)
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	b := diskWalkFallback(context.Background(), f, time.Now(), time.Second)
	var st unix.Stat_t
	if err := unix.Lstat(filepath.Join(root, "data"), &st); err != nil {
		t.Fatal(err)
	}
	if b.reason != "" || !b.fallback || len(b.items) != 1 || b.items[0].Size != uint64(st.Blocks)*512 || !b.items[0].SizeKnown {
		t.Fatal(b)
	}
	b = diskWalkFallback(context.Background(), f, time.Now().Add(-time.Second), time.Millisecond)
	if b.reason != MacWalkBudget || len(b.items) != 0 {
		t.Fatal(b)
	}
}
