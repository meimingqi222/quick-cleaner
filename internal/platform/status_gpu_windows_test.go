package platform

import (
	"testing"
	"unsafe"
)

func TestGPUDXGIAdapterABI(t *testing.T) {
	var desc gpuDXGIAdapterDesc1
	if unsafe.Sizeof(uintptr(0)) != 8 {
		t.Skip("amd64 ABI assertion")
	}
	if unsafe.Sizeof(desc) != 312 || unsafe.Offsetof(desc.LUIDLow) != 296 || unsafe.Offsetof(desc.Flags) != 304 {
		t.Fatalf("DXGI_ADAPTER_DESC1 ABI: size=%d luid=%d flags=%d", unsafe.Sizeof(desc), unsafe.Offsetof(desc.LUIDLow), unsafe.Offsetof(desc.Flags))
	}
}

func TestReadGPUsNative(t *testing.T) {
	for sample := 0; sample < 2; sample++ {
		gpus := ReadGPUs()
		for i, gpu := range gpus {
			if len(gpu.ID) != 16 || gpu.Utilization == nil || (i > 0 && gpus[i-1].ID >= gpu.ID) {
				t.Fatalf("bad GPU reading: %+v", gpu)
			}
			if *gpu.Utilization < 0 || *gpu.Utilization > 100 {
				t.Fatal("utilization out of bounds")
			}
			if gpu.TempC != nil && (*gpu.TempC <= 0 || *gpu.TempC >= 150) {
				t.Fatal("NVML sentinel escaped")
			}
		}
		t.Logf("sample %d: %d GPU(s); empty is valid for unavailable PDH/first sample", sample, len(gpus))
	}
}
