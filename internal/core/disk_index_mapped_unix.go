//go:build darwin || linux

package core

import (
	"os"

	"golang.org/x/sys/unix"
)

// mappedDiskIndex keeps a v7 base mapped instead of copying every 24-byte record
// into a separate heap allocation. The mapping stays attached to the loaded tree
// and is the read buffer for its base nodes. Linux is included so the contract
// can be tested; the product index is macOS.
type mappedDiskIndex struct {
	file *os.File
	data []byte
}

// mappedTreeBase is the read-only view a loaded tree keeps of its v7 file.
type mappedTreeBase struct {
	mapped   *mappedDiskIndex
	file     []byte
	entryOff int
}

func (b *mappedTreeBase) slot(idx uint32) treeSlot {
	record := b.file[b.entryOff+int(idx)*24:][:24]
	slot, err := cacheReadRecord(record, b.pool())
	if err != nil {
		return treeSlot{}
	}
	return slot.slot
}

func (b *mappedTreeBase) pool() []byte {
	off, n := int(cacheU32(b.file, 80)), int(cacheU32(b.file, 16))
	if off < 0 || n < 0 || off+n > len(b.file) {
		return nil
	}
	return b.file[off : off+n]
}

func mapDiskIndex(path string) (*mappedDiskIndex, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	info, err := f.Stat()
	if err != nil || info.Size() < 128 {
		f.Close()
		if err == nil {
			err = cacheInvalid("unknown or incomplete base")
		}
		return nil, err
	}
	data, err := unix.Mmap(int(f.Fd()), 0, int(info.Size()), unix.PROT_READ, unix.MAP_SHARED)
	if err != nil {
		f.Close()
		return nil, err
	}
	return &mappedDiskIndex{file: f, data: data}, nil
}

// mapSpillOutput maps a freshly truncated file for the streaming v7 build.
// Writing into the mapping is the point: the name pool never exists twice.
func mapSpillOutput(file *os.File, size int) ([]byte, error) {
	if err := file.Truncate(int64(size)); err != nil {
		return nil, err
	}
	return unix.Mmap(int(file.Fd()), 0, size, unix.PROT_READ|unix.PROT_WRITE, unix.MAP_SHARED)
}

func unmapSpillOutput(buf []byte) {
	if buf != nil {
		_ = unix.Munmap(buf)
	}
}

func (m *mappedDiskIndex) close() {
	if m == nil {
		return
	}
	if m.data != nil {
		_ = unix.Munmap(m.data)
		m.data = nil
	}
	if m.file != nil {
		m.file.Close()
		m.file = nil
	}
}

func (m *mappedDiskIndex) bytes() []byte {
	if m == nil {
		return nil
	}
	return m.data
}
