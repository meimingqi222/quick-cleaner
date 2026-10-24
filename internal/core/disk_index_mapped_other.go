//go:build !darwin && !linux

package core

import "os"

type mappedDiskIndex struct{}

type mappedTreeBase struct{}

func (t *SizeTree) attachMappedBase(*mappedDiskIndex, []byte, int) {}

func (b *mappedTreeBase) slot(uint32) treeSlot  { return treeSlot{} }
func (b *mappedTreeBase) header() []byte        { return nil }
func (b *mappedTreeBase) bits(uint32) uint32    { return 0 }
func (b *mappedTreeBase) nameOff(uint32) uint32 { return 0 }

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
