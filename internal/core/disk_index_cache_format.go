package core

import (
	"bufio"
	"bytes"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"math"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"time"
	"unicode/utf8"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

const cacheParentMask uint32 = 0x3fffffff
const cacheUsedBit uint32 = 0x40000000
const cacheDirBit uint32 = 0x80000000

func cacheU32(b []byte, off int) uint32      { return binary.LittleEndian.Uint32(b[off : off+4]) }
func cacheU64(b []byte, off int) uint64      { return binary.LittleEndian.Uint64(b[off : off+8]) }
func cachePut32(b []byte, off int, v uint32) { binary.LittleEndian.PutUint32(b[off:off+4], v) }
func cachePut64(b []byte, off int, v uint64) { binary.LittleEndian.PutUint64(b[off:off+8], v) }
func cacheChecksum(b []byte, off int) uint64 {
	h := uint64(0xcbf29ce484222325)
	for i, v := range b {
		if i >= off && i < off+8 {
			v = 0
		}
		h = (h ^ uint64(v)) * 0x100000001b3
	}
	return h
}
func cacheInvalid(reason string) error { return fmt.Errorf("%w: %s", ErrDiskIndexCorrupt, reason) }
func cacheRange(b []byte, off, count uint64) ([]byte, error) {
	if off > uint64(len(b)) || count > uint64(len(b))-off {
		return nil, cacheInvalid("truncated section")
	}
	return b[off : off+count], nil
}

type cacheRecord struct {
	slot    treeSlot
	nameOff uint32
}
type decodedDiskIndex struct {
	tree     *SizeTree
	records  []cacheRecord
	meta     DiskIndexMetadata
	checksum uint64
}

func (d *decodedDiskIndex) recordCount() int {
	if d.records != nil {
		return len(d.records)
	}
	return d.tree.baseCount
}
func (d *decodedDiskIndex) record(idx uint32) cacheRecord {
	if d.records != nil {
		return d.records[idx]
	}
	return cacheRecord{slot: d.tree.base.slot(idx), nameOff: d.tree.base.nameOff(idx)}
}

func cacheReadRecord(b, pool []byte) (cacheRecord, error) {
	bits, off := cacheU32(b, 0), cacheU32(b, 4)
	prefix, err := cacheRange(pool, uint64(off), 2)
	if err != nil {
		return cacheRecord{}, err
	}
	name, err := cacheRange(pool, uint64(off)+2, uint64(binary.LittleEndian.Uint16(prefix)))
	if err != nil || !utf8.Valid(name) {
		return cacheRecord{}, cacheInvalid("invalid name pool")
	}
	return cacheRecord{treeSlot{TreeIndexEntry: TreeIndexEntry{Parent: bits & cacheParentMask, Name: string(name), IsDir: bits&cacheDirBit != 0, Used: bits&cacheUsedBit != 0, Size: cacheU64(b, 8), Mtime: uint64(cacheU32(b, 16))}, files: uint64(cacheU32(b, 20))}, off}, nil
}
func cacheWriteRecord(b []byte, e treeSlot, off uint32) {
	bits := e.Parent
	if e.Used {
		bits |= cacheUsedBit
	}
	if e.IsDir {
		bits |= cacheDirBit
	}
	cachePut32(b, 0, bits)
	cachePut32(b, 4, off)
	cachePut64(b, 8, e.Size)
	cachePut32(b, 16, uint32(e.Mtime))
	cachePut32(b, 20, uint32(e.files))
}

// A backslash is a filename character on Unix and a separator on Windows.
// Base and delta validation must agree with the host's path reconstruction.
func validDiskIndexName[T ~string | ~[]byte](name T) bool {
	if len(name) == 0 || (name[0] == '.' && (len(name) == 1 || (len(name) == 2 && name[1] == '.'))) {
		return false
	}
	for i := 0; i < len(name); i++ {
		if name[i] == '/' || name[i] == 0 || (filepath.Separator == '\\' && name[i] == '\\') {
			return false
		}
	}
	return true
}

func validateDiskTree(t *SizeTree, ordered bool) error {
	if t.nodeCount() == 0 || uint64(t.nodeCount()) > uint64(cacheParentMask)+1 || !t.IsDir(0) || t.slot(0).Parent != 0 {
		return cacheInvalid("invalid root")
	}
	// Iterative visitation handles acquisition trees with forward parents, while
	// rejecting cycles without recursion on attacker-controlled depth.
	state := make([]byte, t.nodeCount())
	state[0] = 2
	for i := 0; i < t.nodeCount(); i++ {
		e := t.slot(uint32(i))
		if e.Parent > cacheParentMask || e.Mtime > math.MaxUint32 || e.files > math.MaxUint32 || len(e.Name) > math.MaxUint16 || !utf8.ValidString(e.Name) {
			return cacheInvalid(fmt.Sprintf("entry %d exceeds v7 representation: parent=%d mtime=%d files=%d name_bytes=%d utf8=%t", i, e.Parent, e.Mtime, e.files, len(e.Name), utf8.ValidString(e.Name)))
		}
		if !e.Used {
			continue
		}
		if i > 0 && !validDiskIndexName(e.Name) {
			return cacheInvalid("unsafe name component")
		}
		if i > 0 && (!t.IsDir(e.Parent) || e.Parent == uint32(i) || (ordered && e.Parent >= uint32(i))) {
			return cacheInvalid("invalid parent")
		}
		var chain []uint32
		cur := uint32(i)
		for state[cur] == 0 {
			state[cur] = 1
			chain = append(chain, cur)
			p := t.slot(cur).Parent
			if !t.IsDir(p) {
				return cacheInvalid("invalid ancestor")
			}
			cur = p
		}
		if state[cur] == 1 {
			return cacheInvalid("parent cycle")
		}
		for _, idx := range chain {
			state[idx] = 2
		}
	}
	return nil
}

func decodeDiskIndex(b []byte, volume VolumeID) (*decodedDiskIndex, error) {
	return decodeDiskIndexWithMapping(b, volume, nil)
}

func decodeDiskIndexWithMapping(b []byte, volume VolumeID, mapped *mappedDiskIndex) (*decodedDiskIndex, error) {
	defer runtime.KeepAlive(mapped)
	if len(b) < 128 || string(b[:8]) != "QCIDXV07" || cacheU32(b, 8) != 7 {
		return nil, cacheInvalid("unknown or incomplete base")
	}
	checksum := cacheU64(b, 72)
	if checksum == 0 || cacheChecksum(b, 72) != checksum {
		return nil, cacheInvalid("base checksum")
	}
	n, poolLen, caLen := uint64(cacheU32(b, 12)), uint64(cacheU32(b, 16)), uint64(cacheU32(b, 20))
	ml, ll := uint64(cacheU32(b, 24)), uint64(cacheU32(b, 28))
	labels, err := cacheRange(b, 128, ml+ll)
	if err != nil || string(labels[:ml]) != volume.Mount || !utf8.Valid(labels) {
		return nil, cacheInvalid("mount/label mismatch")
	}
	po, eo, cs, ca := uint64(cacheU32(b, 80)), uint64(cacheU32(b, 84)), uint64(cacheU32(b, 88)), uint64(cacheU32(b, 92))
	if n == 0 || n > uint64(cacheParentMask)+1 || eo%8 != 0 || cs%4 != 0 || ca%4 != 0 {
		return nil, cacheInvalid("section alignment/count")
	}
	// Both Rust layouts are accepted, but no section may alias another or the header.
	type section struct{ off, size uint64 }
	sections := []section{{0, 128 + ml + ll}, {po, poolLen}, {eo, n * 24}, {cs, (n + 1) * 4}, {ca, caLen * 4}}
	sort.Slice(sections, func(i, j int) bool {
		if sections[i].off == sections[j].off {
			return sections[i].size < sections[j].size
		}
		return sections[i].off < sections[j].off
	})
	var end uint64
	for _, s := range sections {
		if s.off < end {
			return nil, cacheInvalid("overlapping sections")
		}
		if _, err := cacheRange(b, s.off, s.size); err != nil {
			return nil, err
		}
		end = s.off + s.size
	}
	pool, _ := cacheRange(b, po, poolLen)
	if mapped != nil {
		return decodeMappedDiskIndexBody(b, pool, volume, mapped, int(n), int(eo), int(cs), int(ca), caLen, checksum)
	}
	records := make([]cacheRecord, int(n))
	for i := range records {
		r, e := cacheReadRecord(b[int(eo)+i*24:int(eo)+(i+1)*24], pool)
		if e != nil {
			return nil, e
		}
		records[i] = r
		if !r.slot.Used {
			return nil, cacheInvalid("base tombstone")
		}
	}
	t := &SizeTree{volume: volume, entries: make([]treeSlot, int(n))}
	for i, r := range records {
		t.entries[i] = r.slot
	}
	if err := validateDiskTree(t, true); err != nil {
		return nil, err
	}
	if cacheU32(b, int(cs)) != 0 || uint64(cacheU32(b, int(cs+n*4))) != caLen {
		return nil, cacheInvalid("CSR sentinel")
	}
	seen := make([]bool, int(n))
	seen[0] = true
	for i := uint64(0); i < n; i++ {
		start, stop := uint64(cacheU32(b, int(cs+i*4))), uint64(cacheU32(b, int(cs+(i+1)*4)))
		if start > stop || stop > caLen {
			return nil, cacheInvalid("CSR range")
		}
		for j := start; j < stop; j++ {
			child := uint64(cacheU32(b, int(ca+j*4)))
			if child == 0 || child >= n || seen[child] || t.slot(uint32(child)).Parent != uint32(i) {
				return nil, cacheInvalid("CSR child")
			}
			seen[child] = true
		}
	}
	for _, s := range seen {
		if !s {
			return nil, cacheInvalid("missing CSR child")
		}
	}
	meta := DiskIndexMetadata{FileCount: cacheU64(b, 32), DirCount: cacheU64(b, 40), TotalSize: cacheU64(b, 48), LastEventID: cacheU64(b, 56), ScannedAt: cacheU64(b, 64)}
	if meta.FileCount != t.CountUsedFiles() || meta.DirCount != t.CountUsedDirs() {
		return nil, cacheInvalid("base statistics")
	}
	t.RebuildChildArrays()
	return &decodedDiskIndex{t, records, meta, checksum}, nil
}

// All sections and the whole-file checksum are checked before this view is
// created. Ordered parent links prove acyclicity without allocating heap slots.
func decodeMappedDiskIndexBody(b, pool []byte, volume VolumeID, mapped *mappedDiskIndex, n, eo, cs, ca int, childCount, checksum uint64) (*decodedDiskIndex, error) {
	t := newMappedSizeTree(mapped, b, volume, n, eo)
	var files, dirs uint64
	for i := 0; i < n; i++ {
		raw := b[eo+i*24 : eo+(i+1)*24]
		bits, off := cacheU32(raw, 0), uint64(cacheU32(raw, 4))
		prefix, err := cacheRange(pool, off, 2)
		if err != nil {
			return nil, err
		}
		name, err := cacheRange(pool, off+2, uint64(binary.LittleEndian.Uint16(prefix)))
		if err != nil || !utf8.Valid(name) {
			return nil, cacheInvalid("invalid name pool")
		}
		if bits&cacheUsedBit == 0 {
			return nil, cacheInvalid("base tombstone")
		}
		parent := bits & cacheParentMask
		if i == 0 {
			if parent != 0 || bits&cacheDirBit == 0 {
				return nil, cacheInvalid("invalid root")
			}
		} else {
			if !validDiskIndexName(name) {
				return nil, cacheInvalid("unsafe name component")
			}
			if parent >= uint32(i) || t.base.bits(parent)&(cacheUsedBit|cacheDirBit) != cacheUsedBit|cacheDirBit {
				return nil, cacheInvalid("invalid parent")
			}
		}
		if bits&cacheDirBit != 0 {
			dirs++
		} else {
			files++
			if len(name) == len("CACHEDIR.TAG") && isCacheDirTagName(string(name)) {
				t.base.rememberCacheTag(uint32(i))
			}
		}
	}
	if childCount != uint64(n-1) || cacheU32(b, cs) != 0 || uint64(cacheU32(b, cs+n*4)) != childCount {
		return nil, cacheInvalid("CSR sentinel")
	}
	seen := make([]uint64, (n+63)/64)
	for i := 0; i < n; i++ {
		start, stop := uint64(cacheU32(b, cs+i*4)), uint64(cacheU32(b, cs+(i+1)*4))
		if start > stop || stop > childCount {
			return nil, cacheInvalid("CSR range")
		}
		var previous uint32
		ordered := true
		for j := start; j < stop; j++ {
			child := cacheU32(b, ca+int(j)*4)
			if child == 0 || uint64(child) >= uint64(n) || seen[child/64]&(uint64(1)<<(child%64)) != 0 || t.base.bits(child)&cacheParentMask != uint32(i) {
				return nil, cacheInvalid("CSR child")
			}
			seen[child/64] |= uint64(1) << (child % 64)
			if child < previous {
				ordered = false
			}
			previous = child
		}
		if !ordered {
			// Older loaders rebuilt rows in slot order. Preserve that behavior
			// for valid files whose CSR rows were stored in another order.
			children := t.base.children(uint32(i)).copy()
			sort.Slice(children, func(a, b int) bool { return children[a] < children[b] })
			t.base.rememberSortedChildren(uint32(i), children)
		}
	}
	meta := DiskIndexMetadata{FileCount: cacheU64(b, 32), DirCount: cacheU64(b, 40), TotalSize: cacheU64(b, 48), LastEventID: cacheU64(b, 56), ScannedAt: cacheU64(b, 64)}
	if files != meta.FileCount || dirs != meta.DirCount {
		return nil, cacheInvalid("base statistics")
	}
	return &decodedDiskIndex{tree: t, meta: meta, checksum: checksum}, nil
}

type spillLayout struct {
	n                      int
	entOff, csOff, caOff   int
	poolOff                int
	mount, label, rootName string
	event                  uint64
}

// BuildMacWalkSpillIndex streams a spilled macOS scan into a v7 base. Two passes
// over the spill replace holding the raw entry array beside the finished tree:
// the first counts children, the second interns names straight into the mapped
// file and writes records. The layout is Rust's names-trailing layout, which the
// loader already accepts. Publishing goes through the same coverage path as Save.
func BuildMacWalkSpillIndex(cache *DiskIndexCache, volume VolumeID, rootName string, spill *platform.MacWalkSpill, blocked []string, lastEventID uint64) (*ScanResult, error) {
	if spill == nil || spill.Count() == 0 {
		return nil, platform.ErrMacWalkMalformed
	}
	n := int(spill.Count()) + 1
	if uint64(n) > uint64(cacheParentMask)+1 {
		return nil, cacheInvalid("base exceeds v7 offsets")
	}
	file, err := os.Open(spill.Path())
	if err != nil {
		return nil, err
	}
	defer file.Close()
	children := make([]uint32, n)
	if err = countSpillChildren(file, children); err != nil {
		return nil, err
	}
	start := make([]uint32, n)
	var childCount int
	for i, count := range children {
		start[i] = uint32(childCount)
		childCount += int(count)
	}
	layout := spillLayout{
		n: n, mount: volume.Mount, label: volume.Label, rootName: rootName, event: lastEventID,
		entOff: align8(128 + len(volume.Mount) + len(volume.Label)),
	}
	layout.csOff = layout.entOff + n*24
	layout.caOff = layout.csOff + (n+1)*4
	layout.poolOff = layout.caOff + childCount*4
	reserve := layout.poolOff + int(spill.NameBytes()) + len(rootName) + 2
	base, delta := cache.Paths(volume)
	if err = os.MkdirAll(filepath.Dir(base), 0700); err != nil {
		return nil, err
	}
	out, err := os.CreateTemp(filepath.Dir(base), ".scan-index-*.tmp")
	if err != nil {
		return nil, err
	}
	tmp := out.Name()
	defer os.Remove(tmp)
	defer out.Close()
	mapped, err := mapSpillOutput(out, reserve)
	if err != nil {
		out.Close()
		return nil, err
	}
	defer func() {
		if mapped != nil {
			unmapSpillOutput(mapped)
		}
	}()
	written, err := fillSpillIndex(mapped, file, start, layout)
	if err != nil {
		return nil, err
	}
	index := mapped[:written]
	decoded, err := decodeSpillIndex(index, volume)
	if err != nil {
		return nil, err
	}
	decoded.tree.SetBlockedSubtrees(blocked)
	coveragePath, coverage, err := prepareDiskIndexCoverage(base, index, decoded.tree, 0, lastEventID, false)
	if err != nil {
		return nil, err
	}
	// No reader escapes this writable mapping. After validation and coverage
	// preparation, publish the actual built file rather than copying it again.
	unmapSpillOutput(mapped)
	mapped = nil
	if err := out.Truncate(int64(written)); err != nil {
		return nil, err
	}
	if err := out.Sync(); err != nil {
		return nil, err
	}
	if err := out.Close(); err != nil {
		return nil, err
	}
	diskIndexWrites.Lock()
	defer diskIndexWrites.Unlock()
	if lastEventID < diskIndexWrites.watermarks[base] {
		return nil, ErrDiskIndexStale
	}
	if previous, err := cache.load(volume); err == nil && lastEventID < previous.LastEventID {
		return nil, ErrDiskIndexStale
	}
	// The immutable sidecar precedes the base rename, just as in Save. Never
	// remove it on a later error: the index rename may already have succeeded.
	if err := publishDiskIndex(coveragePath, coverage); err != nil {
		return nil, err
	}
	if err := os.Rename(tmp, base); err != nil {
		return nil, err
	}
	diskIndexWrites.watermarks[base] = lastEventID
	if err := syncDiskIndexDirectory(base); err != nil {
		return nil, err
	}
	if err := os.Remove(delta); err != nil && !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}
	loaded, err := cache.load(volume)
	if err != nil {
		return nil, err
	}
	return &loaded.Scan, nil
}

