package core

import (
	"context"
	"errors"
	"math"
	"os"
	"testing"
	"time"
)

type statusFixtureSource struct {
	frames []StatusCounters
	errors map[int]error
	next   int
}

func (f *statusFixtureSource) ReadStatus(ctx context.Context) (StatusCounters, error) {
	if err := ctx.Err(); err != nil {
		return StatusCounters{}, err
	}
	i := f.next
	f.next++
	if err := f.errors[i]; err != nil {
		return StatusCounters{}, err
	}
	return f.frames[i], nil
}

func statusFixtureFrame(second int64, networks ...StatusNetworkCounter) StatusCounters {
	return StatusCounters{At: time.Unix(1000+second, 0), CPUTotal: 1000 + float64(second)*4, CPUBusy: 100 + float64(second), Networks: networks, Snapshot: StatusSnapshot{CoreCount: 4, MemTotal: 8192, MemUsed: 2048, OSName: "fixture"}}
}

func TestStatusSamplerPerInterfaceBaselines(t *testing.T) {
	frames := []StatusCounters{
		statusFixtureFrame(0, StatusNetworkCounter{"eth", 1000, 2000}, StatusNetworkCounter{"long", 100, 100}, StatusNetworkCounter{"lo١", 100, 100}),
		statusFixtureFrame(2, StatusNetworkCounter{"eth", 1300, 2080}, StatusNetworkCounter{"utun0", 9000000, 8000000}, StatusNetworkCounter{"long", 140, 120}, StatusNetworkCounter{"lo١", 160, 140}, StatusNetworkCounter{"LO0", 9999999, 9999999}, StatusNetworkCounter{"Loopback Pseudo-Interface", 9999999, 9999999}),
		statusFixtureFrame(4, StatusNetworkCounter{"eth", 100, 20}, StatusNetworkCounter{"utun0", 9000060, 8000020}),
		statusFixtureFrame(6, StatusNetworkCounter{"eth", 180, 70}, StatusNetworkCounter{"utun0", 9000080, 8000060}, StatusNetworkCounter{"long", 200, 160}, StatusNetworkCounter{"LO0", 19999999, 19999999}, StatusNetworkCounter{"Loopback Pseudo-Interface", 19999999, 19999999}),
	}
	s, err := NewStatusSamplerWithSource(context.Background(), &statusFixtureSource{frames: frames})
	if err != nil {
		t.Fatal(err)
	}
	for i, want := range [][2]float64{{200, 70}, {30, 10}, {80, 65}} {
		got, err := s.Sample(context.Background())
		if err != nil {
			t.Fatal(err)
		}
		if got.RxBPS != want[0] || got.TxBPS != want[1] || got.CPUUsage != 25 || got.MemTotal != 8192 || got.OSName != "fixture" {
			t.Fatalf("sample %d: %+v; want rx/tx %v", i, got, want)
		}
	}
}

func statusFixtureProcess(pid uint32, identity uint64, cpu float64, read, written uint64) StatusProcessCounter {
	id := identity
	return StatusProcessCounter{Info: ProcInfo{PID: pid, StartTime: 10, UniqueID: &id, Name: "fixture", MemBytes: uint64(pid) * 10}, CPUSeconds: &cpu, ReadBytes: &read, WrittenBytes: &written}
}

