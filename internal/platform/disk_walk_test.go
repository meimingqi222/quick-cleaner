//go:build darwin || linux

package platform

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"fmt"
	"math/rand"
	"os"
	"path/filepath"
	"sync/atomic"
	"syscall"
	"testing"
	"time"
)

func diskWalkFixture(name string, typ uint32, common, file uint32) []byte {
	b := make([]byte, 24)
	le := binary.LittleEndian
	le.PutUint32(b[4:], common)
	le.PutUint32(b[16:], file)
	put32 := func(n uint32) { v := make([]byte, 4); le.PutUint32(v, n); b = append(b, v...) }
	put64 := func(n uint64) { v := make([]byte, 8); le.PutUint64(v, n); b = append(b, v...) }
	if common&diskWalkName != 0 {
		put32(0)
		put32(uint32(len(name) + 1))
	}
	if common&diskWalkType != 0 {
		put32(typ)
	}
	if common&diskWalkModtime != 0 {
		put64(1700000000)
		put64(0)
	}
	if common&diskWalkFlags != 0 {
		put32(diskWalkDataless)
	}
	if common&diskWalkError != 0 {
		put32(1)
	}
	if file&diskWalkAlloc != 0 {
		put64(8192)
	}
	if common&diskWalkName != 0 {
		le.PutUint32(b[24:], uint32(len(b)-24))
		b = append(b, []byte(name)...)
		b = append(b, 0)
	}
	le.PutUint32(b, uint32(len(b)))
	return b
}

func TestDiskWalkBinaryAttributeOrderingAndSizes(t *testing.T) {
	common := diskWalkName | diskWalkType | diskWalkModtime | diskWalkFlags | diskWalkError | diskWalkReturned
	b := diskWalkFixture("photo😀.jpg", 1, common, diskWalkAlloc)
	e, err := diskWalkParseRecord(b)
	if err != nil || e.Name != "photo😀.jpg" || !e.IsRegular || e.IsDir || e.Flags != diskWalkDataless || e.AttributeError != 1 || e.Mtime != 1700000000 || e.Size != 8192 || !e.SizeKnown {
		t.Fatalf("%+v %v", e, err)
	}
	plain := diskWalkFixture("readable.txt", 1, diskWalkName|diskWalkType, diskWalkAlloc)
	e, err = diskWalkParseRecord(plain)
	if err != nil || !e.SizeKnown || e.Size != 8192 {
		t.Fatal(e, err)
	}
	for _, typ := range []uint32{2, 3, 4, 5, 6, 7} {
		e, err = diskWalkParseRecord(diskWalkFixture("nonregular", typ, diskWalkName|diskWalkType, diskWalkAlloc))
		if err != nil || e.Size != 0 || e.IsRegular {
			t.Fatal("nonregular allocation must not count", e, err)
		}
	}
	batch, err := diskWalkParseBatch(append(b, plain...), 2)
	if err != nil || len(batch) != 2 || batch[1].Name != "readable.txt" {
		t.Fatal(batch, err)
	}
}

func TestDiskWalkMalformedRecordsAndPartialBatch(t *testing.T) {
	b := diskWalkFixture("hello", 1, diskWalkName|diskWalkType, diskWalkAlloc)
	for _, mutate := range []func([]byte){
		func(b []byte) { binary.LittleEndian.PutUint32(b, 0) },
		func(b []byte) { binary.LittleEndian.PutUint32(b[24:], 0xffffffff) },
		func(b []byte) { binary.LittleEndian.PutUint32(b[28:], 0xffffffff) },
		func(b []byte) { b[len(b)-1] = 'x' },
		func(b []byte) { binary.LittleEndian.PutUint32(b[4:], 0xffffffff) },
		func(b []byte) { binary.LittleEndian.PutUint64(b[36:], 1<<63) },
	} {
		bad := append([]byte(nil), b...)
		mutate(bad)
		if _, err := diskWalkParseRecord(bad); err == nil {
			t.Fatalf("accepted %x", bad)
		}
	}
	for _, name := range []string{"..", ".", "a/b", "a\x00b"} {
		if _, err := diskWalkParseRecord(diskWalkFixture(name, 2, diskWalkName|diskWalkType, 0)); err == nil {
			t.Fatal("unsafe name", name)
		}
	}
	out, err := diskWalkParseBatch(append(append([]byte(nil), b...), make([]byte, 24)...), 2)
	if err == nil || len(out) != 1 {
		t.Fatal(out, err)
	}
	r := rand.New(rand.NewSource(456))
	for i := 0; i < 2000; i++ {
		b := make([]byte, r.Intn(1024))
		_, _ = r.Read(b)
		_, _ = diskWalkParseRecord(b)
		_, _ = diskWalkParseBatch(b, r.Intn(5))
	}
}

