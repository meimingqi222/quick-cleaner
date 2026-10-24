package platform

import (
	"encoding/binary"
	"math"
	"sort"
	"unicode/utf8"
)

var mftLE = binary.LittleEndian

const mftRefMask uint64 = 0x0000ffffffffffff
const mftMaxAttrList = 16 * 1024 * 1024

type mftRun struct {
	VCN, LCN, Clusters uint64
	Sparse             bool
}

// Run offsets are signed deltas local to each attribute, not absolute LCNs.
// Sparse runs consume VCN space without changing the previous physical LCN.
func mftParseRuns(b []byte, vcn uint64) ([]mftRun, error) {
	var out []mftRun
	var lcn int64
	for p := 0; p < len(b); {
		h := b[p]
		p++
		if h == 0 {
			return out, nil
		}
		n, o := int(h&15), int(h>>4)
		if n == 0 || n > 8 || o > 8 || n+o > len(b)-p {
			return nil, ErrMFTCorrupt
		}
		var count, raw uint64
		for i := 0; i < n; i++ {
			count |= uint64(b[p+i]) << uint(8*i)
		}
		p += n
		if count == 0 || count > math.MaxUint64-vcn {
			return nil, ErrMFTCorrupt
		}
		r := mftRun{VCN: vcn, Clusters: count, Sparse: o == 0}
		if o > 0 {
			for i := 0; i < o; i++ {
				raw |= uint64(b[p+i]) << uint(8*i)
			}
			p += o
			shift := uint(64 - o*8)
			delta := int64(raw<<shift) >> shift
			if (delta > 0 && lcn > math.MaxInt64-delta) || (delta < 0 && lcn < math.MinInt64-delta) {
				return nil, ErrMFTCorrupt
			}
			lcn += delta
			if lcn < 0 {
				return nil, ErrMFTCorrupt
			}
			r.LCN = uint64(lcn)
		}
		out = append(out, r)
		vcn += count
	}
	return nil, ErrMFTCorrupt
}

func mftFixup(b []byte, sector uint64) error {
	if len(b) < 0x30 || sector < 2 || uint64(len(b))%sector != 0 {
		return ErrMFTCorrupt
	}
	o, n := int(mftLE.Uint16(b[4:])), int(mftLE.Uint16(b[6:]))
	if n != len(b)/int(sector)+1 || o < 8 || o > len(b)-n*2 {
		return ErrMFTCorrupt
	}
	for i := 1; i < n; i++ {
		end := i * int(sector)
		if mftLE.Uint16(b[end-2:]) != mftLE.Uint16(b[o:]) {
			return ErrMFTCorrupt
		}
	}
	for i := 1; i < n; i++ {
		copy(b[i*int(sector)-2:], b[o+i*2:o+i*2+2])
	}
	return nil
}

type mftAttribute struct {
	typ         uint32
	nonresident bool
	named       bool
	b           []byte
}

func mftAttributes(rec []byte) ([]mftAttribute, error) {
	var out []mftAttribute
	err := mftEachAttr(rec, func(a mftAttribute) error {
		out = append(out, a)
		return nil
	})
	if err != nil {
		return nil, err
	}
	return out, nil
}

// mftEachAttr walks one record without allocating the attribute slice. The
// scan used to allocate that slice once per file.
func mftEachAttr(rec []byte, fn func(mftAttribute) error) error {
	if len(rec) < 0x30 || string(rec[:4]) != "FILE" {
		return ErrMFTCorrupt
	}
	pos := int(mftLE.Uint16(rec[0x14:]))
	if pos < 0x30 || pos > len(rec)-4 {
		return ErrMFTCorrupt
	}
	for pos <= len(rec)-4 {
		typ := mftLE.Uint32(rec[pos:])
		if typ == 0xffffffff {
			return nil
		}
		if pos > len(rec)-16 {
			return ErrMFTCorrupt
		}
		a := rec[pos:]
		size := uint64(mftLE.Uint32(a[4:]))
		min := uint64(24)
		if a[8] > 1 {
			return ErrMFTCorrupt
		}
		if a[8] == 1 {
			min = 64
		}
		if size < min || size > uint64(len(a)) {
			return ErrMFTCorrupt
		}
		a = a[:int(size)]
		if a[9] > 0 {
			o := uint64(mftLE.Uint16(a[10:]))
			if o < min || o+uint64(a[9])*2 > size {
				return ErrMFTCorrupt
			}
		}
		if a[8] == 0 {
			o, l := uint64(mftLE.Uint16(a[20:])), uint64(mftLE.Uint32(a[16:]))
			if o < min || o+l > size {
				return ErrMFTCorrupt
			}
		}
		if err := fn(mftAttribute{typ, a[8] == 1, a[9] != 0, a}); err != nil {
			return err
		}
		pos += int(size)
	}
	return ErrMFTCorrupt
}
func (a mftAttribute) value() []byte {
	o, l := int(mftLE.Uint16(a.b[20:])), int(mftLE.Uint32(a.b[16:]))
	return a.b[o : o+l]
}
func (a mftAttribute) runs() ([]mftRun, error) {
	if !a.nonresident {
		return nil, ErrMFTCorrupt
	}
	o := int(mftLE.Uint16(a.b[32:]))
	if o < 64 || o >= len(a.b) {
		return nil, ErrMFTCorrupt
	}
	return mftParseRuns(a.b[o:], mftLE.Uint64(a.b[16:]))
}
func mftDataRuns(rec []byte) ([]mftRun, error) {
	attrs, err := mftAttributes(rec)
	if err != nil {
		return nil, err
	}
	var out []mftRun
	for _, a := range attrs {
		if a.typ == 0x80 && !a.named && a.nonresident {
			runs, e := a.runs()
			if e != nil {
				return nil, e
			}
			out = append(out, runs...)
		}
	}
	return out, nil
}
func mftOrderRuns(runs []mftRun) error {
	sort.Slice(runs, func(i, j int) bool { return runs[i].VCN < runs[j].VCN })
	var end uint64
	for _, r := range runs {
		if r.VCN < end || r.Clusters > math.MaxUint64-r.VCN {
			return ErrMFTCorrupt
		}
		end = r.VCN + r.Clusters
	}
	return nil
}
func mftListRecords(b []byte) ([]uint64, error) {
	var out []uint64
	seen := map[uint64]bool{}
	for len(b) > 0 {
		if len(b) < 24 {
			return nil, ErrMFTCorrupt
		}
		n := int(mftLE.Uint16(b[4:]))
		if n < 24 || n > len(b) {
			return nil, ErrMFTCorrupt
		}
		if mftLE.Uint32(b) == 0x80 && b[6] == 0 {
			r := mftLE.Uint64(b[16:]) & mftRefMask
			if r != 0 && !seen[r] {
				out = append(out, r)
				seen[r] = true
			}
		}
		b = b[n:]
	}
	return out, nil
}