func countSpillChildren(file *os.File, children []uint32) error {
	if _, err := file.Seek(0, io.SeekStart); err != nil {
		return err
	}
	reader := newSpillRecordReader(file)
	for {
		entry, err := reader.next(false)
		if err == io.EOF {
			return nil
		}
		if err != nil {
			return err
		}
		if int(entry.Parent) >= len(children) {
			return platform.ErrMacWalkMalformed
		}
		children[entry.Parent]++
	}
}

func fillSpillIndex(buf []byte, file *os.File, start []uint32, layout spillLayout) (int, error) {
	copy(buf[128:], layout.mount)
	copy(buf[128+len(layout.mount):], layout.label)
	for i, at := range start {
		cachePut32(buf, layout.csOff+i*4, at)
	}
	cachePut32(buf, layout.csOff+layout.n*4, uint32(layout.poolOff-layout.caOff)/4)
	names := map[uint64]uint32{}
	cursor := layout.poolOff
	writeSpillRecord(buf[layout.entOff:layout.entOff+24], 0, spillIntern(buf, &cursor, layout.poolOff, names, []byte(layout.rootName)), true, 0, 0, 0)
	if _, err := file.Seek(0, io.SeekStart); err != nil {
		return 0, err
	}
	reader := newSpillRecordReader(file)
	for index := 1; ; index++ {
		entry, err := reader.next(true)
		if err == io.EOF {
			if index != layout.n {
				return 0, platform.ErrMacWalkMalformed
			}
			break
		}
		if err != nil {
			return 0, err
		}
		if index >= layout.n || int(entry.Parent) >= index {
			return 0, platform.ErrMacWalkMalformed
		}
		files := uint32(0)
		if !entry.IsDir {
			files = 1
		}
		writeSpillRecord(buf[layout.entOff+index*24:][:24], entry.Parent, spillIntern(buf, &cursor, layout.poolOff, names, entry.Name), entry.IsDir, entry.Size, entry.Mtime, files)
		pos := cacheU32(buf, layout.csOff+int(entry.Parent)*4)
		cachePut32(buf, layout.caOff+int(pos)*4, uint32(index))
		cachePut32(buf, layout.csOff+int(entry.Parent)*4, pos+1)
	}
	for i, at := range start {
		cachePut32(buf, layout.csOff+i*4, at)
	}
	for i := 0; i < layout.n; i++ {
		slot := buf[layout.entOff+i*24:][:24]
		if cacheU32(slot, 0)&cacheDirBit != 0 {
			cachePut64(slot, 8, 0)
			cachePut32(slot, 20, 0)
		}
	}
	// Parents precede children in the spill. Reverse slot order rolls each
	// complete subtree up once instead of walking every file's ancestors.
	for i := layout.n - 1; i > 0; i-- {
		slot := buf[layout.entOff+i*24:][:24]
		parentIdx := cacheU32(slot, 0) & cacheParentMask
		parent := buf[layout.entOff+int(parentIdx)*24:][:24]
		cachePut64(parent, 8, cacheU64(parent, 8)+cacheU64(slot, 8))
		cachePut32(parent, 20, cacheU32(parent, 20)+cacheU32(slot, 20))
	}
	var files, dirs uint64
	for i := 0; i < layout.n; i++ {
		if cacheU32(buf, layout.entOff+i*24)&cacheDirBit != 0 {
			dirs++
		} else {
			files++
		}
	}
	copy(buf[:8], "QCIDXV07")
	cachePut32(buf, 8, 7)
	cachePut32(buf, 12, uint32(layout.n))
	cachePut32(buf, 16, uint32(cursor-layout.poolOff))
	cachePut32(buf, 20, uint32(layout.n-1))
	cachePut32(buf, 24, uint32(len(layout.mount)))
	cachePut32(buf, 28, uint32(len(layout.label)))
	cachePut64(buf, 32, files)
	cachePut64(buf, 40, dirs)
	cachePut64(buf, 48, cacheU64(buf, layout.entOff+8))
	cachePut64(buf, 56, layout.event)
	cachePut64(buf, 64, uint64(time.Now().Unix()))
	cachePut32(buf, 80, uint32(layout.poolOff))
	cachePut32(buf, 84, uint32(layout.entOff))
	cachePut32(buf, 88, uint32(layout.csOff))
	cachePut32(buf, 92, uint32(layout.caOff))
	cachePut64(buf, 72, cacheChecksum(buf[:cursor], 72))
	return cursor, nil
}

