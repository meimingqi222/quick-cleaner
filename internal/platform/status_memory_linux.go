package platform

import (
	"context"

	"github.com/shirou/gopsutil/v4/host"
	"github.com/shirou/gopsutil/v4/mem"
)

// Linux is the inspection/gallery host, not a Rust product platform.
func ReadStatusMemory(ctx context.Context) (StatusMemoryReading, error) {
	if err := ctx.Err(); err != nil {
		return StatusMemoryReading{}, err
	}
	ram, err := mem.VirtualMemoryWithContext(ctx)
	if err != nil {
		return StatusMemoryReading{}, err
	}
	swap, err := mem.SwapMemoryWithContext(ctx)
	if err != nil {
		return StatusMemoryReading{}, err
	}
	return StatusMemoryReading{Used: ram.Used, Total: ram.Total, SwapUsed: swap.Used, SwapTotal: swap.Total}, nil
}

// The caller's gopsutil VMS already has the desired Linux semantics.
func ProcessVirtualMemory(uint32) *uint64 { return nil }

func SystemUptimeSecs(ctx context.Context) (uint64, error) {
	if err := ctx.Err(); err != nil {
		return 0, err
	}
	return host.UptimeWithContext(ctx)
}
