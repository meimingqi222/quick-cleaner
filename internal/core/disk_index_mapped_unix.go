//go:build darwin || linux

package core

import (
	"encoding/binary"
	"os"
	"runtime"

	"golang.org/x/sys/unix"
)

// mappedDiskIndex keeps a v7 base mapped instead of copying every 24-byte record
// into a separate heap allocation. The mapping stays attached to the loaded tree
// and is the read buffer for its base nodes. Linux is included so the contract
// can be tested; the product index is macOS.
type mappedDiskIndex struct {
	file    *os.File
	data    []byte
	cleanup runtime.Cleanup
}

type mappedIndexResources struct {
	file *os.File
	data []byte
}

func releaseMappedIndex(r mappedIndexResources) {
	_ = unix.Munmap(r.data)
	_ = r.file.Close()
}

// mappedTreeBase is the read-only view a loaded tree keeps of its v7 file.
type mappedTreeBase struct {
	mapped         *mappedDiskIndex
	file           []byte
	entryOff       int
	sortedChildren map[uint32][]uint32
	tagNodes       []uint32
}

func newMappedSizeTree(mapped *mappedDiskIndex, file []byte, volume VolumeID, n, entryOff int) *SizeTree {
	return &SizeTree{volume: volume, base: &mappedTreeBase{mapped: mapped, file: file, entryOff: entryOff}, baseCount: n, childNames: make(map[uint32]map[string]uint32)}
}

func (b *mappedTreeBase) rememberSortedChildren(parent uint32, children []uint32) {
	if b.sortedChildren == nil {
		b.sortedChildren = make(map[uint32][]uint32)
	}
	b.sortedChildren[parent] = children
}
func (b *mappedTreeBase) rememberCacheTag(idx uint32) { b.tagNodes = append(b.tagNodes, idx) }
func (b *mappedTreeBase) cacheTagNodes() []uint32     { return b.tagNodes }
func (b *mappedTreeBase) children(parent uint32) treeChildView {
	if b.sortedChildren != nil {
		if children, ok := b.sortedChildren[parent]; ok {
			return treeChildView{slots: children}
		}
	}
	cs, ca := int(cacheU32(b.file, 88)), int(cacheU32(b.file, 92))
	start, stop := int(cacheU32(b.file, cs+int(parent)*4)), int(cacheU32(b.file, cs+(int(parent)+1)*4))
	return treeChildView{data: b.file[ca+start*4 : ca+stop*4], owner: b.mapped}
}

func (b *mappedTreeBase) values(idx uint32) treeSlot {
	raw := b.file[b.entryOff+int(idx)*24:][:24]
	bits := cacheU32(raw, 0)
	slot := treeSlot{TreeIndexEntry: TreeIndexEntry{Parent: bits & cacheParentMask, Used: bits&cacheUsedBit != 0, IsDir: bits&cacheDirBit != 0, Size: cacheU64(raw, 8), Mtime: uint64(cacheU32(raw, 16))}, files: uint64(cacheU32(raw, 20))}
	runtime.KeepAlive(b.mapped)
	return slot
}
func (b *mappedTreeBase) name(idx uint32) string {
	defer runtime.KeepAlive(b.mapped)
	pool := b.pool()
	off := int(b.nameOff(idx))
	n := int(binary.LittleEndian.Uint16(pool[off:]))
	return string(pool[off+2 : off+2+n])
}
func (b *mappedTreeBase) slot(idx uint32) treeSlot {
	slot := b.values(idx)
	slot.Name = b.name(idx)
	return slot
}

// Header callers may outlive their tree. Copy these few bytes so no raw mapped
// slice can survive without keeping the mapping owner reachable.
func (b *mappedTreeBase) header() []byte {
	header := make([]byte, 128)
	copy(header, b.file[:128])
	runtime.KeepAlive(b.mapped)
	return header
}

// bits and nameOff read one record's packed parent/flags word and name-pool
// offset without decoding its name.
func (b *mappedTreeBase) bits(idx uint32) uint32 {
	bits := cacheU32(b.file, b.entryOff+int(idx)*24)
	runtime.KeepAlive(b.mapped)
	return bits
}
func (b *mappedTreeBase) nameOff(idx uint32) uint32 {
	off := cacheU32(b.file, b.entryOff+int(idx)*24+4)
	runtime.KeepAlive(b.mapped)
	return off
}

func (b *mappedTreeBase) nameEqualFold(idx uint32, want string) bool {
	defer runtime.KeepAlive(b.mapped)
	pool := b.pool()
	off := uint64(b.nameOff(idx))
	prefix, err := cacheRange(pool, off, 2)
	if err != nil {
		return false
	}
	n := uint64(binary.LittleEndian.Uint16(prefix))
	if n != uint64(len(want)) {
		return false
	}
	name, err := cacheRange(pool, off+2, n)
	return err == nil && asciiEqFold(string(name), want)
}

func (b *mappedTreeBase) pool() []byte {
	off, n := int(cacheU32(b.file, 80)), int(cacheU32(b.file, 16))
	if off < 0 || n < 0 || off+n > len(b.file) {
		return nil
	}
	return b.file[off : off+n]
}

func (b *mappedTreeBase) poolLen() int {
	n := int(cacheU32(b.file, 16))
	runtime.KeepAlive(b.mapped)
	return n
}

func (b *mappedTreeBase) usedCount(directory bool) uint64 {
	off := 32
	if directory {
		off = 40
	}
	n := cacheU64(b.file, off)
	runtime.KeepAlive(b.mapped)
	return n
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
	mapped := &mappedDiskIndex{file: f, data: data}
	// The cleanup argument must not point back to mapped, or it would keep
	// its own owner reachable forever. Borrowed spill wrappers have no cleanup.
	mapped.cleanup = runtime.AddCleanup(mapped, releaseMappedIndex, mappedIndexResources{f, data})
	return mapped, nil
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
	runtime.KeepAlive(m)
}

func (m *mappedDiskIndex) close() {
	if m == nil {
		return
	}
	m.cleanup.Stop()
	if m.data != nil {
		_ = unix.Munmap(m.data)
		m.data = nil
	}
	if m.file != nil {
		m.file.Close()
		m.file = nil
	}
	runtime.KeepAlive(m)
}

func (m *mappedDiskIndex) bytes() []byte {
	if m == nil {
		return nil
	}
	return m.data
}

// The builder owns this writable mapping and unmaps it before publication;
// no SizeTree returned from this validation may escape that scope.
func decodeSpillIndex(index []byte, volume VolumeID) (*decodedDiskIndex, error) {
	return decodeDiskIndexWithMapping(index, volume, &mappedDiskIndex{data: index})
}
