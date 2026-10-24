package platform

import (
	"errors"
	"testing"
)

func TestFanNativeWriteProtocol(t *testing.T) {
	for _, failure := range []string{"", "permission", "firmware", "truncated", "bad-size", "bad-type"} {
		t.Run(failure, func(t *testing.T) {
			api := smcAPI{call: func(conn, selector uint32, in *smcKeyData, inSize uintptr, out *smcKeyData, outSize *uintptr) int32 {
				if conn != 55 || selector != 2 || inSize != 80 || *outSize != 80 || in.Key != smcKeyCode("F0Tg") || in.Command != 6 || in.Info.Size != 4 || in.Info.Type != smcKeyCode("flt ") {
					t.Fatal("invalid write ABI")
				}
				v := smcValue{Type: in.Info.Type, Size: in.Info.Size, Bytes: in.Bytes}
				if n, ok := v.scalar(); !ok || n != 3200 {
					t.Fatal(n, ok)
				}
				switch failure {
				case "permission":
					return -536870207
				case "firmware":
					out.Result = 0x86
				case "truncated":
					*outSize = 79
				}
				return 0
			}}
			v := smcValue{Type: smcKeyCode("flt "), Size: 4}
			if failure == "bad-size" {
				v.Size = 3
			}
			if failure == "bad-type" {
				v.Type = smcKeyCode("nope")
			}
			err := api.write(55, "F0Tg", v, 3200)
			if (err == nil) != (failure == "") {
				t.Fatal(err)
			}
			if failure == "permission" {
				var native FanError
				if !errors.As(err, &native) || native.Kind != FanNeedsRoot {
					t.Fatal(err)
				}
			}
		})
	}
}

func TestFanNativeThermalPressure(t *testing.T) {
	if _, ok := readFanThermalState(); !ok {
		t.Fatal("NSProcessInfo thermalState unavailable")
	}
}
