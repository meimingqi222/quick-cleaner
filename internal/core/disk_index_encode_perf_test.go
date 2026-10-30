//go:build darwin || linux

package core

import (
	"bytes"
	"runtime"
	"testing"
)

func TestOrderedBaseEncodingDoesNotBuildAChildMatrix(t *testing.T) {
	_, volume, tree := overrideFixture(t, 5_000, 39)
	meta := DiskIndexMetadata{LastEventID: 11, ScannedAt: 17, FileCount: tree.CountUsedFiles(), DirCount: tree.CountUsedDirs(), TotalSize: tree.SizeOf(0)}
	runtime.GC()
	var before, after runtime.MemStats
	runtime.ReadMemStats(&before)
	encoded, err := encodeDiskIndex(tree, volume, meta)
	runtime.ReadMemStats(&after)
	if err != nil {
		t.Fatal(err)
	}
	t.Logf("ordered encode: %d bytes for %d nodes", after.TotalAlloc-before.TotalAlloc, tree.nodeCount())
	if allocated := after.TotalAlloc - before.TotalAlloc; allocated > uint64(tree.nodeCount())*64 {
		t.Errorf("ordered encoding allocated %d bytes for %d nodes by rebuilding and sorting adjacency", allocated, tree.nodeCount())
	}
	decoded, err := decodeDiskIndex(encoded, volume)
	if err != nil || decoded.meta != meta {
		t.Fatal("encoded base failed validation", err)
	}
	reference, err := encodeUnorderedDiskIndex(tree, volume, meta)
	if err != nil || !bytes.Equal(encoded, reference) {
		t.Fatal("ordered fast path changed v7 bytes", err)
	}
}
