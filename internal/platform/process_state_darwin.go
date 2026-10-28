package platform

import (
	"context"
	"encoding/binary"
	"fmt"
	"unsafe"
)

func ProcessStatusText(ctx context.Context, pid uint32) string {
	if ctx.Err() != nil || !loadStatusLibproc() {
		return "Unknown(0)"
	}
	// proc_bsdinfo is 136 bytes; pbi_status is the second uint32.
	var bsd [136]byte
	if statusLibproc.pidInfo(int32(pid), 3, 0, unsafe.Pointer(&bsd[0]), int32(len(bsd))) != int32(len(bsd)) {
		return "Unknown(0)"
	}
	status := binary.LittleEndian.Uint32(bsd[4:8])
	if status == 2 {
		// Match sysinfo's main-thread fallback without launching one ps per PID.
		// proc_threadinfo: two uint64 times, cpu_usage, policy, run_state.
		var thread [112]byte
		if statusLibproc.pidInfo(int32(pid), 5, 0, unsafe.Pointer(&thread[0]), int32(len(thread))) == int32(len(thread)) {
			switch state := binary.LittleEndian.Uint32(thread[24:28]); state {
			case 1:
				return "Run"
			case 2:
				return "Stop"
			case 3:
				return "Sleep"
			case 4:
				return "Dead"
			case 5:
				return "Parked"
			default:
				return fmt.Sprintf("Unknown(%d)", state)
			}
		}
	}
	switch status {
	case 1:
		return "Idle"
	case 2:
		return "Run"
	case 3:
		return "Sleep"
	case 4:
		return "Stop"
	case 5:
		return "Zombie"
	default:
		return fmt.Sprintf("Unknown(%d)", status)
	}
}
