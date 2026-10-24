package core

import (
	"context"
	"math"
	"slices"
	"strings"
	"sync"
	"time"
)

type StatusNetworkCounter struct {
	Name     string
	Received uint64
	Sent     uint64
}

// Missing counters mean unavailable, not zero. Failed reads must not become
// baselines that turn the next successful read into a lifetime-sized spike.
type StatusProcessCounter struct {
	Info         ProcInfo
	CPUSeconds   *float64
	ReadBytes    *uint64
	WrittenBytes *uint64
}

type StatusCounters struct {
	At                time.Time
	CPUTotal, CPUBusy float64
	// Windows process CPU uses aggregate system CPU time / logical CPUs,
	// whereas Darwin uses elapsed monotonic time.
	ProcessCPUClock *float64
	Networks        []StatusNetworkCounter
	Processes       []StatusProcessCounter
	Snapshot        StatusSnapshot
}

// Acquisition and rate accounting are separate so different native sources can
// share the same per-interface and process-identity rules.
type StatusSource interface {
	ReadStatus(context.Context) (StatusCounters, error)
}

type processStatusBaseline struct {
	identity ProcInfo
	cpu      *float64
	read     *uint64
	written  *uint64
	at       time.Time
	cpuClock *float64
}

// A sampler must live across refreshes. Sample blocks and belongs on a worker,
// not the UI thread; concurrent calls are serialized.
type StatusSampler struct {
	mu        sync.Mutex
	source    StatusSource
	previous  StatusCounters
	networks  map[string]StatusNetworkCounter
	processes map[uint32]processStatusBaseline
}

func NewStatusSampler(ctx context.Context) (*StatusSampler, error) {
	return NewStatusSamplerWithSource(ctx, &systemStatusSource{})
}

func NewStatusSamplerWithSource(ctx context.Context, source StatusSource) (*StatusSampler, error) {
	initial, err := source.ReadStatus(ctx)
	if err != nil {
		return nil, err
	}
	s := &StatusSampler{source: source, previous: initial, networks: make(map[string]StatusNetworkCounter), processes: make(map[uint32]processStatusBaseline)}
	for _, network := range initial.Networks {
		s.networks[network.Name] = network
	}
	return s, nil
}

func (s *StatusSampler) Sample(ctx context.Context) (StatusSnapshot, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	current, err := s.source.ReadStatus(ctx)
	if err != nil {
		return StatusSnapshot{}, err
	}
	elapsed := math.Max(current.At.Sub(s.previous.At).Seconds(), 0.001)
	out := current.Snapshot
	out.RxBPS, out.TxBPS, out.CPUUsage = 0, 0, 0
	totalDelta := current.CPUTotal - s.previous.CPUTotal
	if totalDelta > 0 {
		out.CPUUsage = float32(min(100, max(0, (current.CPUBusy-s.previous.CPUBusy)/totalDelta*100)))
	}
	for _, network := range current.Networks {
		name := asciiLower(network.Name)
		loopback := name == "lo" || strings.HasPrefix(name, "loopback")
		if strings.HasPrefix(name, "lo") && len(name) > 2 {
			digits := true
			for _, c := range name[2:] {
				digits = digits && c >= '0' && c <= '9'
			}
			loopback = loopback || digits
		}
		if !loopback {
			if previous, ok := s.networks[network.Name]; ok {
				out.RxBPS += float64(saturatingStatusDelta(network.Received, previous.Received)) / elapsed
				out.TxBPS += float64(saturatingStatusDelta(network.Sent, previous.Sent)) / elapsed
			}
		}
		// Keep vanished interfaces, matching Rust: reappearance uses its last
		// baseline, while a genuinely new interface contributes no first delta.
		s.networks[network.Name] = network
	}
	processes := make(map[uint32]processStatusBaseline, len(current.Processes))
	out.Processes = make([]ProcInfo, 0, len(current.Processes))
	for _, counter := range current.Processes {
		info := counter.Info
		previous, known := s.processes[info.PID]
		known = known && sameStatusProcess(previous.identity, info)
		info.CPU, info.DiskReadBPS, info.DiskWrittenBPS = 0, 0, 0
		baseline := processStatusBaseline{identity: info, at: current.At}
		if current.ProcessCPUClock != nil {
			clock := *current.ProcessCPUClock
			baseline.cpuClock = &clock
		}
		if counter.CPUSeconds != nil {
			cpu := *counter.CPUSeconds
			baseline.cpu = &cpu
			info.CPUTimeMS = uint64(max(0, cpu) * 1000)
			if known && previous.cpu != nil {
				// One fully occupied logical core is 100%, not 100/core_count.
				interval := current.At.Sub(previous.at).Seconds()
				if current.ProcessCPUClock != nil && previous.cpuClock != nil {
					interval = *current.ProcessCPUClock - *previous.cpuClock
				}
				if interval > 0.000001 {
					info.CPU = float32(min(float64(out.CoreCount)*100, max(0, cpu-*previous.cpu)/interval*100))
				}
			}
		}
		if counter.ReadBytes != nil {
			read := *counter.ReadBytes
			info.DiskReadBytes, baseline.read = read, &read
			prior := uint64(0)
			if known && previous.read != nil {
				prior = *previous.read
			}
			// The reference reports lifetime I/O on a process's first refresh.
			if !known || previous.read != nil {
				info.DiskReadBPS = float64(saturatingStatusDelta(read, prior)) / elapsed
			}
		}
		if counter.WrittenBytes != nil {
			written := *counter.WrittenBytes
			info.DiskWrittenBytes, baseline.written = written, &written
			prior := uint64(0)
			if known && previous.written != nil {
				prior = *previous.written
			}
			if !known || previous.written != nil {
				info.DiskWrittenBPS = float64(saturatingStatusDelta(written, prior)) / elapsed
			}
		}
		processes[info.PID] = baseline
		out.Processes = append(out.Processes, info)
	}
	slices.SortStableFunc(out.Processes, CompareStatusProcesses)
	out.ProcessCount = len(out.Processes)
	s.previous, s.processes = current, processes
	return out, nil
}

func sameStatusProcess(a, b ProcInfo) bool {
	if a.PID != b.PID || a.StartTime == 0 || a.StartTime != b.StartTime {
		return false
	}
	if a.UniqueID != nil || b.UniqueID != nil {
		return a.UniqueID != nil && b.UniqueID != nil && *a.UniqueID == *b.UniqueID
	}
	return true
}

func saturatingStatusDelta(current, previous uint64) uint64 {
	if current < previous {
		return 0
	}
	return current - previous
}
