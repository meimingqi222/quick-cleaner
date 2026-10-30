package core

import (
	"context"
	"math"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func TestMacWalkIndexBoundsTimestampsBeforePersistence(t *testing.T) {
	for _, value := range []uint64{0, 1_700_000_000, math.MaxUint32, math.MaxUint32 + 1, math.MaxUint64} {
		cache, volume, _ := diskCacheFixture(t)
		raw := platform.MacWalkResult{Volume: volume, Entries: []platform.MacWalkEntry{
			{Used: true, IsDir: true, Mtime: value},
			{Parent: 0, Name: "payload", Used: true, IsRegular: true, SizeKnown: true, Size: 4096, Mtime: value},
		}}
		scan, err := BuildMacWalkScanResult(context.Background(), raw)
		if err != nil {
			t.Fatal(err)
		}
		want := min(value, uint64(math.MaxUint32))
		if got := scan.Tree.slot(1).Mtime; got != want {
			t.Errorf("acquired timestamp %d, want %d for %d", got, want, value)
		}
		if _, err := cache.Save(volume, scan.Tree, 2); err != nil {
			t.Errorf("native timestamp %d blocked checkpoint: %v", value, err)
		}
	}
}