// spillIntern appends a name to the mapped pool and returns its pool-relative
// offset. Lookup is by FNV-1a with a byte comparison, matching Rust's intern
// table, so identical names share one pool entry and a hash collision cannot.
func spillIntern(buf []byte, cursor *int, poolOff int, names map[uint64]uint32, name []byte) uint32 {
	hash := fnv1a64(name)
	if off, ok := names[hash]; ok {
		existing := buf[poolOff+int(off):]
		n := int(binary.LittleEndian.Uint16(existing))
		if n == len(name) && bytes.Equal(existing[2:2+n], name) {
			return off
		}
	}
	off := uint32(*cursor - poolOff)
	names[hash] = off
	binary.LittleEndian.PutUint16(buf[*cursor:], uint16(len(name)))
	*cursor += 2
	copy(buf[*cursor:], name)
	*cursor += len(name)
	return off
}

func fnv1a64(data []byte) uint64 {
	hash := uint64(0xcbf29ce484222325)
	for _, b := range data {
		hash = (hash ^ uint64(b)) * 0x100000001b3
	}
	return hash
}

func writeSpillRecord(b []byte, parent, nameOff uint32, dir bool, size uint64, mtime, files uint32) {
	bits := parent | cacheUsedBit
	if dir {
		bits |= cacheDirBit
	}
	cachePut32(b, 0, bits)
	cachePut32(b, 4, nameOff)
	cachePut64(b, 8, size)
	cachePut32(b, 16, mtime)
	cachePut32(b, 20, files)
}

