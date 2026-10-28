package platform

import (
	"os"
	"testing"
	"unsafe"
)

func TestPDHCounterArrayABI(t *testing.T) {
	var item pdhCounterItem
	if unsafe.Sizeof(uintptr(0)) == 8 && (unsafe.Sizeof(item) != 24 || unsafe.Offsetof(item.Status) != 8 || unsafe.Offsetof(item.Value) != 16) {
		t.Fatal("PDH_FMT_COUNTERVALUE_ITEM_W ABI mismatch")
	}
}

func TestPDHEnglishCounterAndHandleLifetime(t *testing.T) {
	q := openPDHQuery([]string{`\Process(*)\ID Process`, `\System\System Up Time`})
	if q == nil {
		t.Fatal("built-in English uptime counter unavailable")
	}
	defer q.close()
	if !q.collect() || !q.collect() {
		t.Fatal("persistent query did not collect")
	}
	found := false
	for _, value := range q.values(0) {
		found = found || value == float64(os.Getpid())
	}
	if !found {
		t.Fatal("formatted wildcard array did not contain own PID")
	}
	if len(q.values(-1)) != 0 || len(q.values(2)) != 0 {
		t.Fatal("out-of-range counter must have no instances")
	}
	if q := openPDHQuery([]string{`\System\System Up Time`, `\QuickCleaner nonexistent object\nonexistent counter`}); q != nil {
		q.close()
		t.Fatal("partial counter set must not be accepted")
	}
}
