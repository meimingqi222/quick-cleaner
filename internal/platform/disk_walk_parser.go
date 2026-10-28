package platform

import (
	"bytes"
	"encoding/binary"
	"strings"
)

const (
	diskWalkName     uint32 = 1
	diskWalkType     uint32 = 8
	diskWalkModtime  uint32 = 0x400
	diskWalkFlags    uint32 = 0x40000
	diskWalkError    uint32 = 0x20000000
	diskWalkReturned uint32 = 0x80000000
	diskWalkAlloc    uint32 = 4
	diskWalkDataless uint32 = 0x40000000
)

type diskWalkItem struct {
	MacWalkEntry
	flagsKnown bool
}

func diskWalkValidName(s string) bool {
	return s != "" && s != "." && s != ".." && !strings.ContainsAny(s, "/\x00")
}

// Both supported Darwin architectures use little endian, 64-bit timespecs,
// and four-byte-packed attribute fields. NAME's signed offset is relative to
// its attrreference, not to the record. ERROR follows FLAGS, not NAME.
func diskWalkParseRecord(b []byte) (diskWalkItem, error) {
	var item diskWalkItem
	le := binary.LittleEndian
	if len(b) < 24 || uint64(le.Uint32(b)) != uint64(len(b)) {
		return item, ErrMacWalkMalformed
	}
	common, file := le.Uint32(b[4:]), le.Uint32(b[16:])
	if common&(diskWalkName|diskWalkType) != diskWalkName|diskWalkType {
		return item, ErrMacWalkMalformed
	}
	if common & ^(diskWalkName|diskWalkType|diskWalkModtime|diskWalkFlags|diskWalkError|diskWalkReturned) != 0 || file & ^diskWalkAlloc != 0 || le.Uint32(b[8:]) != 0 || le.Uint32(b[12:]) != 0 || le.Uint32(b[20:]) != 0 {
		return item, ErrMacWalkMalformed
	}
	p := 24
	take := func(n int) []byte {
		if n > len(b)-p {
			return nil
		}
		v := b[p : p+n]
		p += n
		return v
	}
	var nameStart, nameLen int64
	if common&diskWalkName != 0 {
		at := p
		v := take(8)
		if v == nil {
			return item, ErrMacWalkMalformed
		}
		nameStart = int64(at) + int64(int32(le.Uint32(v)))
		nameLen = int64(le.Uint32(v[4:]))
	}
	var typ uint32
	if common&diskWalkType != 0 {
		v := take(4)
		if v == nil {
			return item, ErrMacWalkMalformed
		}
		typ = le.Uint32(v)
		if typ < 1 || typ > 7 {
			return item, ErrMacWalkMalformed
		}
	}
	if common&diskWalkModtime != 0 {
		v := take(16)
		if v == nil {
			return item, ErrMacWalkMalformed
		}
		sec := int64(le.Uint64(v))
		if sec > 0 {
			item.Mtime = uint64(sec)
		}
	}
	if common&diskWalkFlags != 0 {
		v := take(4)
		if v == nil {
			return item, ErrMacWalkMalformed
		}
		item.Flags = le.Uint32(v)
		item.flagsKnown = true
	}
	if common&diskWalkError != 0 {
		v := take(4)
		if v == nil {
			return item, ErrMacWalkMalformed
		}
		item.AttributeError = le.Uint32(v)
	}
	if file&diskWalkAlloc != 0 {
		v := take(8)
		if v == nil || int64(le.Uint64(v)) < 0 {
			return item, ErrMacWalkMalformed
		}
		if typ == 1 {
			item.Size = le.Uint64(v)
			item.SizeKnown = true
		}
	}
	if nameStart < int64(p) || nameLen < 2 || nameStart > int64(len(b))-nameLen {
		return item, ErrMacWalkMalformed
	}
	name := b[int(nameStart):int(nameStart+nameLen)]
	if name[len(name)-1] != 0 || bytes.IndexByte(name[:len(name)-1], 0) >= 0 {
		return item, ErrMacWalkMalformed
	}
	item.Name = string(name[:len(name)-1])
	if !diskWalkValidName(item.Name) {
		return item, ErrMacWalkMalformed
	}
	item.IsDir = typ == 2
	item.IsRegular = typ == 1
	item.Used = true
	return item, nil
}

func diskWalkParseBatch(b []byte, count int) ([]diskWalkItem, error) {
	var out []diskWalkItem
	if count < 0 || count > len(b)/24 {
		return nil, ErrMacWalkMalformed
	}
	for i := 0; i < count; i++ {
		if len(b) < 4 {
			return out, ErrMacWalkMalformed
		}
		n := uint64(binary.LittleEndian.Uint32(b))
		if n < 24 || n > uint64(len(b)) {
			return out, ErrMacWalkMalformed
		}
		item, err := diskWalkParseRecord(b[:int(n)])
		if err != nil {
			return out, err
		}
		out = append(out, item)
		b = b[int(n):]
	}
	return out, nil
}
