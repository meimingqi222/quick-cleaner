package platform

import (
	"sync"
	"syscall"
	"unicode/utf8"
	"unsafe"
)

type nvmlGPUDevice struct {
	name   string
	handle uintptr
}
type nvmlGPUState struct {
	library     *syscall.DLL
	temperature *syscall.Proc
	devices     []nvmlGPUDevice
}

var nvmlGPUOnce sync.Once
var nvmlGPULoaded *nvmlGPUState

func gpuTemperatures() map[string]float32 {
	nvmlGPUOnce.Do(func() { nvmlGPULoaded = loadNVMLGPU() })
	out := make(map[string]float32)
	if nvmlGPULoaded == nil {
		return out
	}
	for _, device := range nvmlGPULoaded.devices {
		var celsius uint32
		status, _, _ := nvmlGPULoaded.temperature.Call(device.handle, 0, uintptr(unsafe.Pointer(&celsius)))
		if int32(status) == 0 && celsius > 0 && celsius < 150 {
			out[device.name] = float32(celsius)
		}
	}
	return out
}

func loadNVMLGPU() *nvmlGPUState {
	library, err := syscall.LoadDLL("nvml.dll")
	if err != nil {
		return nil
	}
	// Keep NVML loaded even on failure, as in Rust. No Shutdown: device handles
	// and the initialized driver session must remain valid for later samples.
	procs := make([]*syscall.Proc, 5)
	for i, name := range []string{"nvmlDeviceGetCount_v2", "nvmlDeviceGetHandleByIndex_v2", "nvmlDeviceGetName", "nvmlDeviceGetTemperature", "nvmlInit_v2"} {
		procs[i], err = library.FindProc(name)
		if err != nil {
			return nil
		}
	}
	if status, _, _ := procs[4].Call(); int32(status) != 0 {
		return nil
	}
	var count uint32
	if status, _, _ := procs[0].Call(uintptr(unsafe.Pointer(&count))); int32(status) != 0 {
		return nil
	}
	state := &nvmlGPUState{library: library, temperature: procs[3]}
	for index := uint32(0); index < count; index++ {
		var handle uintptr
		if status, _, _ := procs[1].Call(uintptr(index), uintptr(unsafe.Pointer(&handle))); int32(status) != 0 {
			continue
		}
		var buffer [96]byte
		if status, _, _ := procs[2].Call(handle, uintptr(unsafe.Pointer(&buffer[0])), uintptr(len(buffer))); int32(status) != 0 {
			continue
		}
		end := 0
		for end < len(buffer) && buffer[end] != 0 {
			end++
		}
		name := nvmlLossyName(buffer[:end])
		if name != "" {
			state.devices = append(state.devices, nvmlGPUDevice{name, handle})
		}
	}
	if len(state.devices) == 0 {
		return nil
	}
	return state
}

// Match String::from_utf8_lossy, including incomplete multibyte sequences.
func nvmlLossyName(b []byte) string {
	out := make([]byte, 0, len(b))
	for len(b) > 0 {
		r, size := utf8.DecodeRune(b)
		if r != utf8.RuneError || size != 1 {
			out = append(out, b[:size]...)
			b = b[size:]
			continue
		}
		n := 1
		if b[0] >= 0xc2 && b[0] <= 0xf4 {
			want := 2
			if b[0] >= 0xe0 {
				want = 3
			}
			if b[0] >= 0xf0 {
				want = 4
			}
			for n < want && n < len(b) {
				c := b[n]
				if c < 0x80 || c > 0xbf || (n == 1 && ((b[0] == 0xe0 && c < 0xa0) || (b[0] == 0xed && c > 0x9f) || (b[0] == 0xf0 && c < 0x90) || (b[0] == 0xf4 && c > 0x8f))) {
					break
				}
				n++
			}
		}
		out = append(out, "\ufffd"...)
		b = b[n:]
	}
	return string(out)
}