func diskWalkTestItem(name string, dir bool) diskWalkItem {
	return diskWalkItem{MacWalkEntry: MacWalkEntry{Name: name, IsDir: dir, IsRegular: !dir, Size: 4096, SizeKnown: true, Used: true}, flagsKnown: true}
}
func diskWalkTestState() *diskWalkState { return &diskWalkState{slots: make(chan struct{}, 8)} }

func TestDiskWalkMirrorPruningAndDatalessCoverage(t *testing.T) {
	enum := func(_ context.Context, path string, _ time.Duration) diskWalkBatch {
		if path == "/System/Volumes" {
			cloud := diskWalkTestItem("cloud", true)
			cloud.Flags = diskWalkDataless
			return diskWalkBatch{items: []diskWalkItem{diskWalkTestItem("Data", true), diskWalkTestItem("VM", true), cloud}}
		}
		if path == "/System/Volumes/VM" {
			return diskWalkBatch{items: []diskWalkItem{diskWalkTestItem("swap", false)}}
		}
		t.Errorf("unexpected enumeration %s", path)
		return diskWalkBatch{}
	}
	r, err := diskWalkAcquire(context.Background(), VolumeID{Mount: "/System/Volumes"}, nil, enum, diskWalkTestState(), 8, time.Second, time.Second)
	if err != nil || len(r.Entries) != 4 || r.Entries[3].Parent != 1 || r.Entries[3].Name != "swap" || r.Counts.ExcludedMirrors != 1 || r.Counts.SkippedDataless != 1 || len(r.Incomplete) != 1 || r.Incomplete[0].Index != 2 {
		t.Fatalf("%+v %v", r, err)
	}
	if r.Counts.AllocatedBytes != 4096 || r.Counts.Files != 1 {
		t.Fatal(r.Counts)
	}
}

func TestDiskWalkTimeoutContinuesAndPersistsExclusion(t *testing.T) {
	state := diskWalkTestState()
	release := make(chan struct{})
	defer close(release)
	var calls atomic.Int32
	enum := func(_ context.Context, path string, _ time.Duration) diskWalkBatch {
		if path == "/fixture" {
			return diskWalkBatch{items: []diskWalkItem{diskWalkTestItem("hang", true), diskWalkTestItem("ok", true)}}
		}
		if filepath.Base(path) == "hang" {
			calls.Add(1)
			<-release
			return diskWalkBatch{items: []diskWalkItem{diskWalkTestItem("late", false)}}
		}
		return diskWalkBatch{items: []diskWalkItem{diskWalkTestItem("present", false)}}
	}
	for scan := 0; scan < 2; scan++ {
		r, err := diskWalkAcquire(context.Background(), VolumeID{Mount: "/fixture"}, nil, enum, state, 8, time.Second, 20*time.Millisecond)
		if err != nil || r.Counts.Files != 1 || len(r.Incomplete) != 1 || r.Entries[len(r.Entries)-1].Name != "present" {
			t.Fatal(r, err)
		}
		if scan == 0 && r.Counts.TimedOutDirectories != 1 {
			t.Fatal(r.Counts)
		}
		if scan == 1 && r.Counts.SkippedHung != 1 {
			t.Fatal(r.Counts)
		}
	}
	if calls.Load() != 1 {
		t.Fatal("hung path retried", calls.Load())
	}
}

