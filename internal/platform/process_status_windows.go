package platform

import (
	"encoding/binary"
	"fmt"
	"slices"
	"syscall"
	"unsafe"
)

var statusIPHelper = syscall.NewLazyDLL("iphlpapi.dll")
var extendedTCPTable = statusIPHelper.NewProc("GetExtendedTcpTable")
var extendedUDPTable = statusIPHelper.NewProc("GetExtendedUdpTable")

func ProcessUniqueID(uint32) *uint64 { return nil }

// The same handle binds identity verification and termination. A separate PID
// lookup followed by OpenProcess could terminate a reused PID instead (P57).
func TerminateProcess(pid uint32, expectedStart uint64, _ *uint64) error {
	const queryLimited = 0x1000
	h, err := syscall.OpenProcess(syscall.PROCESS_TERMINATE|queryLimited, false, pid)
	if err != nil {
		return fmt.Errorf("OpenProcess(%d): %w", pid, err)
	}
	defer syscall.CloseHandle(h)
	var created, exited, kernel, user syscall.Filetime
	if err := syscall.GetProcessTimes(h, &created, &exited, &kernel, &user); err != nil {
		return fmt.Errorf("PID %d 创建时间读取失败，拒绝终止: %w", pid, err)
	}
	ticks := uint64(created.HighDateTime)<<32 | uint64(created.LowDateTime)
	actual := ticks/10_000_000 - 11_644_473_600
	if actual != expectedStart {
		return fmt.Errorf("PID %d 已退出或已被其他进程复用", pid)
	}
	if err := syscall.TerminateProcess(h, 1); err != nil {
		return fmt.Errorf("TerminateProcess(%d): %w", pid, err)
	}
	return nil
}

func ForceTerminateProcess(pid uint32, expectedStart uint64, unique *uint64) error {
	return TerminateProcess(pid, expectedStart, unique)
}

func ProcessPortsMap() map[uint32][]uint16 {
	out := make(map[uint32][]uint16)
	// OWNER_PID_ALL TCP rows: IPv4=24B, IPv6=56B; UDP rows: 12B/28B.
	for _, table := range []struct {
		proc           *syscall.LazyProc
		family, class  uintptr
		row, port, pid int
	}{
		{extendedTCPTable, 2, 5, 24, 8, 20},
		{extendedTCPTable, 23, 5, 56, 20, 52},
		{extendedUDPTable, 2, 1, 12, 4, 8},
		{extendedUDPTable, 23, 1, 28, 20, 24},
	} {
		var size uint32
		_, _, _ = table.proc.Call(0, uintptr(unsafe.Pointer(&size)), 0, table.family, table.class, 0)
		if size < 4 {
			continue
		}
		buf := make([]byte, size)
		result, _, _ := table.proc.Call(uintptr(unsafe.Pointer(&buf[0])), uintptr(unsafe.Pointer(&size)), 0, table.family, table.class, 0)
		if result != 0 || size < 4 || uint64(size) > uint64(len(buf)) {
			continue
		}
		count := int(binary.LittleEndian.Uint32(buf[:4]))
		if count > (int(size)-4)/table.row {
			continue
		}
		for i := 0; i < count; i++ {
			row := buf[4+i*table.row : 4+(i+1)*table.row]
			port := binary.BigEndian.Uint16(row[table.port:])
			if port == 0 {
				continue
			}
			pid := binary.LittleEndian.Uint32(row[table.pid:])
			out[pid] = append(out[pid], port)
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
	h, err := syscall.CreateToolhelp32Snapshot(syscall.TH32CS_SNAPPROCESS, 0)
	if err != nil {
		return out
	}
	defer syscall.CloseHandle(h)
	entry := syscall.ProcessEntry32{}
	entry.Size = uint32(unsafe.Sizeof(entry))
	for err = syscall.Process32First(h, &entry); err == nil; err = syscall.Process32Next(h, &entry) {
		out[entry.ProcessID] = entry.Threads
	}
	return out
}
