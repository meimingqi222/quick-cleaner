package core

import (
	"context"
	"errors"
	"testing"
)

type trayFixtureSource struct {
	frames  []TrayCounters
	errors  map[int]error
	refresh []bool
}

func (s *trayFixtureSource) ReadTray(ctx context.Context, thermal bool) (TrayCounters, error) {
	if err := ctx.Err(); err != nil {
		return TrayCounters{}, err
	}
	i := len(s.refresh)
	s.refresh = append(s.refresh, thermal)
	return s.frames[i], s.errors[i]
}

func TestTraySamplerPersistentCPUAndTemperature(t *testing.T) {
	temp := float32(76.5)
	source := &trayFixtureSource{frames: []TrayCounters{
		{CPUTotal: 100, CPUBusy: 20},
		{CPUTotal: 200, CPUBusy: 45.5, Snapshot: TraySnapshot{MemoryUsed: 31, MemoryTotal: 87, SwapUsed: 9, TempC: &temp}},
		{CPUTotal: 300, CPUBusy: 100, Snapshot: TraySnapshot{MemoryUsed: 39, MemoryTotal: 87, SwapUsed: 11}},
		{CPUTotal: 400, CPUBusy: 101},
	}}
	s, err := NewTraySamplerWithSource(context.Background(), source)
	if err != nil {
		t.Fatal(err)
	}
	first, err := s.Sample(context.Background(), true)
	if err != nil {
		t.Fatal(err)
	}
	if first.CPUPct != 26 || first.MemoryUsed != 31 || first.MemoryTotal != 87 || first.SwapUsed != 9 || first.TempC == nil || *first.TempC != 76.5 {
		t.Fatalf("first: %+v", first)
	}
	*first.TempC = -1
	second, err := s.Sample(context.Background(), false)
	if err != nil {
		t.Fatal(err)
	}
	if second.CPUPct != 55 || second.MemoryUsed != 39 || second.SwapUsed != 11 || second.TempC == nil || *second.TempC != 76.5 {
		t.Fatalf("second: %+v", second)
	}
	third, err := s.Sample(context.Background(), true)
	if err != nil || third.TempC != nil || third.CPUPct != 1 {
		t.Fatalf("third: %+v err=%v", third, err)
	}
}

func TestTraySamplerFailedSampleDoesNotReplaceBaseline(t *testing.T) {
	cause := errors.New("memory unavailable")
	source := &trayFixtureSource{frames: []TrayCounters{
		{CPUTotal: 100, CPUBusy: 20}, {CPUTotal: 999, CPUBusy: 999}, {CPUTotal: 300, CPUBusy: 90},
	}, errors: map[int]error{1: cause}}
	s, err := NewTraySamplerWithSource(context.Background(), source)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.Sample(context.Background(), false); !errors.Is(err, cause) {
		t.Fatal(err)
	}
	got, err := s.Sample(context.Background(), false)
	if err != nil || got.CPUPct != 35 {
		t.Fatalf("baseline replaced: %+v %v", got, err)
	}
}

func TestTraySamplerNativeMemoryAndCancellation(t *testing.T) {
	s, err := NewTraySampler(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	got, err := s.Sample(context.Background(), false)
	if err != nil || got.MemoryTotal == 0 || got.MemoryUsed > got.MemoryTotal {
		t.Fatalf("snapshot=%+v err=%v", got, err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err = s.Sample(ctx, false); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}
