package platform

import (
	"encoding/binary"
	"math"
	"sort"
	"unicode/utf16"
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
	if len(rec) < 0x30 || string(rec[:4]) != "FILE" {
		return nil, ErrMFTCorrupt
	}
	pos := int(mftLE.Uint16(rec[0x14:]))
	if pos < 0x30 || pos > len(rec)-4 {
		return nil, ErrMFTCorrupt
	}
	var out []mftAttribute
	for pos <= len(rec)-4 {
		typ := mftLE.Uint32(rec[pos:])
		if typ == 0xffffffff {
			return out, nil
		}
		if pos > len(rec)-16 {
			return nil, ErrMFTCorrupt
		}
		a := rec[pos:]
		size := uint64(mftLE.Uint32(a[4:]))
		min := uint64(24)
		if a[8] > 1 {
			return nil, ErrMFTCorrupt
		}
		if a[8] == 1 {
			min = 64
		}
		if size < min || size > uint64(len(a)) {
			return nil, ErrMFTCorrupt
		}
		a = a[:int(size)]
		if a[9] > 0 {
			o := uint64(mftLE.Uint16(a[10:]))
			if o < min || o+uint64(a[9])*2 > size {
				return nil, ErrMFTCorrupt
			}
		}
		if a[8] == 0 {
			o, l := uint64(mftLE.Uint16(a[20:])), uint64(mftLE.Uint32(a[16:]))
			if o < min || o+l > size {
				return nil, ErrMFTCorrupt
			}
		}
		out = append(out, mftAttribute{typ, a[8] == 1, a[9] != 0, a})
		pos += int(size)
	}
	return nil, ErrMFTCorrupt
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
func mftDecodeRecord(rec []byte) (MFTEntry, error) {
	var e MFTEntry
	attrs, err := mftAttributes(rec)
	if err != nil {
		return e, err
	}
	flags := mftLE.Uint16(rec[22:])
	if flags&1 == 0 {
		return e, nil
	}
	e.IsDir = flags&2 != 0
	e.BaseRecord = mftLE.Uint64(rec[32:]) & mftRefMask
	best := 4
	got := false
	for _, a := range attrs {
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
				return MFTEntry{}, ErrMFTCorrupt
			}
			parent := mftLE.Uint64(v) & mftRefMask
			found := false
			for _, p := range e.LinkParents {
				if p == parent {
					found = true
				}
			}
			if !found {
				e.LinkParents = append(e.LinkParents, parent)
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
				units := make([]uint16, int(v[64]))
				for i := range units {
					units[i] = mftLE.Uint16(v[66+i*2:])
				}
				e.Name = string(utf16.Decode(units))
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
