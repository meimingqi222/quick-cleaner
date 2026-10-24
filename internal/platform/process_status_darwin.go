package platform

import (
	"fmt"
	"slices"
	"sync"
	"syscall"
	"unsafe"

	"github.com/ebitengine/purego"
)

var statusLibproc struct {
	sync.Once
	err     error
	listAll func(unsafe.Pointer, int32) int32
	pidInfo func(int32, int32, uint64, unsafe.Pointer, int32) int32
	fdInfo  func(int32, int32, int32, unsafe.Pointer, int32) int32
}

func loadStatusLibproc() bool {
	statusLibproc.Do(func() {
		var handle uintptr
		handle, statusLibproc.err = purego.Dlopen("/usr/lib/libproc.dylib", purego.RTLD_NOW|purego.RTLD_LOCAL)
		if statusLibproc.err != nil {
			return
		}
		for _, binding := range []struct {
			name string
			fn   any
		}{
			{"proc_listallpids", &statusLibproc.listAll}, {"proc_pidinfo", &statusLibproc.pidInfo}, {"proc_pidfdinfo", &statusLibproc.fdInfo},
		} {
			var address uintptr
			address, statusLibproc.err = purego.Dlsym(handle, binding.name)
			if statusLibproc.err != nil {
				return
			}
			purego.RegisterFunc(binding.fn, address)
		}
	})
	return statusLibproc.err == nil
}

func statusAllPIDs() []int32 {
	if !loadStatusLibproc() {
		return nil
	}
	for size := 4096; ; size *= 2 {
		pids := make([]int32, size)
		n := statusLibproc.listAll(unsafe.Pointer(&pids[0]), int32(len(pids)*4))
		if n <= 0 {
			return nil
		}
		if int(n) < len(pids) {
			return pids[:n]
		}
	}
}

func ProcessUniqueID(pid uint32) *uint64 {
	if !loadStatusLibproc() {
		return nil
	}
	var info struct {
		UUID                                                [16]byte
		Unique, ParentUnique, IDVersion, Reserve2, Reserve3 uint64
	}
	size := int32(unsafe.Sizeof(info))
	if statusLibproc.pidInfo(int32(pid), 17, 0, unsafe.Pointer(&info), size) != size || info.Unique == 0 {
		return nil
	}
	id := info.Unique
	return &id
}

func ensureSameStatusProcess(pid uint32, expected *uint64) error {
	if expected == nil {
		return fmt.Errorf("PID %d 缺少 Darwin 进程标识，拒绝结束", pid)
	}
	actual := ProcessUniqueID(pid)
	if actual == nil {
		return fmt.Errorf("PID %d 已退出或无法读取进程标识", pid)
	}
	if *actual != *expected {
		return fmt.Errorf("PID %d 已退出或已被其他进程复用", pid)
	}
	again := ProcessUniqueID(pid)
	if again == nil || *again != *expected {
		return fmt.Errorf("PID %d 已退出或已被其他进程复用", pid)
	}
	return nil
}

func TerminateProcess(pid uint32, _ uint64, unique *uint64) error {
	if err := ensureSameStatusProcess(pid, unique); err != nil {
		return err
	}
	return syscall.Kill(int(pid), syscall.SIGTERM)
}

func ForceTerminateProcess(pid uint32, _ uint64, unique *uint64) error {
	if err := ensureSameStatusProcess(pid, unique); err != nil {
		return err
	}
	return syscall.Kill(int(pid), syscall.SIGKILL)
}

// SDK socket_fdinfo has a 792B ABI; retain the Rust reference's measured
// family/kind/local-port offsets, rather than approximating the union layout.
type statusSocketFDInfo struct {
	Prefix      [184]byte
	Family      int32
	Middle      [68]byte
	Kind        int32
	Reserved    uint32
	ForeignPort int32
	LocalPort   int32
	Rest        [520]byte
}

var _ [792 - unsafe.Sizeof(statusSocketFDInfo{})]byte
var _ [unsafe.Sizeof(statusSocketFDInfo{}) - 792]byte
var _ [184 - unsafe.Offsetof(statusSocketFDInfo{}.Family)]byte
var _ [unsafe.Offsetof(statusSocketFDInfo{}.Family) - 184]byte
var _ [256 - unsafe.Offsetof(statusSocketFDInfo{}.Kind)]byte
var _ [unsafe.Offsetof(statusSocketFDInfo{}.Kind) - 256]byte
var _ [268 - unsafe.Offsetof(statusSocketFDInfo{}.LocalPort)]byte
var _ [unsafe.Offsetof(statusSocketFDInfo{}.LocalPort) - 268]byte

func ProcessPortsMap() map[uint32][]uint16 {
	out := make(map[uint32][]uint16)
	for _, pid := range statusAllPIDs() {
		needed := statusLibproc.pidInfo(pid, 1, 0, nil, 0)
		if needed < 8 {
			continue
		}
		fds := make([]struct {
			FD   int32
			Kind uint32
		}, int(needed)/8)
		got := statusLibproc.pidInfo(pid, 1, 0, unsafe.Pointer(&fds[0]), int32(len(fds)*8))
		if got <= 0 || int(got) > len(fds)*8 {
			continue
		}
		for _, fd := range fds[:int(got)/8] {
			if fd.Kind != 2 {
				continue
			}
			var info statusSocketFDInfo
			size := int32(unsafe.Sizeof(info))
			if statusLibproc.fdInfo(pid, fd.FD, 3, unsafe.Pointer(&info), size) < size {
				continue
			}
			if (info.Kind != 1 && info.Kind != 2) || (info.Family != syscall.AF_INET && info.Family != syscall.AF_INET6) {
				continue
			}
			n := uint16(info.LocalPort)
			port := n>>8 | n<<8
			if port > 0 {
				out[uint32(pid)] = append(out[uint32(pid)], port)
			}
		}
	}
	for pid, ports := range out {
		slices.Sort(ports)
		out[pid] = slices.Compact(ports)
	}
	return out
}

func ProcessThreadsMap() map[uint32]uint32 {
	out := make(map[uint32]uint32)
	for _, pid := range statusAllPIDs() {
		// proc_taskinfo: six uint64 counters followed by twelve int32 fields.
		var info struct {
			Counters                                                                                  [6]uint64
			Policy, Faults, Pageins, COW, Sent, Received, Mach, Unix, CSW, Threads, Running, Priority int32
		}
		size := int32(unsafe.Sizeof(info))
		if statusLibproc.pidInfo(pid, 4, 0, unsafe.Pointer(&info), size) == size && info.Threads > 0 {
			out[uint32(pid)] = uint32(info.Threads)
		}
	}
	return out
}