type spillRecord struct {
	Parent uint32
	IsDir  bool
	Mtime  uint32
	Size   uint64
	// Name aliases the reader until its next call; interning copies it.
	Name []byte
}

type spillRecordReader struct {
	reader *bufio.Reader
	header [19]byte
	name   []byte
}

func newSpillRecordReader(reader io.Reader) *spillRecordReader {
	return &spillRecordReader{reader: bufio.NewReaderSize(reader, 256<<10)}
}
func (r *spillRecordReader) next(withName bool) (spillRecord, error) {
	if _, err := io.ReadFull(r.reader, r.header[:]); err != nil {
		return spillRecord{}, err
	}
	n := int(binary.LittleEndian.Uint16(r.header[17:]))
	var name []byte
	if withName {
		if cap(r.name) < n {
			r.name = make([]byte, n)
		} else {
			r.name = r.name[:n]
		}
		if _, err := io.ReadFull(r.reader, r.name); err != nil {
			if err == io.EOF {
				err = io.ErrUnexpectedEOF
			}
			return spillRecord{}, err
		}
		name = r.name
	} else {
		if _, err := r.reader.Discard(n); err != nil {
			if err == io.EOF {
				err = io.ErrUnexpectedEOF
			}
			return spillRecord{}, err
		}
	}
	return spillRecord{Parent: binary.LittleEndian.Uint32(r.header[:4]), IsDir: r.header[4] != 0, Mtime: binary.LittleEndian.Uint32(r.header[5:9]), Size: binary.LittleEndian.Uint64(r.header[9:17]), Name: name}, nil
}

