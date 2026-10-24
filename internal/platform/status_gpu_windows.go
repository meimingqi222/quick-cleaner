package platform

import (
	"sync"
	"syscall"
	"unsafe"
)

var gpuState struct {
	sync.Mutex
	opened bool
	query  *pdhQuery
}

var gpuAdaptersOnce sync.Once
var gpuAdapters map[uint64]gpuAdapter

// ReadGPUs returns all PDH-visible hardware GPUs in ascending LUID order.
// The query and its differential-counter history live for the process lifetime.
func ReadGPUs() []GpuReading {
	gpuState.Lock()
	defer gpuState.Unlock()
	if !gpuState.opened {
		gpuState.opened = true
		gpuState.query = openPDHQuery([]string{`\GPU Engine(*)\Utilization Percentage`, `\GPU Adapter Memory(*)\Dedicated Usage`})
	}
	query := gpuState.query
	if query == nil || !query.collect() {
		return []GpuReading{}
	}
	gpuAdaptersOnce.Do(func() { gpuAdapters = enumerateGPUAdapters() })
	gpus := aggregateGPUs(query.values(0), query.values(1), gpuAdapters)
	temperatures := gpuTemperatures()
	for i := range gpus {
		if name := gpus[i].Name; name != nil {
			if temp, ok := temperatures[*name]; ok {
				gpus[i].TempC = &temp
			}
		}
	}
	return gpus
}

type gpuDXGIAdapterDesc1 struct {
	Description                                                     [128]uint16
	VendorID, DeviceID, SubSysID, Revision                          uint32
	DedicatedVideoMemory, DedicatedSystemMemory, SharedSystemMemory uintptr
	LUIDLow                                                         uint32
	LUIDHigh                                                        int32
	Flags                                                           uint32
}

type gpuCOMObject struct {
	vtable *[13]uintptr
}

//go:uintptrescapes
func gpuCOMCall(object *gpuCOMObject, slot int, args ...uintptr) uintptr {
	result, _, _ := syscall.SyscallN(object.vtable[slot], append([]uintptr{uintptr(unsafe.Pointer(object))}, args...)...)
	return result
}

func enumerateGPUAdapters() map[uint64]gpuAdapter {
	out := make(map[uint64]gpuAdapter)
	create := syscall.NewLazyDLL("dxgi.dll").NewProc("CreateDXGIFactory1")
	if create.Find() != nil {
		return out
	}
	iid := syscall.GUID{Data1: 0x770aae78, Data2: 0xf26f, Data3: 0x4dba, Data4: [8]byte{0xa8, 0x29, 0x25, 0x3c, 0x83, 0xd1, 0xb3, 0x87}}
	var factory *gpuCOMObject
	hr, _, _ := create.Call(uintptr(unsafe.Pointer(&iid)), uintptr(unsafe.Pointer(&factory)))
	if int32(hr) < 0 || factory == nil {
		return out
	}
	defer gpuCOMCall(factory, 2)
	for index := uint32(0); ; index++ {
		var adapter *gpuCOMObject
		// IDXGIFactory1::EnumAdapters1 follows the twelve IDXGIFactory slots.
		if int32(gpuCOMCall(factory, 12, uintptr(index), uintptr(unsafe.Pointer(&adapter)))) < 0 || adapter == nil {
			break
		}
		var desc gpuDXGIAdapterDesc1
		if int32(gpuCOMCall(adapter, 10, uintptr(unsafe.Pointer(&desc)))) >= 0 {
			name := syscall.UTF16ToString(desc.Description[:])
			entry := gpuAdapter{software: desc.Flags&2 != 0}
			if name != "" {
				entry.name = &name
			}
			out[uint64(uint32(desc.LUIDHigh))<<32|uint64(desc.LUIDLow)] = entry
		}
		gpuCOMCall(adapter, 2)
	}
	return out
}
