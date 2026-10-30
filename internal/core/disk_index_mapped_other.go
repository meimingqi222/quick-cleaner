//go:build !darwin && !linux

package core

import "os"

type mappedDiskIndex struct{}

type mappedTreeBase struct{}

func newMappedSizeTree(*mappedDiskIndex, []byte, VolumeID, int, int) *SizeTree { return nil }

func (b *mappedTreeBase) rememberSortedChildren(uint32, []uint32) {}
func (b *mappedTreeBase) rememberCacheTag(uint32)                 {}
func (b *mappedTreeBase) cacheTagNodes() []uint32                 { return nil }

func (b *mappedTreeBase) children(uint32) treeChildView { return treeChildView{} }

func (b *mappedTreeBase) values(uint32) treeSlot { return treeSlot{} }
func (b *mappedTreeBase) name(uint32) string     { return "" }
func (b *mappedTreeBase) pool() []byte           { return nil }
func (b *mappedTreeBase) poolLen() int           { return 0 }
func (b *mappedTreeBase) usedCount(bool) uint64  { return 0 }

func (b *mappedTreeBase) slot(uint32) treeSlot              { return treeSlot{} }
func (b *mappedTreeBase) header() []byte                    { return nil }
func (b *mappedTreeBase) bits(uint32) uint32                { return 0 }
func (b *mappedTreeBase) nameOff(uint32) uint32             { return 0 }
func (b *mappedTreeBase) nameEqualFold(uint32, string) bool { return false }

func mapSpillOutput(*os.File, int) ([]byte, error) {
	return nil, cacheInvalid("mapped index unavailable")
}
func unmapSpillOutput([]byte) {}

func mapDiskIndex(string) (*mappedDiskIndex, error) {
	return nil, cacheInvalid("mapped index unavailable")
}
func (m *mappedDiskIndex) close()        {}
func (m *mappedDiskIndex) releasePages() {}
func (m *mappedDiskIndex) bytes() []byte { return nil }

func decodeSpillIndex(index []byte, volume VolumeID) (*decodedDiskIndex, error) {
	return decodeDiskIndex(index, volume)
}
