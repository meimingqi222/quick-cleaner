package platform

import (
	"math"
	"testing"
)

func TestStatusMemoryWindowsSwapAccounting(t *testing.T) {
	for _, tc := range []struct {
		name                                       string
		commit, limit, physical, page, used, total uint64
	}{
		{"below RAM", 80, 150, 100, 4096, 0, 50 * 4096},
		{"above RAM", 120, 150, 100, 4096, 20 * 4096, 50 * 4096},
		{"no pagefile", 80, 90, 100, 4096, 0, 0},
		{"saturating bytes", math.MaxUint64, math.MaxUint64, 0, 4096, math.MaxUint64, math.MaxUint64},
	} {
		t.Run(tc.name, func(t *testing.T) {
			used, total := statusWindowsSwap(tc.commit, tc.limit, tc.physical, tc.page)
			if used != tc.used || total != tc.total {
				t.Fatalf("got (%d,%d), want (%d,%d)", used, total, tc.used, tc.total)
			}
		})
	}
}

func TestStatusMemoryDarwinUsedAccounting(t *testing.T) {
	// Inactive/free/purgeable pages do not enter Rust's used value, whereas
	// speculative and compressor pages must not disappear from it.
	if got := statusDarwinUsed(11, 7, 5, 3, 16384); got != 26*16384 {
		t.Fatalf("used=%d, want %d", got, 26*16384)
	}
	if got := statusDarwinUsed(math.MaxUint32, 1, 0, 0, 4096); got != (uint64(math.MaxUint32)+1)*4096 {
		t.Fatalf("page counts overflowed before widening: %d", got)
	}
	if got := statusDarwinUsed(1, 1, 0, 0, math.MaxUint64); got != math.MaxUint64 {
		t.Fatalf("byte count did not saturate: %d", got)
	}
}
