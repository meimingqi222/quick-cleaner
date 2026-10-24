package core

import "sort"

// Deltas are cumulative relative to the immutable base, not chained patches.
func encodeDiskIndexDelta(t *SizeTree, base *decodedDiskIndex, meta DiskIndexMetadata) ([]byte, int, error) {
	bn := len(base.records)
	if t.nodeCount() < bn {
		return nil, 0, cacheInvalid("slots no longer match base")
	}
	if err := validateDiskTree(t, true); err != nil {
		return nil, 0, err
	}
	var overrides []int
	for i, r := range base.records {
		e := t.slot(uint32(i))
		if e.Parent != r.slot.Parent || e.Name != r.slot.Name {
			return nil, 0, cacheInvalid("base identity changed")
		}
		if e != r.slot {
			overrides = append(overrides, i)
		}
	}
	n := t.nodeCount() - bn
	pool := []byte{}
	names := map[string]uint32{}
	offs := make([]uint32, n)
	for i := 0; i < n; i++ {
		offs[i] = cacheIntern(&pool, names, t.slot(uint32(bn+i)).Name)
	}
	extra := map[uint32][]uint32{}
	// Rust overlays replace a parent's complete child slice, including its live
	// base children. Only parents with appended live nodes need an overlay.
	for i := bn; i < t.nodeCount(); i++ {
		e := t.slot(uint32(i))
		if e.Used {
			extra[e.Parent] = nil
		}
	}
	for i := 0; i < t.nodeCount(); i++ {
		e := t.slot(uint32(i))
		if i > 0 && e.Used {
			if _, ok := extra[e.Parent]; ok {
				extra[e.Parent] = append(extra[e.Parent], uint32(i))
			}
		}
	}
	parents := make([]uint32, 0, len(extra))
	count := n + len(overrides)
	length := 128 + n*24 + len(pool) + len(overrides)*28
	for p, kids := range extra {
		parents = append(parents, p)
		length += 8 + len(kids)*4
		count += len(kids)
	}
	sort.Slice(parents, func(i, j int) bool { return parents[i] < parents[j] })
	b := make([]byte, length)
	copy(b, "QCDLTV01")
	cachePut32(b, 8, 1)
	for off, val := range map[int]uint64{12: base.checksum, 20: meta.LastEventID, 28: meta.ScannedAt, 36: meta.FileCount, 44: meta.DirCount, 52: meta.TotalSize, 60: uint64(n), 68: uint64(len(pool)), 76: uint64(len(overrides)), 84: uint64(len(extra)), 92: uint64(length - 128)} {
		cachePut64(b, off, val)
	}
	cur := 128
	for i := 0; i < n; i++ {
		cacheWriteRecord(b[cur:cur+24], t.slot(uint32(bn+i)), offs[i])
		cur += 24
	}
	copy(b[cur:], pool)
	cur += len(pool)
	for _, idx := range overrides {
		cachePut32(b, cur, uint32(idx))
		cacheWriteRecord(b[cur+4:cur+28], t.slot(uint32(idx)), base.records[idx].nameOff)
		cur += 28
	}
	for _, p := range parents {
		kids := extra[p]
		cachePut32(b, cur, p)
		cachePut32(b, cur+4, uint32(len(kids)))
		cur += 8
		for _, k := range kids {
			cachePut32(b, cur, k)
			cur += 4
		}
	}
	cachePut64(b, 100, cacheChecksum(b, 100))
	return b, count, nil
}