func align8(v int) int { return (v + 7) &^ 7 }

func cacheIntern(pool *[]byte, names map[string]uint32, name string) uint32 {
	if off, ok := names[name]; ok {
		return off
	}
	off := uint32(len(*pool))
	*pool = append(*pool, byte(len(name)), byte(len(name)>>8))
	*pool = append(*pool, name...)
	names[name] = off
	return off
}

func encodeDiskIndex(t *SizeTree, v VolumeID, meta DiskIndexMetadata) ([]byte, error) {
	if b, ordered, err := encodeOrderedDiskIndex(t, v, meta); ordered {
		return b, err
	}
	return encodeUnorderedDiskIndex(t, v, meta)
}

func encodeUnorderedDiskIndex(t *SizeTree, v VolumeID, meta DiskIndexMetadata) ([]byte, error) {
	// Preserve slot/search order when already parent-first. Only acquisition
	// trees with forward parents require a topological reordering.
	children := make([][]uint32, t.nodeCount())
	ordered := true
	for i := 0; i < t.nodeCount(); i++ {
		e := t.slot(uint32(i))
		if i > 0 && e.Used {
			children[e.Parent] = append(children[e.Parent], uint32(i))
			if e.Parent >= uint32(i) {
				ordered = false
			}
		}
	}
	stack := []uint32{0}
	order := make([]uint32, 0, t.nodeCount())
	remap := make([]uint32, t.nodeCount())
	for len(stack) > 0 {
		idx := stack[len(stack)-1]
		stack = stack[:len(stack)-1]
		remap[idx] = uint32(len(order))
		order = append(order, idx)
		for j := len(children[idx]) - 1; j >= 0; j-- {
			stack = append(stack, children[idx][j])
		}
	}
	if ordered {
		sort.Slice(order, func(i, j int) bool { return order[i] < order[j] })
		for i, idx := range order {
			remap[idx] = uint32(i)
		}
	}
	pool := []byte{}
	names := map[string]uint32{}
	offsets := make([]uint32, len(order))
	for i, idx := range order {
		offsets[i] = cacheIntern(&pool, names, t.slot(idx).Name)
	}
	n := len(order)
	b, eo, cs, ca, err := allocateDiskIndex(v, meta, pool, n)
	if err != nil {
		return nil, err
	}
	cursor := 0
	for i, idx := range order {
		e := t.slot(idx)
		e.Parent = remap[e.Parent]
		cacheWriteRecord(b[eo+i*24:eo+(i+1)*24], e, offsets[i])
		cachePut32(b, cs+i*4, uint32(cursor))
		for _, c := range children[idx] {
			cachePut32(b, ca+cursor*4, remap[c])
			cursor++
		}
	}
	cachePut32(b, cs+n*4, uint32(cursor))
	cachePut64(b, 72, cacheChecksum(b, 72))
	return b, nil
}