func TestDiskWalkCancellationDoesNotWaitForBlockedIO(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	entered := make(chan struct{})
	release := make(chan struct{})
	defer close(release)
	enum := func(context.Context, string, time.Duration) diskWalkBatch {
		close(entered)
		<-release
		return diskWalkBatch{}
	}
	done := make(chan error, 1)
	go func() {
		r, err := diskWalkAcquire(ctx, VolumeID{Mount: "/fixture"}, nil, enum, diskWalkTestState(), 8, time.Second, time.Hour)
		if len(r.Incomplete) != 1 || r.Incomplete[0].Reason != MacWalkCancelled {
			done <- fmt.Errorf("missing coverage %+v", r)
			return
		}
		done <- err
	}()
	<-entered
	cancel()
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatal(err)
		}
	case <-time.After(time.Second):
		t.Fatal("cancellation waited for kernel call")
	}
}

func TestDiskWalkWorkerBoundAndCapacityCoverage(t *testing.T) {
	state := diskWalkTestState()
	release := make(chan struct{})
	defer close(release)
	var active, peak atomic.Int32
	enum := func(_ context.Context, path string, _ time.Duration) diskWalkBatch {
		if path == "/fixture" {
			b := diskWalkBatch{}
			for i := 0; i < 20; i++ {
				b.items = append(b.items, diskWalkTestItem(fmt.Sprint(i), true))
			}
			return b
		}
		n := active.Add(1)
		for old := peak.Load(); n > old && !peak.CompareAndSwap(old, n); old = peak.Load() {
		}
		defer active.Add(-1)
		<-release
		return diskWalkBatch{}
	}
	r, err := diskWalkAcquire(context.Background(), VolumeID{Mount: "/fixture"}, nil, enum, state, 100, time.Second, 20*time.Millisecond)
	if err != nil || peak.Load() != 8 || r.Counts.TimedOutDirectories != 8 || r.Counts.IncompleteDirectories != 20 {
		t.Fatal(r.Counts, peak.Load(), err)
	}
	capacity := 0
	for _, c := range r.Incomplete {
		if c.Reason == MacWalkCapacity {
			capacity++
		}
	}
	if capacity != 12 {
		t.Fatal(capacity)
	}
}

func TestDiskWalkTwoLocalWorkers(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	entered := make(chan struct{}, 6)
	release := make(chan struct{})
	defer close(release)
	enum := func(_ context.Context, path string, _ time.Duration) diskWalkBatch {
		if path == "/fixture" {
			batch := diskWalkBatch{}
			for i := 0; i < 6; i++ {
				batch.items = append(batch.items, diskWalkTestItem(fmt.Sprint(i), true))
			}
			return batch
		}
		entered <- struct{}{}
		select {
		case <-release:
		case <-ctx.Done():
		}
		return diskWalkBatch{}
	}
	done := make(chan error, 1)
	go func() {
		_, err := diskWalkAcquire(ctx, VolumeID{Mount: "/fixture"}, nil, enum, diskWalkTestState(), 2, time.Second, time.Second)
		done <- err
	}()
	for i := 0; i < 2; i++ {
		select {
		case <-entered:
		case <-ctx.Done():
			t.Fatal("two workers did not start")
		}
	}
	select {
	case <-entered:
		t.Fatal("more than two local workers started")
	case <-time.After(20 * time.Millisecond):
	}
	// Cancel blocked workers rather than allowing the hard timeout to replace
	// them; this test isolates local scheduling from abandoned-kernel-call slots.
	cancel()
	if err := <-done; !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	if _, err := WalkMacVolumeFewThreads(ctx, VolumeID{}, nil); !errors.Is(err, context.Canceled) {
		t.Fatal("few-thread entry lost cancellation", err)
	}
}

