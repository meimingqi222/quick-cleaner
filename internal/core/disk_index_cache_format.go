package core

import (
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"math"
	"os"
	"path/filepath"
	"sort"
	"strings"
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
func validDiskIndexName(name string) bool {
	return name != "" && name != "." && name != ".." &&
		!strings.ContainsAny(name, "/\x00") &&
		(filepath.Separator != '\\' || !strings.ContainsRune(name, '\\'))
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
			return cacheInvalid("entry exceeds v7 representation")
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
	mapped, err := mapSpillOutput(out, reserve)
	if err != nil {
		out.Close()
		return nil, err
	}
	written, err := fillSpillIndex(mapped, file, start, layout)
	unmapSpillOutput(mapped)
	if err != nil {
		out.Close()
		return nil, err
	}
	if err = out.Truncate(int64(written)); err != nil {
		out.Close()
		return nil, err
	}
	if err = out.Close(); err != nil {
		return nil, err
	}
	index, err := readDiskIndexRegular(tmp)
	if err != nil {
		return nil, err
	}
	decoded, err := decodeDiskIndex(index, volume)
	if err != nil {
		return nil, err
	}
	decoded.tree.SetBlockedSubtrees(blocked)
	if err = publishDiskIndexCovered(base, index, decoded.tree, 0, lastEventID, false); err != nil {
		return nil, err
	}
	if err = os.Remove(delta); err != nil && !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}
	return &ScanResult{Volume: volume, Tree: decoded.tree, TotalSize: decoded.tree.SizeOf(0), FileCount: decoded.tree.CountUsedFiles(), DirCount: decoded.tree.CountUsedDirs()}, nil
}

func countSpillChildren(file *os.File, children []uint32) error {
	if _, err := file.Seek(0, io.SeekStart); err != nil {
		return err
	}
	for {
		entry, err := readSpillRecord(file)
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
	writeSpillRecord(buf[layout.entOff:layout.entOff+24], 0, spillIntern(buf, &cursor, layout.poolOff, names, layout.rootName), true, 0, 0, 0)
	if _, err := file.Seek(0, io.SeekStart); err != nil {
		return 0, err
	}
	for index := 1; ; index++ {
		entry, err := readSpillRecord(file)
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
	for i := 0; i < layout.n; i++ {
		slot := buf[layout.entOff+i*24:][:24]
		if cacheU32(slot, 0)&cacheDirBit != 0 {
			continue
		}
		addSize := cacheU64(slot, 8)
		cur := cacheU32(slot, 0) & cacheParentMask
		for {
			parent := buf[layout.entOff+int(cur)*24:][:24]
			cachePut64(parent, 8, cacheU64(parent, 8)+addSize)
			cachePut32(parent, 20, cacheU32(parent, 20)+1)
			if cur == 0 {
				break
			}
			cur = cacheU32(parent, 0) & cacheParentMask
		}
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
func spillIntern(buf []byte, cursor *int, poolOff int, names map[uint64]uint32, name string) uint32 {
	hash := fnv1a64([]byte(name))
	if off, ok := names[hash]; ok {
		existing := buf[poolOff+int(off):]
		n := int(binary.LittleEndian.Uint16(existing))
		if n == len(name) && string(existing[2:2+n]) == name {
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
	Name   string
}

func readSpillRecord(file *os.File) (spillRecord, error) {
	var header [19]byte
	if _, err := io.ReadFull(file, header[:]); err != nil {
		return spillRecord{}, err
	}
	n := int(binary.LittleEndian.Uint16(header[17:19]))
	name := make([]byte, n)
	if _, err := io.ReadFull(file, name); err != nil {
		return spillRecord{}, err
	}
	return spillRecord{
		Parent: binary.LittleEndian.Uint32(header[0:4]),
		IsDir:  header[4] != 0,
		Mtime:  binary.LittleEndian.Uint32(header[5:9]),
		Size:   binary.LittleEndian.Uint64(header[9:17]),
		Name:   string(name),
	}, nil
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
	po := (128 + len(v.Mount) + len(v.Label) + 7) & ^7
	eo := (po + len(pool) + 7) & ^7
	cs := eo + n*24
	ca := cs + (n+1)*4
	length := ca + (n-1)*4
	if uint64(length) > math.MaxUint32 {
		return nil, cacheInvalid("base exceeds v7 offsets")
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