func TestStatusSamplerProcessIdentityAndCompleteSorting(t *testing.T) {
	frames := []StatusCounters{statusFixtureFrame(0), statusFixtureFrame(2), statusFixtureFrame(4), statusFixtureFrame(6)}
	frames[1].Processes = []StatusProcessCounter{statusFixtureProcess(5, 7, 10, 1000, 800), statusFixtureProcess(6, 8, 20, 600, 200)}
	frames[2].Processes = []StatusProcessCounter{statusFixtureProcess(5, 7, 13, 1400, 900), statusFixtureProcess(6, 8, 21, 800, 280)}
	// Same PID and same-second start time, but different Darwin identity.
	frames[3].Processes = []StatusProcessCounter{statusFixtureProcess(5, 9, 100, 5000, 3000)}
	for pid := uint32(10); pid < 50; pid++ {
		frames[2].Processes = append(frames[2].Processes, statusFixtureProcess(pid, uint64(pid), 1, 0, 0))
	}
	s, err := NewStatusSamplerWithSource(context.Background(), &statusFixtureSource{frames: frames})
	if err != nil {
		t.Fatal(err)
	}
	first, err := s.Sample(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if first.Processes[0].PID != 6 || first.Processes[0].CPU != 0 || first.Processes[1].DiskReadBPS != 500 {
		t.Fatalf("first process refresh must baseline CPU, retain reference first I/O and sort memory ties: %+v", first.Processes)
	}
	second, err := s.Sample(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if second.ProcessCount != 42 || len(second.Processes) != 42 {
		t.Fatalf("process list was truncated: %d/%d", second.ProcessCount, len(second.Processes))
	}
	a, b := second.Processes[0], second.Processes[1]
	if a.PID != 5 || a.CPU != 150 || a.CPUTimeMS != 13000 || a.DiskReadBPS != 200 || a.DiskWrittenBPS != 50 || b.PID != 6 || b.CPU != 50 {
		t.Fatalf("per-core CPU / I/O deltas or CPU-first sorting: %+v / %+v", a, b)
	}
	third, err := s.Sample(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if third.Processes[0].CPU != 0 || third.Processes[0].DiskReadBPS != 2500 {
		t.Fatalf("reused PID inherited previous owner's counters: %+v", third.Processes)
	}
}

func TestStatusSamplerSystemCPUClock(t *testing.T) {
	frames := []StatusCounters{statusFixtureFrame(0), statusFixtureFrame(2), statusFixtureFrame(4), statusFixtureFrame(6)}
	clocks := []float64{100, 102, 103, 1}
	for i := range frames {
		frames[i].ProcessCPUClock = &clocks[i]
		frames[i].Processes = []StatusProcessCounter{statusFixtureProcess(5, 7, float64(i)*1.5, 0, 0)}
	}
	s, err := NewStatusSamplerWithSource(context.Background(), &statusFixtureSource{frames: frames})
	if err != nil {
		t.Fatal(err)
	}
	for _, want := range []float32{0, 150, 0} {
		got, err := s.Sample(context.Background())
		if err != nil || got.Processes[0].CPU != want {
			t.Fatalf("system CPU clock (not wall clock) and reset: %+v %v; want %v", got, err, want)
		}
	}
}

func TestStatusSamplerMissingCountersAndFailedSamples(t *testing.T) {
	frames := []StatusCounters{statusFixtureFrame(0), statusFixtureFrame(2), statusFixtureFrame(4), {}, statusFixtureFrame(8), statusFixtureFrame(10), statusFixtureFrame(12)}
	frames[1].Processes = []StatusProcessCounter{statusFixtureProcess(5, 7, 10, 1000, 800)}
	missing := statusFixtureProcess(5, 7, 0, 0, 0)
	missing.CPUSeconds, missing.ReadBytes, missing.WrittenBytes = nil, nil, nil
	frames[2].Processes = []StatusProcessCounter{missing}
	frames[4].Processes = []StatusProcessCounter{statusFixtureProcess(5, 7, 20, 2000, 1600)}
	frames[5].Processes = []StatusProcessCounter{statusFixtureProcess(5, 7, 21, 2200, 1700)}
	frames[6].Processes = []StatusProcessCounter{statusFixtureProcess(5, 7, 1, 100, 50)}
	failed := errors.New("temporarily unreadable")
	s, err := NewStatusSamplerWithSource(context.Background(), &statusFixtureSource{frames: frames, errors: map[int]error{3: failed}})
	if err != nil {
		t.Fatal(err)
	}
	for _, want := range []float32{0, 0} {
		got, err := s.Sample(context.Background())
		if err != nil || got.Processes[0].CPU != want {
			t.Fatalf("missing counter handling: %+v %v", got, err)
		}
	}
	if _, err := s.Sample(context.Background()); !errors.Is(err, failed) {
		t.Fatalf("source error lost: %v", err)
	}
	recovered, err := s.Sample(context.Background())
	if err != nil || recovered.CPUUsage != 25 || recovered.Processes[0].CPU != 0 || recovered.Processes[0].DiskReadBPS != 0 {
		t.Fatalf("recovery invented a spike or advanced failed baseline: %+v %v", recovered, err)
	}
	next, err := s.Sample(context.Background())
	if err != nil || next.Processes[0].CPU != 50 || next.Processes[0].DiskReadBPS != 100 {
		t.Fatalf("recovered baseline did not resume: %+v %v", next, err)
	}
	reset, err := s.Sample(context.Background())
	if err != nil || reset.Processes[0].CPU != 0 || reset.Processes[0].DiskReadBPS != 0 || reset.Processes[0].DiskWrittenBPS != 0 {
		t.Fatalf("counter reset underflow: %+v %v", reset, err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := s.Sample(ctx); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancellation lost: %v", err)
	}
}

func TestStatusSamplerLiveReadOnly(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	s, err := NewStatusSampler(ctx)
	if err != nil {
		t.Fatal(err)
	}
	snapshot, err := s.Sample(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if snapshot.CoreCount <= 0 || snapshot.MemTotal == 0 || snapshot.MemTotal != snapshot.MemTotalLabelBytes || snapshot.MemUsed > snapshot.MemTotal || snapshot.UptimeSecs == 0 || snapshot.ProcessCount != len(snapshot.Processes) || math.IsNaN(float64(snapshot.CPUUsage)) || snapshot.CPUUsage < 0 || snapshot.CPUUsage > 100 {
		t.Fatalf("invalid native snapshot: %+v", snapshot)
	}
	for _, p := range snapshot.Processes {
		if p.PID == uint32(os.Getpid()) {
			if p.Name == "" || p.StartTime == 0 || p.MemBytes == 0 || p.Threads == 0 || p.CPU != 0 {
				t.Fatalf("invalid own process / no initial CPU baseline: %+v", p)
			}
			t.Logf("read-only native sample: cores=%d total_memory=%d processes=%d own_start=%d", snapshot.CoreCount, snapshot.MemTotal, snapshot.ProcessCount, p.StartTime)
			return
		}
	}
	t.Fatal("own process missing from native snapshot")
}
