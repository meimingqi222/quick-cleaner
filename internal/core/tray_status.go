package core

import (
	"context"
	"math"
	"sync"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type TraySnapshot struct {
	CPUPct                            uint32
	MemoryUsed, MemoryTotal, SwapUsed uint64
	TempC                             *float32
}

type TrayCounters struct {
	CPUTotal, CPUBusy float64
	Snapshot          TraySnapshot
}

// Tray acquisition deliberately has no process, port, network, GPU or account
// inventory. Temperature is refreshed only on the slow polling cycle.
type TraySource interface {
	ReadTray(context.Context, bool) (TrayCounters, error)
}

type systemTraySource struct{}

func (systemTraySource) ReadTray(ctx context.Context, refreshThermal bool) (TrayCounters, error) {
	var out TrayCounters
	var err error
	out.CPUTotal, out.CPUBusy, err = readSystemCPU(ctx)
	if err != nil {
		return out, err
	}
	memory, err := platform.ReadStatusMemory(ctx)
	if err != nil {
		return out, err
	}
	out.Snapshot = TraySnapshot{MemoryUsed: memory.Used, MemoryTotal: memory.Total, SwapUsed: memory.SwapUsed}
	if refreshThermal {
		out.Snapshot.TempC = platform.ReadThermal().CPUTemp
	}
	return out, ctx.Err()
}

// Retain one sampler across ticks and invoke it on a worker, not the UI thread.
type TraySampler struct {
	mu       sync.Mutex
	source   TraySource
	previous TrayCounters
	temp     *float32
}

func NewTraySampler(ctx context.Context) (*TraySampler, error) {
	return NewTraySamplerWithSource(ctx, systemTraySource{})
}

func NewTraySamplerWithSource(ctx context.Context, source TraySource) (*TraySampler, error) {
	initial, err := source.ReadTray(ctx, false)
	if err != nil {
		return nil, err
	}
	return &TraySampler{source: source, previous: initial}, nil
}

func (s *TraySampler) Sample(ctx context.Context, refreshThermal bool) (TraySnapshot, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	current, err := s.source.ReadTray(ctx, refreshThermal)
	if err != nil {
		return TraySnapshot{}, err
	}
	out := current.Snapshot
	out.CPUPct = 0
	if total := current.CPUTotal - s.previous.CPUTotal; total > 0 {
		// sysinfo computes an f32 percentage before Rust rounds to u32.
		usage := float32(min(100, max(0, (current.CPUBusy-s.previous.CPUBusy)/total*100)))
		out.CPUPct = uint32(math.Round(float64(usage)))
	}
	if refreshThermal {
		s.temp = nil
		if out.TempC != nil {
			temp := *out.TempC
			s.temp = &temp
		}
	}
	out.TempC = nil
	if s.temp != nil {
		temp := *s.temp
		out.TempC = &temp
	}
	s.previous = current
	return out, nil
}
