package platform

import (
	"bytes"
	"strconv"
	"unicode/utf8"
)

// ReadGPUs preserves IORegistry enumeration order and skips virtual/unreadable
// accelerators rather than inventing a zero utilization measurement for them.
func ReadGPUs() []GpuReading {
	if !loadBatteryIOKit() {
		return nil
	}
	matcher := batteryIOKit.matching("IOAccelerator")
	if matcher == 0 {
		return nil
	}
	var iterator uint32
	if batteryIOKit.services(0, matcher, &iterator) != 0 || iterator == 0 {
		return nil
	}
	defer batteryIOKit.objectRelease(iterator)
	var out []GpuReading
	for entry := batteryIOKit.next(iterator); entry != 0; entry = batteryIOKit.next(iterator) {
		var dict uintptr
		result := batteryIOKit.properties(entry, &dict, 0, 0)
		batteryIOKit.objectRelease(entry)
		if result != 0 || dict == 0 {
			continue
		}
		if reading := gpuRegistryReading(dict, len(out)); reading != nil {
			out = append(out, *reading)
		}
		batteryIOKit.release(dict)
	}
	return out
}

func gpuRegistryReading(dict uintptr, index int) *GpuReading {
	stats := batteryRegistryValue(dict, "PerformanceStatistics")
	if stats == 0 || batteryIOKit.typeID(stats) != batteryIOKit.dictTypeID() {
		return nil
	}
	util := batteryRegistryNumber(stats, "Device Utilization %")
	if util == nil {
		util = batteryRegistryNumber(stats, "Renderer Utilization %")
	}
	if util == nil {
		return nil
	}
	value := float32(min(int64(100), max(int64(0), *util)))
	reading := &GpuReading{ID: strconv.Itoa(index), Utilization: &value}
	reading.Name = gpuRegistryString(dict, "model")
	if reading.Name == nil {
		reading.Name = gpuRegistryString(dict, "IOClass")
	}
	if renderer := batteryRegistryNumber(stats, "Renderer Utilization %"); renderer != nil {
		value := float32(min(int64(100), max(int64(0), *renderer)))
		reading.RendererUtilization = &value
	}
	if memory := batteryRegistryNumber(stats, "In use system memory"); memory != nil && *memory > 0 {
		value := uint64(*memory)
		reading.VRAMInUse = &value
	}
	return reading
}

func gpuRegistryString(dict uintptr, key string) *string {
	v := batteryRegistryValue(dict, key)
	if v == 0 || batteryIOKit.typeID(v) != batteryIOKit.stringTypeID() {
		return nil
	}
	var buffer [256]byte
	if !batteryIOKit.stringValue(v, &buffer[0], int64(len(buffer)), 0x08000100) {
		return nil
	}
	end := bytes.IndexByte(buffer[:], 0)
	if end <= 0 || !utf8.Valid(buffer[:end]) {
		return nil
	}
	value := string(buffer[:end])
	return &value
}