func allocateDiskIndex(v VolumeID, meta DiskIndexMetadata, pool []byte, n int) ([]byte, int, int, int, error) {
	po := (128 + len(v.Mount) + len(v.Label) + 7) & ^7
	eo := (po + len(pool) + 7) & ^7
	cs := eo + n*24
	ca := cs + (n+1)*4
	length := ca + (n-1)*4
	if uint64(length) > math.MaxUint32 {
		return nil, 0, 0, 0, cacheInvalid("base exceeds v7 offsets")
	}
	b := make([]byte, length)
	copy(b, "QCIDXV07")
	cachePut32(b, 8, 7)
	for off, val := range map[int]int{12: n, 16: len(pool), 20: n - 1, 24: len(v.Mount), 28: len(v.Label), 80: po, 84: eo, 88: cs, 92: ca} {
		cachePut32(b, off, uint32(val))
	}
	for off, val := range map[int]uint64{32: meta.FileCount, 40: meta.DirCount, 48: meta.TotalSize, 56: meta.LastEventID, 64: meta.ScannedAt} {
		cachePut64(b, off, val)
	}
	copy(b[128:], v.Mount)
	copy(b[128+len(v.Mount):], v.Label)
	copy(b[po:], pool)
	return b, eo, cs, ca, nil
}

