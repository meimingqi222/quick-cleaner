//go:build !darwin && !linux

package core

import "os"

type mappedDiskIndex struct{}

type mappedTreeBase struct{}

func (b *mappedTreeBase) slot(uint32) treeSlot { return treeSlot{} }

func mapSpillOutput(*os.File, int) ([]byte, error) {
	return nil, cacheInvalid("mapped index unavailable")
}
func unmapSpillOutput([]byte) {}

func mapDiskIndex(string) (*mappedDiskIndex, error) {
	return nil, cacheInvalid("mapped index unavailable")
}
func (m *mappedDiskIndex) close()        {}
func (m *mappedDiskIndex) bytes() []byte { return nil }
