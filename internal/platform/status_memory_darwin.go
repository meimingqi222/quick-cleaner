package platform

import (
	"context"
	"fmt"
	"sync"
	"time"
	"unsafe"

	"github.com/ebitengine/purego"
)

var statusMemorySystem struct {
	sync.Once
	err        error
	hostSelf   func() uint32
	taskSelf   func() uint32
	deallocate func(uint32, uint32) int32
	statistics func(uint32, int32, unsafe.Pointer, *uint32) int32
	pageSize   func() int32
	sysctl     func(string, unsafe.Pointer, *uintptr, unsafe.Pointer, uintptr) int32
}

func loadStatusMemorySystem() error {
	statusMemorySystem.Do(func() {
		var handle uintptr
		handle, statusMemorySystem.err = purego.Dlopen("/usr/lib/libSystem.B.dylib", purego.RTLD_NOW|purego.RTLD_LOCAL)
		if statusMemorySystem.err != nil {
			return
		}
		for _, binding := range []struct {
			name string
			fn   any
		}{
			{"mach_host_self", &statusMemorySystem.hostSelf},
			{"mach_task_self", &statusMemorySystem.taskSelf},
			{"mach_port_deallocate", &statusMemorySystem.deallocate},
			{"host_statistics64", &statusMemorySystem.statistics},
			{"getpagesize", &statusMemorySystem.pageSize},
			{"sysctlbyname", &statusMemorySystem.sysctl},
		} {
			var address uintptr
			address, statusMemorySystem.err = purego.Dlsym(handle, binding.name)
			if statusMemorySystem.err != nil {
				return
			}
			purego.RegisterFunc(binding.fn, address)
		}
	})
	return statusMemorySystem.err
}

// vm_statistics64 rev1, HOST_VM_INFO64_COUNT=38 natural_t words.
type statusVMStatistics64 struct {
	Free, Active, Inactive, Wired             uint32
	Counters                                  [9]uint64
	Purgeable, Speculative                    uint32
	CompressorCounters                        [4]uint64
	Compressor, Throttled, External, Internal uint32
	TotalUncompressed                         uint64
}

func statusMemorySysctl(name string, out unsafe.Pointer, size uintptr) error {
	n := size
	if result := statusMemorySystem.sysctl(name, out, &n, nil, 0); result != 0 || n != size {
		return fmt.Errorf("sysctlbyname(%s): result=%d size=%d, want %d", name, result, n, size)
	}
	return nil
}

func ReadStatusMemory(ctx context.Context) (StatusMemoryReading, error) {
	if err := ctx.Err(); err != nil {
		return StatusMemoryReading{}, err
	}
	if err := loadStatusMemorySystem(); err != nil {
		return StatusMemoryReading{}, err
	}
	var total uint64
	if err := statusMemorySysctl("hw.memsize", unsafe.Pointer(&total), unsafe.Sizeof(total)); err != nil {
		return StatusMemoryReading{}, err
	}
	var swap struct {
		Total, Avail, Used  uint64
		PageSize, Encrypted uint32
	}
	if err := statusMemorySysctl("vm.swapusage", unsafe.Pointer(&swap), unsafe.Sizeof(swap)); err != nil {
		return StatusMemoryReading{}, err
	}
	var stat statusVMStatistics64
	count := uint32(unsafe.Sizeof(stat) / 4)
	port := statusMemorySystem.hostSelf()
	defer statusMemorySystem.deallocate(statusMemorySystem.taskSelf(), port)
	if kr := statusMemorySystem.statistics(port, 4, unsafe.Pointer(&stat), &count); kr != 0 || count < 38 {
		return StatusMemoryReading{}, fmt.Errorf("host_statistics64: result=%d count=%d", kr, count)
	}
	pageSize := statusMemorySystem.pageSize()
	if pageSize <= 0 {
		return StatusMemoryReading{}, fmt.Errorf("getpagesize: %d", pageSize)
	}
	return StatusMemoryReading{Used: statusDarwinUsed(stat.Active, stat.Wired, stat.Compressor, stat.Speculative, uint64(pageSize)), Total: total, SwapUsed: swap.Total - swap.Avail, SwapTotal: swap.Total}, nil
}

// The caller's gopsutil VMS already matches sysinfo on Darwin.
func ProcessVirtualMemory(uint32) *uint64 { return nil }

func SystemUptimeSecs(ctx context.Context) (uint64, error) {
	if err := ctx.Err(); err != nil {
		return 0, err
	}
	if err := loadStatusMemorySystem(); err != nil {
		return 0, err
	}
	var boot struct {
		Sec  int64
		Usec int32
		Pad  int32
	}
	if err := statusMemorySysctl("kern.boottime", unsafe.Pointer(&boot), unsafe.Sizeof(boot)); err != nil {
		return 0, err
	}
	if boot.Sec <= 0 {
		return 0, fmt.Errorf("invalid boot time: %d", boot.Sec)
	}
	return statusSaturatingSub(uint64(time.Now().Unix()), uint64(boot.Sec)), nil
}
