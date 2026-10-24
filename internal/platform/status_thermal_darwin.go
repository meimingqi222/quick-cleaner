package platform

import (
	"sync"
	"unsafe"

	"github.com/ebitengine/purego"
)

// Separate from the registry-only battery bindings: SMC owns a user connection.
type smcAPI struct {
	matching func(string) uintptr
	service  func(uint32, uintptr) uint32
	open     func(uint32, uint32, uint32, *uint32) int32
	close    func(uint32) int32
	release  func(uint32) int32
	taskSelf func() uint32
	call     func(uint32, uint32, *smcKeyData, uintptr, *smcKeyData, *uintptr) int32
}

var smcIOKit struct {
	sync.Once
	smcAPI
	err error
}

func loadSMCIOKit() bool {
	smcIOKit.Do(func() {
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
				{"IOServiceMatching", &smcIOKit.matching},
				{"IOServiceGetMatchingService", &smcIOKit.service},
				{"IOServiceOpen", &smcIOKit.open},
				{"IOServiceClose", &smcIOKit.close},
				{"IOObjectRelease", &smcIOKit.release},
				{"IOConnectCallStructMethod", &smcIOKit.call},
			}},
			{"/usr/lib/libSystem.B.dylib", []struct {
				name string
				fn   any
			}{
				{"mach_task_self", &smcIOKit.taskSelf},
			}},
		} {
			var handle uintptr
			handle, smcIOKit.err = purego.Dlopen(library.path, purego.RTLD_NOW|purego.RTLD_LOCAL)
			if smcIOKit.err != nil {
				return
			}
			for _, binding := range library.bindings {
				var address uintptr
				address, smcIOKit.err = purego.Dlsym(handle, binding.name)
				if smcIOKit.err != nil {
					return
				}
				purego.RegisterFunc(binding.fn, address)
			}
		}
	})
	return smcIOKit.err == nil
}

func (api *smcAPI) read(conn uint32, key string) (smcValue, bool) {
	input := smcKeyData{Key: smcKeyCode(key), Command: 9}
	call := func() (smcKeyData, bool) {
		var output smcKeyData
		size := unsafe.Sizeof(output)
		kr := api.call(conn, 2, &input, unsafe.Sizeof(input), &output, &size)
		return output, kr == 0 && output.Result == 0 && size == unsafe.Sizeof(output)
	}
	info, ok := call()
	if !ok || info.Info.Size == 0 || info.Info.Size > 32 {
		return smcValue{}, false
	}
	input = smcKeyData{Key: input.Key, Info: smcKeyInfo{Size: info.Info.Size, Type: info.Info.Type}, Command: 5}
	output, ok := call()
	if !ok {
		return smcValue{}, false
	}
	return smcValue{Type: info.Info.Type, Size: info.Info.Size, Bytes: output.Bytes}, true
}

func (api *smcAPI) readThermal() ThermalReading {
	matcher := api.matching("AppleSMC")
	if matcher == 0 {
		return ThermalReading{}
	}
	// GetMatchingService consumes the matcher on success and failure.
	device := api.service(0, matcher)
	if device == 0 {
		return ThermalReading{}
	}
	var conn uint32
	kr := api.open(device, api.taskSelf(), 0, &conn)
	api.release(device)
	if kr != 0 || conn == 0 {
		return ThermalReading{}
	}
	defer api.close(conn)
	return smcReadThermal(func(key string) (smcValue, bool) { return api.read(conn, key) })
}

// ReadThermal reads AppleSMC without writes, elevation, or external commands.
// Missing hardware, permissions, and unavailable sensors leave fields empty.
func ReadThermal() ThermalReading {
	if !loadSMCIOKit() {
		return ThermalReading{}
	}
	return smcIOKit.readThermal()
}
