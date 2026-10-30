//go:build darwin

package platform

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"testing"
	"time"
)

func TestDiskWalkNativeReusesBufferWithoutAliasingResults(t *testing.T) {
	if err := diskWalkLoad(); err != nil {
		t.Skip(err)
	}
	root := t.TempDir()
	empty := filepath.Join(root, "empty")
	if err := os.Mkdir(empty, 0700); err != nil {
		t.Fatal(err)
	}
	first := diskWalkNative(context.Background(), empty, 5*time.Second)
	if first.fallback {
		t.Skip("filesystem does not support bulk enumeration")
	}
	if first.err != nil {
		t.Fatal(first.err)
	}
	runtime.GC()
	var before, after runtime.MemStats
	runtime.ReadMemStats(&before)
	for range 256 {
		batch := diskWalkNative(context.Background(), empty, 5*time.Second)
		if batch.err != nil || len(batch.items) != 0 {
			t.Fatal("empty enumeration changed", batch.err)
		}
	}
	runtime.ReadMemStats(&after)
	// The race runtime intentionally drops some sync.Pool entries. Even then,
	// reuse must stay well below the original 64 MiB of native slabs.
	if allocated := after.TotalAlloc - before.TotalAlloc; allocated > 48<<20 {
		t.Errorf("256 empty directories allocated %d bytes of per-directory native buffers", allocated)
	}
	for _, name := range []string{"first", "second"} {
		dir := filepath.Join(root, name)
		if err := os.Mkdir(dir, 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(dir, name+".bin"), []byte("data"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	old := runtime.GOMAXPROCS(1)
	defer runtime.GOMAXPROCS(old)
	a := diskWalkNative(context.Background(), filepath.Join(root, "first"), 5*time.Second)
	b := diskWalkNative(context.Background(), filepath.Join(root, "second"), 5*time.Second)
	if a.err != nil || b.err != nil || len(a.items) != 1 || len(b.items) != 1 {
		t.Fatal("native entries lost", a.err, b.err)
	}
	if a.items[0].Name != "first.bin" || b.items[0].Name != "second.bin" {
		t.Fatal("reused native bytes changed an earlier snapshot")
	}
}

func BenchmarkDiskWalkNative(b *testing.B) {
	if err := diskWalkLoad(); err != nil {
		b.Fatal(err)
	}
	for _, size := range []struct {
		name  string
		files int
	}{{"empty", 0}, {"single-batch", 100}, {"multi-batch", 4096}} {
		b.Run(size.name, func(b *testing.B) {
			root := b.TempDir()
			for i := range size.files {
				if err := os.WriteFile(filepath.Join(root, fmt.Sprintf("entry-%04d.bin", i)), []byte("data"), 0600); err != nil {
					b.Fatal(err)
				}
			}
			warm := diskWalkNative(context.Background(), root, time.Second)
			if warm.fallback {
				b.Skip("filesystem does not support bulk enumeration")
			}
			if warm.err != nil || len(warm.items) != size.files {
				b.Fatal("warm native enumeration", warm.err, len(warm.items))
			}
			b.ReportAllocs()
			b.ResetTimer()
			for i := 0; i < b.N; i++ {
				batch := diskWalkNative(context.Background(), root, time.Second)
				if batch.err != nil || batch.reason != "" || len(batch.items) != size.files {
					b.Fatal("native enumeration changed", batch.err, batch.reason, len(batch.items))
				}
			}
		})
	}
}

func TestDiskWalkNativeClosesDescriptorsOnEveryExit(t *testing.T) {
	if err := diskWalkLoad(); err != nil {
		t.Skip(err)
	}
	root := t.TempDir()
	initial, err := os.ReadDir("/dev/fd")
	if err != nil {
		t.Skip(err)
	}
	cancelled, cancel := context.WithCancel(context.Background())
	cancel()
	for i := range 128 {
		ctx := context.Background()
		budget := time.Second
		switch i % 3 {
		case 1:
			ctx = cancelled
		case 2:
			budget = 0
		}
		batch := diskWalkNative(ctx, root, budget)
		if ctx.Err() != nil && batch.reason != MacWalkCancelled {
			t.Fatal("native cancellation changed", batch.reason, batch.err)
		}
		if ctx.Err() == nil && budget == 0 && batch.reason != MacWalkBudget {
			t.Fatal("native budget changed", batch.reason, batch.err)
		}
	}
	final, err := os.ReadDir("/dev/fd")
	if err != nil {
		t.Fatal(err)
	}
	if len(final) > len(initial)+2 {
		t.Fatalf("native worker leaked descriptors: %d -> %d", len(initial), len(final))
	}
}
