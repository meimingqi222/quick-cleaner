//go:build darwin || linux

package core

import (
	"fmt"
	"testing"
)

func TestMappedDiscoveryFlagsAndTagsAvoidNameAllocations(t *testing.T) {
	cache, err := NewDiskIndexCache(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	volume := VolumeFromMountPoint(diskCacheVolumeDir(t))
	entries := []TreeIndexEntry{{Used: true, IsDir: true, Name: "/"}, {Used: true, IsDir: true, Name: "tagged"}, {Used: true, Parent: 1, Name: "CaCheDiR.TaG", Size: 16}}
	for i := range 512 {
		entries = append(entries, TreeIndexEntry{Used: true, IsDir: true, Name: fmt.Sprintf("directory-with-a-long-name-%04d", i)}, TreeIndexEntry{Used: true, Name: fmt.Sprintf("ordinary-file-with-a-long-name-%04d", i), Size: 1})
	}
	if _, err = cache.Save(volume, NewSizeTree(volume, entries), 1); err != nil {
		t.Fatal(err)
	}
	loaded, err := cache.Load(volume)
	if err != nil {
		t.Fatal(err)
	}
	tree := loaded.Scan.Tree
	t.Cleanup(tree.base.mapped.close)
	if allocs := testing.AllocsPerRun(3, func() {
		for i := 0; i < tree.nodeCount(); i++ {
			_ = tree.Valid(uint32(i))
			_ = tree.IsDir(uint32(i))
		}
	}); allocs > 0 {
		t.Errorf("mapped flags allocated names: %.0f allocations", allocs)
	}
	var tags []uint64
	if allocs := testing.AllocsPerRun(3, func() { tags = cacheTagBitsSlow(tree) }); allocs > 2 {
		t.Errorf("mapped tag scan allocated names: %.0f allocations", allocs)
	}
	if !tagBit(tags, 1) {
		t.Fatal("mapped mixed-case tag was not found")
	}
	// Overrides and appended nodes must take precedence over the immutable base.
	tag := tree.slot(2)
	tag.Name = "ordinary-file"
	tree.setSlot(2, tag)
	if tagBit(cacheTagBitsSlow(tree), 1) {
		t.Fatal("mapped tag name override ignored")
	}
	tag.Name = "CACHEDIR.TAG"
	tag.Parent = 0
	tree.setSlot(2, tag)
	if tags := cacheTagBitsSlow(tree); !tagBit(tags, 0) || tagBit(tags, 1) {
		t.Fatal("mapped tag parent override ignored")
	}
	dir := tree.slot(1)
	dir.Used = false
	tree.setSlot(1, dir)
	tree.entries = append(tree.entries, treeSlot{TreeIndexEntry: TreeIndexEntry{Used: true, IsDir: true, Name: "added"}}, treeSlot{TreeIndexEntry: TreeIndexEntry{Used: true, Parent: uint32(tree.baseCount), Name: "CACHEDIR.TAG", Size: 1}})
	tags = cacheTagBitsSlow(tree)
	if tree.Valid(1) || tree.IsDir(1) || tagBit(tags, 1) {
		t.Fatal("mapped tombstone or renamed tag ignored")
	}
	if !tree.IsDir(uint32(tree.baseCount)) || !tagBit(tags, uint32(tree.baseCount)) {
		t.Fatal("appended tagged directory ignored")
	}
	if tree.Valid(uint32(tree.nodeCount())) || tree.IsDir(uint32(tree.nodeCount())) {
		t.Fatal("out-of-range node accepted")
	}
	ordinary := tree.slot(4)
	ordinary.Name, ordinary.Parent = "CACHEDIR.TAG", 3
	tree.setSlot(4, ordinary)
	if !tagBit(cacheTagBitsSlow(tree), 3) {
		t.Fatal("ordinary base node renamed to tag was ignored")
	}
	ordinary.Used = false
	tree.setSlot(4, ordinary)
	if tagBit(cacheTagBitsSlow(tree), 3) {
		t.Fatal("tombstoned tag was retained")
	}
	ordinary.Used, ordinary.IsDir = true, true
	tree.setSlot(4, ordinary)
	if tagBit(cacheTagBitsSlow(tree), 3) {
		t.Fatal("directory named as a tag was accepted")
	}
}

func BenchmarkMappedCacheTagScan(b *testing.B) {
	v := VolumeFromMountPoint(b.TempDir())
	cache, err := NewDiskIndexCache(b.TempDir())
	if err != nil {
		b.Fatal(err)
	}
	entries := spillEntries(200_000, 16)
	entries = append(entries, TreeIndexEntry{Used: true, Parent: 16, Name: "CaCheDiR.TaG", Size: 16})
	if _, err := cache.Save(v, NewSizeTree(v, entries), 1); err != nil {
		b.Fatal(err)
	}
	loaded, err := cache.Load(v)
	if err != nil {
		b.Fatal(err)
	}
	tree := loaded.Scan.Tree
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		if tags := cacheTagBitsSlow(tree); !tagBit(tags, 16) {
			b.Fatal("mixed case tag lost")
		}
	}
}
