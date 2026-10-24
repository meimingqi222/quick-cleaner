package platform

import (
	"encoding/binary"
	"math"
	"reflect"
	"testing"
	"unsafe"
)

func smcTestFloat(n float32) smcValue {
	v := smcValue{Type: smcKeyCode("flt "), Size: 4}
	binary.LittleEndian.PutUint32(v.Bytes[:4], math.Float32bits(n))
	return v
}

func TestSMCABI(t *testing.T) {
	v := smcKeyData{}
	if unsafe.Sizeof(v) != 80 || unsafe.Offsetof(v.Version) != 4 || unsafe.Offsetof(v.PowerLimit) != 12 ||
		unsafe.Offsetof(v.Info) != 28 || unsafe.Offsetof(v.Result) != 40 || unsafe.Offsetof(v.Command) != 42 ||
		unsafe.Offsetof(v.Data) != 44 || unsafe.Offsetof(v.Bytes) != 48 {
		t.Fatal("AppleSMC C ABI mismatch")
	}
	if smcKeyCode("FNum") != 0x464e756d || smcKeyCode("x") != 0x78202020 || smcKeyCode("ABCDE") != 0x41424344 {
		t.Fatal("key must be big endian, space padded and truncated")
	}
}

func TestSMCScalar(t *testing.T) {
	for _, tt := range []struct {
		typeName string
		size     uint32
		bytes    []byte
		want     float32
		ok       bool
	}{
		{"fpe2", 2, []byte{0x17, 0x71}, 1500.25, true},
		{"fpe2", 3, []byte{0xff, 0xff}, 16383.75, true},
		{"sp78", 2, []byte{0xff, 0x80}, -0.5, true},
		{"sp78", 2, []byte{0x55, 0x40}, 85.25, true},
		{"flt ", 4, []byte{0, 0x80, 0xaa, 0x42}, 85.25, true},
		{"ui8 ", 1, []byte{255}, 255, true},
		{"ui8 ", 2, []byte{4}, 0, false},
		{"flt ", 3, nil, 0, false},
		{"fpe2", 1, nil, 0, false},
		{"sp78", 1, nil, 0, false},
		{"ui16", 2, nil, 0, false},
	} {
		v := smcValue{Type: smcKeyCode(tt.typeName), Size: tt.size}
		copy(v.Bytes[:], tt.bytes)
		if got, ok := v.scalar(); ok != tt.ok || got != tt.want {
			t.Errorf("%q/%d = %v,%v want %v,%v", tt.typeName, tt.size, got, ok, tt.want, tt.ok)
		}
	}
}

func TestSMCThermalFans(t *testing.T) {
	for _, tt := range []struct {
		count float32
		want  int
	}{
		{-1, 0}, {0, 0}, {0.9, 0}, {1, 1}, {2.9, 2}, {4, 4}, {255, 4},
		{float32(math.Inf(1)), 4}, {float32(math.Inf(-1)), 0}, {float32(math.NaN()), 0},
	} {
		var requested []string
		reading := smcReadThermal(func(key string) (smcValue, bool) {
			if key == "FNum" {
				return smcTestFloat(tt.count), true
			}
			if len(key) == 4 && key[0] == 'F' {
				requested = append(requested, key)
				return smcTestFloat(float32(key[1] - '0')), true
			}
			return smcValue{}, false
		})
		if len(reading.Fans) != tt.want || len(requested) != tt.want {
			t.Fatalf("count %v: %+v keys=%v", tt.count, reading, requested)
		}
	}
	values := map[string]smcValue{"FNum": smcTestFloat(4), "F0Ac": smcTestFloat(-1), "F1Ac": smcTestFloat(0), "F2Ac": smcTestFloat(20_000), "F3Ac": smcTestFloat(19999.5)}
	read := func(key string) (smcValue, bool) { v, ok := values[key]; return v, ok }
	if got := smcReadThermal(read).Fans; !reflect.DeepEqual(got, []FanInfo{{Label: "Fan 2", RPM: 0}, {Label: "Fan 4", RPM: 19999.5}}) {
		t.Fatalf("fans: %+v", got)
	}
	for _, invalid := range []float32{float32(math.NaN()), float32(math.Inf(1)), float32(math.Inf(-1))} {
		values["F1Ac"], values["F3Ac"] = smcTestFloat(invalid), smcTestFloat(invalid)
		if got := smcReadThermal(read).Fans; len(got) != 0 {
			t.Fatalf("invalid RPM accepted: %+v", got)
		}
	}
	delete(values, "FNum")
	if got := smcReadThermal(read).Fans; len(got) != 0 {
		t.Fatal("missing count must not probe fans")
	}
}

func TestSMCThermalTemperaturePreference(t *testing.T) {
	values := map[string]smcValue{"TC0P": smcTestFloat(40), "Tp01": smcTestFloat(70), "Tp05": smcTestFloat(99.5), "Tp09": smcTestFloat(80), "Tp0D": smcTestFloat(150)}
	read := func(key string) (smcValue, bool) { v, ok := values[key]; return v, ok }
	if got := smcReadThermal(read).CPUTemp; got == nil || *got != 40 {
		t.Fatalf("Intel package preference: %v", got)
	}
	for _, invalid := range []float32{-1, 150, float32(math.NaN()), float32(math.Inf(1))} {
		values["TC0P"] = smcTestFloat(invalid)
		if got := smcReadThermal(read).CPUTemp; got == nil || *got != 99.5 {
			t.Fatalf("hottest P-core for %v: %v", invalid, got)
		}
	}
	clear(values)
	if got := smcReadThermal(read); got.CPUTemp != nil || len(got.Fans) != 0 {
		t.Fatalf("missing sensors: %+v", got)
	}
	values["Tp0D"] = smcTestFloat(0)
	if got := smcReadThermal(read).CPUTemp; got == nil || *got != 0 {
		t.Fatal("zero is a valid temperature")
	}
}
