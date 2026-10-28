package platform

import (
	"context"
	"fmt"
	"syscall"
	"unsafe"
)

var statusMemoryKernel = syscall.NewLazyDLL("kernel32.dll")
var statusGlobalMemory = statusMemoryKernel.NewProc("GlobalMemoryStatusEx")
var statusPerformanceInfo = statusMemoryKernel.NewProc("K32GetPerformanceInfo")
var statusProcessMemory = statusMemoryKernel.NewProc("K32GetProcessMemoryInfo")
var statusTickCount = statusMemoryKernel.NewProc("GetTickCount64")

type statusMemoryEx struct {
	Length, Load                                       uint32
	TotalPhys, AvailPhys, TotalPageFile, AvailPageFile uint64
	TotalVirtual, AvailVirtual, AvailExtendedVirtual   uint64
}

type statusPerformance struct {
	Size                                               uint32
	CommitTotal, CommitLimit, CommitPeak               uintptr
	PhysicalTotal, PhysicalAvailable, SystemCache      uintptr
	KernelTotal, KernelPaged, KernelNonpaged, PageSize uintptr
	HandleCount, ProcessCount, ThreadCount             uint32
}

type statusProcessMemoryEx struct {
	Size, PageFaultCount                               uint32
	PeakWorkingSetSize, WorkingSetSize                 uintptr
	QuotaPeakPagedPoolUsage, QuotaPagedPoolUsage       uintptr
	QuotaPeakNonPagedPoolUsage, QuotaNonPagedPoolUsage uintptr
	PagefileUsage, PeakPagefileUsage, PrivateUsage     uintptr
}

func ReadStatusMemory(ctx context.Context) (StatusMemoryReading, error) {
	if err := ctx.Err(); err != nil {
		return StatusMemoryReading{}, err
	}
	var ram statusMemoryEx
	ram.Length = uint32(unsafe.Sizeof(ram))
	if ok, _, err := statusGlobalMemory.Call(uintptr(unsafe.Pointer(&ram))); ok == 0 {
		return StatusMemoryReading{}, fmt.Errorf("GlobalMemoryStatusEx: %w", err)
	}
	var perf statusPerformance
	perf.Size = uint32(unsafe.Sizeof(perf))
	if ok, _, err := statusPerformanceInfo.Call(uintptr(unsafe.Pointer(&perf)), uintptr(perf.Size)); ok == 0 {
		return StatusMemoryReading{}, fmt.Errorf("K32GetPerformanceInfo: %w", err)
	}
	used, total := statusWindowsSwap(uint64(perf.CommitTotal), uint64(perf.CommitLimit), uint64(perf.PhysicalTotal), uint64(perf.PageSize))
	return StatusMemoryReading{Used: ram.TotalPhys - ram.AvailPhys, Total: ram.TotalPhys, SwapUsed: used, SwapTotal: total}, nil
}

// sysinfo virtual_memory() is private committed bytes, not PagefileUsage/VMS.
func ProcessVirtualMemory(pid uint32) *uint64 {
	const processVMRead = 0x0010
	h, err := syscall.OpenProcess(syscall.PROCESS_QUERY_INFORMATION|processVMRead, false, pid)
	if err != nil {
		return nil
	}
	defer syscall.CloseHandle(h)
	var counters statusProcessMemoryEx
	counters.Size = uint32(unsafe.Sizeof(counters))
	if ok, _, _ := statusProcessMemory.Call(uintptr(h), uintptr(unsafe.Pointer(&counters)), uintptr(counters.Size)); ok == 0 {
		return nil
	}
	value := uint64(counters.PrivateUsage)
	return &value
}

func SystemUptimeSecs(ctx context.Context) (uint64, error) {
	if err := ctx.Err(); err != nil {
		return 0, err
	}
	ticks, _, _ := statusTickCount.Call()
	return uint64(ticks) / 1000, nil
}
