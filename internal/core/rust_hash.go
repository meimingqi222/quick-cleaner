package core

import (
	"encoding/binary"
	"math/bits"
)

// Rust DefaultHasher::new uses SipHash-1-3 with zero keys. Keep compatibility
// for duplicate group IDs and the photo placeholder palette, not cryptography.
type rustHasher struct {
	v       [4]uint64
	tail    [8]byte
	length  uint64
	pending int
}

func newRustHasher() rustHasher {
	return rustHasher{v: [4]uint64{0x736f6d6570736575, 0x646f72616e646f6d, 0x6c7967656e657261, 0x7465646279746573}}
}

func (h *rustHasher) round() {
	v := &h.v
	v[0] += v[1]
	v[1] = bits.RotateLeft64(v[1], 13) ^ v[0]
	v[0] = bits.RotateLeft64(v[0], 32)
	v[2] += v[3]
	v[3] = bits.RotateLeft64(v[3], 16) ^ v[2]
	v[0] += v[3]
	v[3] = bits.RotateLeft64(v[3], 21) ^ v[0]
	v[2] += v[1]
	v[1] = bits.RotateLeft64(v[1], 17) ^ v[2]
	v[2] = bits.RotateLeft64(v[2], 32)
}

func (h *rustHasher) block(m uint64) {
	h.v[3] ^= m
	h.round()
	h.v[0] ^= m
}

func (h *rustHasher) Write(data []byte) (int, error) {
	n := len(data)
	h.length += uint64(n)
	for len(data) > 0 {
		if h.pending == 0 && len(data) >= 8 {
			h.block(binary.LittleEndian.Uint64(data))
			data = data[8:]
			continue
		}
		copied := copy(h.tail[h.pending:], data)
		h.pending += copied
		data = data[copied:]
		if h.pending == 8 {
			h.block(binary.LittleEndian.Uint64(h.tail[:]))
			h.pending = 0
		}
	}
	return n, nil
}

func (h rustHasher) Sum64() uint64 {
	b := h.length << 56
	for i := range h.pending {
		b |= uint64(h.tail[i]) << (8 * i)
	}
	h.block(b)
	h.v[2] ^= 0xff
	for range 3 {
		h.round()
	}
	return h.v[0] ^ h.v[1] ^ h.v[2] ^ h.v[3]
}
