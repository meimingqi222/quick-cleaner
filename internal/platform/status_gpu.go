package platform

import (
	"fmt"
	"math"
	"sort"
	"strconv"
	"strings"
)

type gpuAdapter struct {
	name     *string
	software bool
}

func gpuParseLUID(instance string) (uint64, bool) {
	parts := strings.Split(instance, "luid_")
	if len(parts) < 2 {
		return 0, false
	}
	halves := strings.Split(parts[1], "_")
	if len(halves) < 2 {
		return 0, false
	}
	parse := func(s string) (uint64, error) {
		if !strings.HasPrefix(s, "0x") {
			return 0, strconv.ErrSyntax
		}
		return strconv.ParseUint(strings.TrimPrefix(s[2:], "+"), 16, 32)
	}
	high, err := parse(halves[0])
	if err != nil {
		return 0, false
	}
	low, err := parse(halves[1])
	if err != nil {
		return 0, false
	}
	return high<<32 | low, true
}

func gpuParseEngine(instance string) (string, bool) {
	i := strings.Index(instance, "_phys_")
	if i < 0 {
		return "", false
	}
	engine := instance[i+1:]
	return engine, strings.Contains(engine, "engtype_")
}

func gpuEngineType(engine string) string {
	parts := strings.Split(engine, "engtype_")
	if len(parts) < 2 {
		return ""
	}
	return parts[1]
}

// Rust f64::max ignores NaN, whereas Go math.Max propagates it.
func gpuMax(a, b float64) float64 {
	if math.IsNaN(a) {
		return b
	}
	if math.IsNaN(b) {
		return a
	}
	return math.Max(a, b)
}

func gpuPercent(v float64) *float32 {
	value := float32(math.Min(math.Max(v, 0), 100))
	return &value
}

func aggregateGPUs(engines, memory map[string]float64, adapters map[uint64]gpuAdapter) []GpuReading {
	perAdapter := make(map[uint64]map[string]float64)
	vram := make(map[uint64]uint64)
	luids := make(map[uint64]bool)
	for instance, value := range engines {
		luid, ok := gpuParseLUID(instance)
		if !ok {
			continue
		}
		engine, ok := gpuParseEngine(instance)
		if !ok {
			continue
		}
		if perAdapter[luid] == nil {
			perAdapter[luid] = make(map[string]float64)
		}
		// Sum processes on one specific engine, never independent engines of one type.
		perAdapter[luid][engine] += value
		luids[luid] = true
	}
	for instance, value := range memory {
		luid, ok := gpuParseLUID(instance)
		if !ok || !(value > 0) {
			continue
		}
		// Rust's float-to-u64 cast saturates, including +infinity.
		bytes := uint64(math.MaxUint64)
		if value < float64(math.MaxUint64) {
			bytes = uint64(value)
		}
		vram[luid] = bytes
		luids[luid] = true
	}
	ordered := make([]uint64, 0, len(luids))
	for luid := range luids {
		if !adapters[luid].software {
			ordered = append(ordered, luid)
		}
	}
	sort.Slice(ordered, func(i, j int) bool { return ordered[i] < ordered[j] })
	out := make([]GpuReading, 0, len(ordered))
	for _, luid := range ordered {
		util := 0.0
		var renderer *float64
		for engine, value := range perAdapter[luid] {
			util = gpuMax(util, value)
			if gpuEngineType(engine) == "3d" {
				if renderer == nil {
					v := value
					renderer = &v
				} else {
					*renderer = gpuMax(*renderer, value)
				}
			}
		}
		gpu := GpuReading{ID: fmt.Sprintf("%016x", luid), Name: adapters[luid].name, Utilization: gpuPercent(util)}
		if gpu.Name != nil {
			name := *gpu.Name
			gpu.Name = &name
		}
		if renderer != nil {
			gpu.RendererUtilization = gpuPercent(*renderer)
		}
		if bytes, ok := vram[luid]; ok {
			gpu.VRAMInUse = &bytes
		}
		out = append(out, gpu)
	}
	return out
}
