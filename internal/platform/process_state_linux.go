package platform

import (
	"context"

	"github.com/shirou/gopsutil/v4/process"
)

func ProcessStatusText(ctx context.Context, pid uint32) string {
	p, err := process.NewProcessWithContext(ctx, int32(pid))
	if err != nil {
		return "Unknown(0)"
	}
	states, _ := p.StatusWithContext(ctx)
	if len(states) == 0 {
		return "Unknown(0)"
	}
	switch states[0] {
	case process.Running:
		return "Run"
	case process.Sleep:
		return "Sleep"
	case process.Idle:
		return "Idle"
	case process.Stop:
		return "Stop"
	case process.Zombie:
		return "Zombie"
	case process.Blocked:
		return "UninterruptibleDiskSleep"
	default:
		return "Unknown(0)"
	}
}
