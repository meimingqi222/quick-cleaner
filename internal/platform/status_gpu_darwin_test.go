package platform

import (
	"strings"
	"testing"

	"github.com/ebitengine/purego"
)

func TestGPURegistryNativeDictionaries(t *testing.T) {
	if !loadBatteryIOKit() {
		t.Fatal("IOKit/CoreFoundation bindings unavailable", batteryIOKit.err)
	}
	lib, err := purego.Dlopen("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		t.Fatal(err)
	}
	defer purego.Dlclose(lib)
	var create func(uintptr, int64, uintptr, uintptr) uintptr
	var set func(uintptr, uintptr, uintptr)
	var number func(uintptr, int32, *int64) uintptr
	purego.RegisterLibFunc(&create, lib, "CFDictionaryCreateMutable")
	purego.RegisterLibFunc(&set, lib, "CFDictionarySetValue")
	purego.RegisterLibFunc(&number, lib, "CFNumberCreate")
	keys, err := purego.Dlsym(lib, "kCFTypeDictionaryKeyCallBacks")
	if err != nil {
		t.Fatal(err)
	}
	values, err := purego.Dlsym(lib, "kCFTypeDictionaryValueCallBacks")
	if err != nil {
		t.Fatal(err)
	}
	for _, tc := range []struct {
		name, model, wantName string
		stats                 map[string]any
		show                  bool
		util, renderer        float32
		hasRenderer           bool
		memory                uint64
	}{
		{"device preferred", "显卡型号", "显卡型号", map[string]any{"Device Utilization %": int64(63), "Renderer Utilization %": int64(41), "In use system memory": int64(1024)}, true, 63, 41, true, 1024},
		{"renderer fallback", "", "AGXAccelerator", map[string]any{"Renderer Utilization %": int64(21)}, true, 21, 21, true, 0},
		{"invalid type fallback", "", "AGXAccelerator", map[string]any{"Device Utilization %": "invalid", "Renderer Utilization %": int64(44)}, true, 44, 44, true, 0},
		{"clamp and absent memory", strings.Repeat("x", 256), "AGXAccelerator", map[string]any{"Device Utilization %": int64(-5), "Renderer Utilization %": int64(180), "In use system memory": int64(-1)}, true, 0, 100, true, 0},
		{"no renderer", "", "AGXAccelerator", map[string]any{"Device Utilization %": int64(101)}, true, 100, 0, false, 0},
		{"virtual accelerator", "", "", map[string]any{"In use system memory": int64(1024)}, false, 0, 0, false, 0},
	} {
		t.Run(tc.name, func(t *testing.T) {
			dict, stats := create(0, 0, keys, values), create(0, 0, keys, values)
			if dict == 0 || stats == 0 {
				t.Fatal("fixture dictionaries unavailable")
			}
			defer batteryIOKit.release(dict)
			defer batteryIOKit.release(stats)
			put := func(dict uintptr, key string, value any) {
				keyRef := batteryIOKit.stringCreate(0, key, 0x08000100)
				defer batteryIOKit.release(keyRef)
				var valueRef uintptr
				switch v := value.(type) {
				case string:
					valueRef = batteryIOKit.stringCreate(0, v, 0x08000100)
				case int64:
					valueRef = number(0, 4, &v)
				case uintptr:
					set(dict, keyRef, v)
					return
				}
				if keyRef == 0 || valueRef == 0 {
					t.Fatal("fixture property unavailable")
				}
				set(dict, keyRef, valueRef)
				batteryIOKit.release(valueRef)
			}
			put(dict, "PerformanceStatistics", stats)
			put(dict, "model", tc.model)
			put(dict, "IOClass", "AGXAccelerator")
			for key, value := range tc.stats {
				put(stats, key, value)
			}
			got := gpuRegistryReading(dict, 2)
			if !tc.show {
				if got != nil {
					t.Fatal("virtual accelerator must be skipped", got)
				}
				return
			}
			if got == nil || got.ID != "2" || got.Name == nil || *got.Name != tc.wantName || got.Utilization == nil || *got.Utilization != tc.util || (got.RendererUtilization != nil) != tc.hasRenderer || got.RendererUtilization != nil && *got.RendererUtilization != tc.renderer || (got.VRAMInUse != nil) != (tc.memory > 0) || got.VRAMInUse != nil && *got.VRAMInUse != tc.memory || got.TempC != nil {
				t.Fatalf("registry interpretation mismatch: %+v", got)
			}
		})
	}
}