func decodeDiskIndexDelta(b []byte, base *decodedDiskIndex) (*SizeTree, DiskIndexMetadata, error) {
	fail := func(reason string) (*SizeTree, DiskIndexMetadata, error) {
		return nil, DiskIndexMetadata{}, cacheInvalid(reason)
	}
	if len(b) < 128 || string(b[:8]) != "QCDLTV01" || cacheU32(b, 8) != 1 {
		return fail("unknown or incomplete delta")
	}
	if cacheU64(b, 12) != base.checksum || cacheU64(b, 100) == 0 || cacheChecksum(b, 100) != cacheU64(b, 100) {
		return fail("delta checksum/base binding")
	}
	if cacheU64(b, 92) != uint64(len(b)-128) {
		return fail("delta length")
	}
	meta := DiskIndexMetadata{LastEventID: cacheU64(b, 20), ScannedAt: cacheU64(b, 28), FileCount: cacheU64(b, 36), DirCount: cacheU64(b, 44), TotalSize: cacheU64(b, 52)}
	if meta.LastEventID < base.meta.LastEventID {
		return fail("delta watermark predates base")
	}
	n, pl, no, np := cacheU64(b, 60), cacheU64(b, 68), cacheU64(b, 76), cacheU64(b, 84)
	bn := uint64(len(base.records))
	total := bn + n
	if total < bn || total > uint64(cacheParentMask)+1 || n > uint64(len(b))/24 || no > uint64(len(b))/28 || np > uint64(len(b))/8 {
		return fail("delta counts")
	}
	cur := uint64(128)
	take := func(count uint64) ([]byte, error) {
		s, e := cacheRange(b, cur, count)
		if e == nil {
			cur += count
		}
		return s, e
	}
	raw, err := take(n * 24)
	if err != nil {
		return fail("truncated delta entries")
	}
	pool, err := take(pl)
	if err != nil {
		return fail("truncated delta pool")
	}
	// Transactional staging: neither base.tree nor any caller-owned tree is
	// mutated until the complete payload has passed structural validation.
	t := base.tree.Clone()
	for i := uint64(0); i < n; i++ {
		r, e := cacheReadRecord(raw[i*24:(i+1)*24], pool)
		if e != nil {
			return fail("delta name")
		}
		t.entries = append(t.entries, r.slot)
	}
	seenOverrides := make(map[uint32]bool)
	for i := uint64(0); i < no; i++ {
		bytes, e := take(28)
		if e != nil {
			return fail("truncated override")
		}
		idx := cacheU32(bytes, 0)
		if uint64(idx) >= bn || seenOverrides[idx] {
			return fail("duplicate/out of bounds override")
		}
		seenOverrides[idx] = true
		original := base.records[idx]
		if cacheU32(bytes, 8) != original.nameOff || cacheU32(bytes, 4)&cacheParentMask != original.slot.Parent {
			return fail("override identity")
		}
		// Override names are offsets in the BASE name pool, not the delta pool.
		bits := cacheU32(bytes, 4)
		slot := original.slot
		slot.Used = bits&cacheUsedBit != 0
		slot.IsDir = bits&cacheDirBit != 0
		slot.Size = cacheU64(bytes, 12)
		slot.Mtime = uint64(cacheU32(bytes, 20))
		slot.files = uint64(cacheU32(bytes, 24))
		t.setSlot(idx, slot)
	}
	extras := make(map[uint32]map[uint32]bool)
	for i := uint64(0); i < np; i++ {
		bytes, e := take(8)
		if e != nil {
			return fail("truncated overlay")
		}
		p, count := cacheU32(bytes, 0), uint64(cacheU32(bytes, 4))
		if !t.IsDir(p) || extras[p] != nil || count > total {
			return fail("overlay parent/count")
		}
		kids, e := take(count * 4)
		if e != nil {
			return fail("truncated overlay children")
		}
		unique := make(map[uint32]bool)
		for j := uint64(0); j < count; j++ {
			k := cacheU32(kids, int(j*4))
			if k == 0 || k == p || uint64(k) >= total || unique[k] || t.slot(k).Parent != p {
				return fail("invalid overlay child")
			}
			unique[k] = true
		}
		if uint64(p) < bn {
			for _, k := range base.tree.children[p] {
				if t.Valid(k) && !unique[k] {
					return fail("overlay omits base child")
				}
			}
		}
		extras[p] = unique
	}
	if cur != uint64(len(b)) {
		return fail("trailing delta bytes")
	}
	if err := validateDiskTree(t, true); err != nil {
		return nil, DiskIndexMetadata{}, err
	}
	for i := bn; i < total; i++ {
		e := t.slot(uint32(i))
		if e.Used && !extras[e.Parent][uint32(i)] {
			return fail("appended child missing from overlay")
		}
	}
	if meta.FileCount != t.CountUsedFiles() || meta.DirCount != t.CountUsedDirs() || meta.TotalSize != t.SizeOf(0) {
		return fail("delta statistics")
	}
	t.RebuildChildArrays()
	return t, meta, nil
}