func TestDiskWalkPartialBudgetAndErrorsKeepObservedChildren(t *testing.T) {
	enum := func(_ context.Context, path string, _ time.Duration) diskWalkBatch {
		if path == "/fixture" {
			return diskWalkBatch{items: []diskWalkItem{diskWalkTestItem("sub", true), diskWalkTestItem("seen", false)}, reason: MacWalkBudget, fallback: true}
		}
		return diskWalkBatch{reason: MacWalkUnreadable, err: os.ErrPermission}
	}
	var last MacWalkProgress
	r, err := diskWalkAcquire(context.Background(), VolumeID{Mount: "/fixture"}, func(p MacWalkProgress) { last = p }, enum, diskWalkTestState(), 8, time.Second, time.Second)
	if err != nil || len(r.Entries) != 3 || len(r.Incomplete) != 2 || r.Counts.Files != 1 || last != r.Counts || r.Counts.FallbackDirectories != 1 {
		t.Fatal(r, err)
	}
}

// This real-filesystem adapter is injected only at the acquisition boundary;
// it does not pretend that Linux executed getattrlistbulk.
func diskWalkTestFS(ctx context.Context, path string, _ time.Duration) diskWalkBatch {
	entries, err := os.ReadDir(path)
	if err != nil {
		return diskWalkBatch{reason: MacWalkUnreadable, err: err}
	}
	batch := diskWalkBatch{}
	for _, d := range entries {
		if err := ctx.Err(); err != nil {
			return diskWalkBatch{reason: MacWalkCancelled, err: err}
		}
		info, err := os.Lstat(filepath.Join(path, d.Name()))
		if err != nil {
			return diskWalkBatch{reason: MacWalkUnreadable, err: err}
		}
		e := diskWalkTestItem(d.Name(), info.IsDir())
		e.IsRegular = info.Mode().IsRegular()
		e.Size = 0
		if e.IsRegular {
			e.Size = uint64(info.Sys().(*syscall.Stat_t).Blocks) * 512
		}
		e.Mtime = uint64(max(int64(0), info.ModTime().Unix()))
		batch.items = append(batch.items, e)
	}
	return batch
}
func TestDiskWalkRealTempFilesystemBoundary(t *testing.T) {
	root := t.TempDir()
	if err := os.MkdirAll(filepath.Join(root, "node_modules", "pkg"), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "node_modules", "pkg", "index.js"), []byte("hello"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Link(filepath.Join(root, "node_modules", "pkg", "index.js"), filepath.Join(root, "hardlink")); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(root, filepath.Join(root, "loop")); err != nil {
		t.Skip("symlink unavailable", err)
	}
	r, err := diskWalkAcquire(context.Background(), VolumeFromMountPoint(root), nil, diskWalkTestFS, diskWalkTestState(), 8, time.Second, time.Second)
	if err != nil || len(r.Entries) != 6 || r.Counts.Files != 2 || r.Counts.OtherEntries != 1 || len(r.Incomplete) != 0 {
		t.Fatal(r, err)
	}
	for _, e := range r.Entries {
		if e.Name == "index.js" && (e.Size == 5 || r.Counts.AllocatedBytes != e.Size*2) {
			t.Fatal("logical length substituted or hard links deduplicated", e, r.Counts)
		}
	}
	for i, e := range r.Entries {
		if i > 0 && (!r.Entries[e.Parent].IsDir || e.Parent >= uint64(i)) {
			t.Fatal("invalid parent", i, e)
		}
	}
}

func TestDiskWalkBatchAllocatesRecordsOnce(t *testing.T) {
	const records = 512
	record := diskWalkFixture("owned-name", 1, diskWalkName|diskWalkType, diskWalkAlloc)
	data := bytes.Repeat(record, records)
	var parsed []diskWalkItem
	allocated := testing.AllocsPerRun(5, func() {
		var err error
		parsed, err = diskWalkParseBatch(data, records)
		if err != nil || len(parsed) != records {
			t.Fatal("batch parse changed", err, len(parsed))
		}
	})
	if allocated > records+2 {
		t.Errorf("parsing %d names used %.0f allocations; record storage grew repeatedly", records, allocated)
	}
	if parsed[0].Name != "owned-name" || parsed[records-1].Size != 8192 {
		t.Fatal("batch records changed")
	}
	for i := range data {
		data[i] = 0
	}
	if parsed[0].Name != "owned-name" || parsed[records-1].Name != "owned-name" {
		t.Fatal("parsed names still alias the native buffer")
	}
}
