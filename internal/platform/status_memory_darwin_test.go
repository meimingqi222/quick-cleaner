package platform

import (
	"testing"
	"unsafe"
)

func TestStatusMemoryDarwinABI(t *testing.T) {
	var stat statusVMStatistics64
	if unsafe.Sizeof(stat) != 152 || unsafe.Offsetof(stat.Speculative) != 92 || unsafe.Offsetof(stat.Compressor) != 128 {
		t.Fatal("vm_statistics64 rev1 ABI mismatch")
	}
	if ProcessVirtualMemory(1) != nil {
		t.Fatal("Darwin must keep caller's VMS")
	}
}
