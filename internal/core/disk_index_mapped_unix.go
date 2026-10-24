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

// attachMappedBase drops the decoded heap copy and serves those nodes from the
// mapped file. entryOff is the file offset of the first 24-byte record.
func (t *SizeTree) attachMappedBase(mapped *mappedDiskIndex, file []byte, entryOff int) {
	if t == nil || mapped == nil || entryOff < 0 || entryOff+len(t.entries)*24 > len(file) {
		return
	}
	t.base = &mappedTreeBase{mapped: mapped, file: file, entryOff: entryOff}
	t.baseCount = len(t.entries)
	t.entries = nil
}

func (b *mappedTreeBase) slot(idx uint32) treeSlot {
	record := b.file[b.entryOff+int(idx)*24:][:24]
	slot, err := cacheReadRecord(record, b.pool())
	if err != nil {
		return treeSlot{}
	}
	return slot.slot
}

// header is the checksum-verified 128-byte v7 header of the mapped file.
func (b *mappedTreeBase) header() []byte { return b.file[:128] }

// bits and nameOff read one record's packed parent/flags word and name-pool
// offset without decoding its name.
func (b *mappedTreeBase) bits(idx uint32) uint32 {
	return cacheU32(b.file, b.entryOff+int(idx)*24)
}
func (b *mappedTreeBase) nameOff(idx uint32) uint32 {
	return cacheU32(b.file, b.entryOff+int(idx)*24+4)
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
		// Like Rust's MmapOut: hand dirty pages back before unmapping, or the
		// physical footprint stays at its peak. The data stays in the file.
		_ = unix.Madvise(buf, unix.MADV_DONTNEED)
		_ = unix.Munmap(buf)
	}
}

// releasePages drops the pages a full decode touched, as Rust does after
// validating its mapping. The view is read-only and file-backed, so later
// reads fault them back in. Errors are ignored: this is only advice.
func (m *mappedDiskIndex) releasePages() {
	if m == nil || m.data == nil {
		return
	}
	_ = unix.Madvise(m.data, unix.MADV_DONTNEED)
	_ = unix.Madvise(m.data, unix.MADV_FREE)
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
