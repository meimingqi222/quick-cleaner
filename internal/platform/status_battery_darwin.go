package platform

import (
	"sync"

	"github.com/ebitengine/purego"
)

var batteryIOKit struct {
	sync.Once
	err           error
	matching      func(string) uintptr
	service       func(uint32, uintptr) uint32
	services      func(uint32, uintptr, *uint32) int32
	next          func(uint32) uint32
	objectRelease func(uint32) int32
	properties    func(uint32, *uintptr, uintptr, uint32) int32
	release       func(uintptr)
	typeID        func(uintptr) uintptr
	numberTypeID  func() uintptr
	booleanTypeID func() uintptr
	stringTypeID  func() uintptr
	dictTypeID    func() uintptr
	value         func(uintptr, uintptr) uintptr
	number        func(uintptr, int32, *int64) bool
	boolean       func(uintptr) bool
	stringCreate  func(uintptr, string, uint32) uintptr
	stringValue   func(uintptr, *byte, int64, uint32) bool
}

func loadBatteryIOKit() bool {
	batteryIOKit.Do(func() {
		for _, library := range []struct {
			path     string
			bindings []struct {
				name string
				fn   any
			}
		}{
			{"/System/Library/Frameworks/IOKit.framework/IOKit", []struct {
				name string
				fn   any
			}{
				{"IOServiceMatching", &batteryIOKit.matching},
				{"IOServiceGetMatchingService", &batteryIOKit.service},
				{"IOServiceGetMatchingServices", &batteryIOKit.services},
				{"IOIteratorNext", &batteryIOKit.next},
				{"IOObjectRelease", &batteryIOKit.objectRelease},
				{"IORegistryEntryCreateCFProperties", &batteryIOKit.properties},
			}},
			{"/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation", []struct {
				name string
				fn   any
			}{
				{"CFRelease", &batteryIOKit.release}, {"CFGetTypeID", &batteryIOKit.typeID},
				{"CFNumberGetTypeID", &batteryIOKit.numberTypeID}, {"CFBooleanGetTypeID", &batteryIOKit.booleanTypeID},
				{"CFStringGetTypeID", &batteryIOKit.stringTypeID}, {"CFDictionaryGetTypeID", &batteryIOKit.dictTypeID},
				{"CFDictionaryGetValue", &batteryIOKit.value}, {"CFNumberGetValue", &batteryIOKit.number},
				{"CFBooleanGetValue", &batteryIOKit.boolean}, {"CFStringCreateWithCString", &batteryIOKit.stringCreate},
				{"CFStringGetCString", &batteryIOKit.stringValue},
			}},
		} {
			var handle uintptr
			handle, batteryIOKit.err = purego.Dlopen(library.path, purego.RTLD_NOW|purego.RTLD_LOCAL)
			if batteryIOKit.err != nil {
				return
			}
			for _, binding := range library.bindings {
				var address uintptr
				address, batteryIOKit.err = purego.Dlsym(handle, binding.name)
				if batteryIOKit.err != nil {
					return
				}
				purego.RegisterFunc(binding.fn, address)
			}
		}
	})
	return batteryIOKit.err == nil
}

func batteryRegistryValue(dict uintptr, key string) uintptr {
	keyRef := batteryIOKit.stringCreate(0, key, 0x08000100)
	if keyRef == 0 {
		return 0
	}
	defer batteryIOKit.release(keyRef)
	// Get-rule references remain borrowed from the owned properties dictionary.
	return batteryIOKit.value(dict, keyRef)
}

func batteryRegistryNumber(dict uintptr, key string) *int64 {
	v := batteryRegistryValue(dict, key)
	if v == 0 || batteryIOKit.typeID(v) != batteryIOKit.numberTypeID() {
		return nil
	}
	var out int64
	if !batteryIOKit.number(v, 4, &out) {
		return nil
	}
	return &out
}

func batteryRegistryBoolean(dict uintptr, key string) *bool {
	v := batteryRegistryValue(dict, key)
	if v == 0 || batteryIOKit.typeID(v) != batteryIOKit.booleanTypeID() {
		return nil
	}
	out := batteryIOKit.boolean(v)
	return &out
}

// ReadBattery reads AppleSmartBattery; desktops and unavailable properties return nil.
func ReadBattery() *BatteryReading {
	if !loadBatteryIOKit() {
		return nil
	}
	matcher := batteryIOKit.matching("AppleSmartBattery")
	if matcher == 0 {
		return nil
	}
	// IOServiceGetMatchingService consumes matcher, even when no service exists.
	service := batteryIOKit.service(0, matcher)
	if service == 0 {
		return nil
	}
	var dict uintptr
	result := batteryIOKit.properties(service, &dict, 0, 0)
	batteryIOKit.objectRelease(service)
	if result != 0 || dict == 0 {
		return nil
	}
	defer batteryIOKit.release(dict)
	boolValue := func(key string) bool {
		v := batteryRegistryBoolean(dict, key)
		return v != nil && *v
	}
	return batteryFromRegistry(batteryRegistryData{
		installed: batteryRegistryBoolean(dict, "BatteryInstalled"),
		current:   batteryRegistryNumber(dict, "CurrentCapacity"), maximum: batteryRegistryNumber(dict, "MaxCapacity"),
		rawMaximum: batteryRegistryNumber(dict, "AppleRawMaxCapacity"), design: batteryRegistryNumber(dict, "DesignCapacity"),
		remaining: batteryRegistryNumber(dict, "TimeRemaining"), cycles: batteryRegistryNumber(dict, "CycleCount"),
		designCycles: batteryRegistryNumber(dict, "DesignCycleCount9C"), temperature: batteryRegistryNumber(dict, "Temperature"),
		charging: boolValue("IsCharging"), external: boolValue("ExternalConnected"), fullyCharged: boolValue("FullyCharged"),
	})
}
