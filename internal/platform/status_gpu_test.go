package platform

import (
	"math"
	"testing"
)

func TestGPUInstanceParsing(t *testing.T) {
	for _, tc := range []struct {
		input string
		luid  uint64
		ok    bool
	}{
		{"pid_12680_luid_0x00000000_0x00011365_phys_0_eng_0_engtype_3d", 0x11365, true},
		{"luid_0x00000001_0x00000002_phys_0", 0x100000002, true},
		{"luid_0xffffffff_0xffffffff_phys_0", math.MaxUint64, true},
		{"luid_0x+1_0x+2", 0x100000002, true},
		{"luid_0X0_0x1", 0, false},
		{"luid_0x100000000_0x1", 0, false},
		{"luid_0x0_0x-1", 0, false},
		{"luid_0x0_0x1luid_0x0_0x2", 1, true},
		{"no luid here", 0, false},
	} {
		got, ok := gpuParseLUID(tc.input)
		if got != tc.luid || ok != tc.ok {
			t.Errorf("%q: got (%x,%v), want (%x,%v)", tc.input, got, ok, tc.luid, tc.ok)
		}
	}
	if _, ok := gpuParseEngine("luid_0x0_0x1_phys_0"); ok {
		t.Fatal("memory instance became an engine")
	}
	engine, ok := gpuParseEngine("pid_1_luid_0x0_0x1_phys_0_eng_10_engtype_video codec 0")
	if !ok || engine != "phys_0_eng_10_engtype_video codec 0" || gpuEngineType(engine) != "video codec 0" {
		t.Fatal(engine, ok)
	}
	if gpuEngineType("engtype_3dengtype_copy") != "3d" {
		t.Fatal("must use split nth(1), not the entire suffix")
	}
}

func TestGPUAggregationFixtures(t *testing.T) {
	name := "NVIDIA Example"
	engines := map[string]float64{
		"pid_1_luid_0x0_0x1_phys_0_eng_0_engtype_3d":   40,
		"pid_2_luid_0x0_0x1_phys_0_eng_0_engtype_3d":   20,
		"pid_1_luid_0x0_0x1_phys_0_eng_1_engtype_copy": 40,
		"pid_1_luid_0x0_0x2_phys_0_eng_1_engtype_copy": 60,
		"pid_1_luid_0x0_0x2_phys_0_eng_2_engtype_copy": 60,
		"pid_1_luid_0x0_0x4_phys_0_eng_0_engtype_3d":   80,
		"not a gpu": 100,
	}
	memory := map[string]float64{"luid_0x0_0x1_phys_0": 1024, "luid_0x0_0x2_phys_0": 900, "luid_0x0_0x3_phys_0": 512, "luid_0x0_0x5_phys_0": 0}
	gpus := aggregateGPUs(engines, memory, map[uint64]gpuAdapter{1: {name: &name}, 4: {software: true}, 6: {name: &name}})
	if len(gpus) != 3 {
		t.Fatalf("want three hardware PDH adapters: %+v", gpus)
	}
	for i, gpu := range gpus {
		if gpu.ID != []string{"0000000000000001", "0000000000000002", "0000000000000003"}[i] {
			t.Fatal(gpu.ID)
		}
		if *gpu.Utilization != []float32{60, 60, 0}[i] || *gpu.VRAMInUse != []uint64{1024, 900, 512}[i] {
			t.Fatalf("bad values: %+v", gpu)
		}
		if gpu.TempC != nil {
			t.Fatal("aggregation invented temperature")
		}
	}
	if gpus[0].Name == nil || *gpus[0].Name != name || *gpus[0].RendererUtilization != 60 {
		t.Fatal(gpus[0])
	}
	if gpus[1].Name != nil || gpus[1].RendererUtilization != nil || gpus[2].RendererUtilization != nil {
		t.Fatal("unknown fields were invented")
	}
	if len(aggregateGPUs(nil, nil, map[uint64]gpuAdapter{1: {name: &name}})) != 0 {
		t.Fatal("DXGI alone must not invent PDH adapters")
	}
	*gpus[0].Name = "changed"
	if name != "NVIDIA Example" {
		t.Fatal("reading name aliases cached DXGI metadata")
	}
}

func TestGPURustNumericSemantics(t *testing.T) {
	for _, tc := range []struct {
		value       float64
		utilization float32
		rendererNaN bool
	}{
		{math.NaN(), 0, true}, {math.Inf(1), 100, false}, {math.Inf(-1), 0, false}, {-50, 0, false}, {120, 100, false},
	} {
		g := aggregateGPUs(map[string]float64{"pid_1_luid_0x0_0x1_phys_0_eng_0_engtype_3d": tc.value}, nil, nil)[0]
		if *g.Utilization != tc.utilization || math.IsNaN(float64(*g.RendererUtilization)) != tc.rendererNaN {
			t.Fatalf("%v: %+v", tc.value, g)
		}
		if g.VRAMInUse != nil || g.Name != nil || g.TempC != nil {
			t.Fatal("unknown field became known")
		}
	}
	for _, tc := range []struct {
		value float64
		want  uint64
		count int
	}{
		{math.NaN(), 0, 0}, {0, 0, 0}, {-1, 0, 0}, {0.5, 0, 1}, {42.9, 42, 1}, {math.Inf(1), math.MaxUint64, 1}, {float64(math.MaxUint64), math.MaxUint64, 1},
	} {
		g := aggregateGPUs(nil, map[string]float64{"luid_0x0_0x1_phys_0": tc.value}, nil)
		if len(g) != tc.count {
			t.Fatalf("VRAM %v: count %d", tc.value, len(g))
		}
		if tc.count != 0 && *g[0].VRAMInUse != tc.want {
			t.Fatalf("VRAM %v: %d", tc.value, *g[0].VRAMInUse)
		}
	}
}