// mftDecodeScratch reuses the UTF-16 scratch and the parent arena across a
// chunk. LinkParents subslices set cap to len so a later append cannot
// overwrite the next record's parents.
type mftDecodeScratch struct {
	parents []uint64
	batch   []uint64
	name    []byte
}

func mftDecodeRecord(rec []byte) (MFTEntry, error) {
	var scratch mftDecodeScratch
	return mftDecodeRecordScratch(rec, &scratch)
}

func mftDecodeRecordScratch(rec []byte, scratch *mftDecodeScratch) (MFTEntry, error) {
	if err := mftEachAttr(rec, func(mftAttribute) error { return nil }); err != nil {
		return MFTEntry{}, err
	}
	flags := mftLE.Uint16(rec[22:])
	if flags&1 == 0 {
		return MFTEntry{}, nil
	}
	var e MFTEntry
	e.IsDir = flags&2 != 0
	e.BaseRecord = mftLE.Uint64(rec[32:]) & mftRefMask
	best := 4
	got := false
	scratch.batch = scratch.batch[:0]
	err := mftEachAttr(rec, func(a mftAttribute) error {
		switch {
		case a.typ == 0x10 && !a.nonresident:
			v := a.value()
			if len(v) >= 16 {
				ft := mftLE.Uint64(v[8:])
				if ft > 116444736000000000 {
					e.Mtime = (ft - 116444736000000000) / 10000000
				}
			}
		case a.typ == 0x30 && !a.nonresident:
			v := a.value()
			if len(v) < 66 || int(v[64])*2 > len(v)-66 {
				return ErrMFTCorrupt
			}
			parent := mftLE.Uint64(v) & mftRefMask
			found := false
			for _, p := range scratch.batch {
				if p == parent {
					found = true
				}
			}
			if !found {
				scratch.batch = append(scratch.batch, parent)
			}
			rank := 3
			switch v[65] {
			case 1:
				rank = 0
			case 3:
				rank = 1
			case 0:
				rank = 2
			}
			if rank < best {
				best = rank
				e.Parent = parent
				units := int(v[64])
				scratch.name = mftAppendUTF16(scratch.name[:0], v[66:66+units*2])
				e.Name = string(scratch.name)
				got = true
			}
		case a.typ == 0x80 && !a.named:
			if a.nonresident {
				if mftLE.Uint64(a.b[16:]) == 0 {
					e.Size = mftLE.Uint64(a.b[48:])
				}
			} else {
				e.Size = uint64(len(a.value()))
			}
		}
		return nil
	})
	if err != nil {
		return MFTEntry{}, err
	}
	if len(scratch.batch) > 0 {
		start := len(scratch.parents)
		scratch.parents = append(scratch.parents, scratch.batch...)
		end := len(scratch.parents)
		e.LinkParents = scratch.parents[start:end:end]
	}
	if e.BaseRecord != 0 {
		return e, nil
	}
	if !got {
		return MFTEntry{}, ErrMFTCorrupt
	}
	e.Used = true
	if e.IsDir {
		e.Size = 0
	}
	return e, nil
}

func mftAppendUTF16(dst, raw []byte) []byte {
	for i := 0; i+1 < len(raw); {
		u := uint16(raw[i]) | uint16(raw[i+1])<<8
		i += 2
		if u >= 0xD800 && u <= 0xDBFF && i+1 < len(raw) {
			u2 := uint16(raw[i]) | uint16(raw[i+1])<<8
			if u2 >= 0xDC00 && u2 <= 0xDFFF {
				r := 0x10000 + (uint32(u)-0xD800)<<10 + (uint32(u2) - 0xDC00)
				dst = utf8.AppendRune(dst, rune(r))
				i += 2
				continue
			}
		}
		dst = utf8.AppendRune(dst, rune(u))
	}
	return dst
}