// Ordered acquisition and mapped snapshots already have stable parent-first
// slots. Compact in that order and use the output CSR as its own count table.
func encodeOrderedDiskIndex(t *SizeTree, v VolumeID, meta DiskIndexMetadata) ([]byte, bool, error) {
	order := make([]uint32, 0, t.nodeCount())
	remap := make([]uint32, t.nodeCount())
	for i := 0; i < t.nodeCount(); i++ {
		e := t.slotValues(uint32(i))
		if !e.Used {
			continue
		}
		if i > 0 && e.Parent >= uint32(i) {
			return nil, false, nil
		}
		remap[i] = uint32(len(order))
		order = append(order, uint32(i))
	}
	pool := []byte{}
	if t.base != nil {
		pool = make([]byte, 0, t.base.poolLen())
	}
	names := make(map[string]uint32)
	baseNames := make(map[uint32]uint32)
	offsets := make([]uint32, len(order))
	for i, idx := range order {
		if int(idx) < t.baseCount {
			if e, ok := t.overrides[idx]; ok {
				offsets[i] = cacheIntern(&pool, names, e.Name)
				continue
			}
			off := t.base.nameOff(idx)
			if mapped, ok := baseNames[off]; ok {
				offsets[i] = mapped
				continue
			}
			offsets[i] = cacheIntern(&pool, names, t.base.name(idx))
			baseNames[off] = offsets[i]
		} else {
			offsets[i] = cacheIntern(&pool, names, t.entries[int(idx)-t.baseCount].Name)
		}
	}
	n := len(order)
	b, eo, cs, ca, err := allocateDiskIndex(v, meta, pool, n)
	if err != nil {
		return nil, true, err
	}
	for i, idx := range order {
		e := t.slotValues(idx)
		e.Parent = remap[e.Parent]
		cacheWriteRecord(b[eo+i*24:eo+(i+1)*24], e, offsets[i])
		if i > 0 {
			at := cs + (int(e.Parent)+1)*4
			cachePut32(b, at, cacheU32(b, at)+1)
		}
	}
	for i := 0; i < n; i++ {
		cachePut32(b, cs+(i+1)*4, cacheU32(b, cs+i*4)+cacheU32(b, cs+(i+1)*4))
	}
	// Names are encoded now; reuse offsets for each parent's insertion cursor.
	for i := range offsets {
		offsets[i] = cacheU32(b, cs+i*4)
	}
	for i, idx := range order {
		if i == 0 {
			continue
		}
		parent := remap[t.slotValues(idx).Parent]
		cachePut32(b, ca+int(offsets[parent])*4, uint32(i))
		offsets[parent]++
	}
	cachePut64(b, 72, cacheChecksum(b, 72))
	return b, true, nil
}
