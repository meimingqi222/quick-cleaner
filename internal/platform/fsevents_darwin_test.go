package platform

import (
	"context"
	"errors"
	"testing"
	"unsafe"
)

func TestFSEventNativeCallback(t *testing.T) {
	if unsafe.Sizeof(fseventContext{}) != 40 || unsafe.Offsetof(fseventContext{}.Info) != 8 {
		t.Fatal("FSEventStreamContext ABI mismatch")
	}
	c := &fseventCollector{root: "/"}
	handle := uintptr(fseventSequence.Add(1))
	fseventHandles.Store(handle, c)
	defer fseventHandles.Delete(handle)
	name := append([]byte("/Library/FSEventFixture"), 0)
	invalidUTF8 := []byte{'/', 0xff, 0}
	paths := []*byte{&name[0], &invalidUTF8[0], nil}
	flags := []uint32{0x01, 0, 0x10}
	ids := []uint64{11, 12, 13}
	receiveFSEvents(0, handle, 3, unsafe.Pointer(&paths[0]), &flags[0], &ids[0])
	got, err := c.changes(10, 13, "", nil, 0)
	if err != nil || !c.historyDone || c.maxID != 13 || got.RawEventCount != 1 || len(got.MustRescan) != 1 || got.MustRescan[0] != "/Library/FSEventFixture" {
		t.Fatalf("native callback lost flags/IDs or accepted invalid UTF-8: %+v, %+v, %v", c, got, err)
	}
}

func TestFSEventNativeRead(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := ReadFSEventChanges(ctx, t.TempDir(), 1, ""); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancel before native initialization: %v", err)
	}
	id, err := CurrentFSEventID()
	if err != nil || id == 0 {
		t.Fatalf("current native FSEvents ID: %d, %v", id, err)
	}
	if changes, err := ReadFSEventChanges(context.Background(), t.TempDir(), id, ""); err != nil || changes == nil {
		t.Fatalf("replay from current event ID: %+v, %v", changes, err)
	}
}
