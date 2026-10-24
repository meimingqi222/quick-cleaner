package platform

import (
	"encoding/binary"
	"fmt"
	"math"
	"unsafe"
)

type smcVersion struct {
	Major, Minor, Build, Reserved uint8
	Release                       uint16
}

type smcPowerLimit struct {
	Version, Length  uint16
	CPU, GPU, Memory uint32
}

type smcKeyInfo struct {
	Size, Type uint32
	Attributes uint8
	_          [3]byte
}

// AppleSMC's user-client structure is 80 bytes, including C alignment padding.
type smcKeyData struct {
	Key                     uint32
	Version                 smcVersion
	_                       [2]byte
	PowerLimit              smcPowerLimit
	Info                    smcKeyInfo
	Result, Status, Command uint8
	_                       byte
	Data                    uint32
	Bytes                   [32]byte
}

// Fail compilation rather than quietly calling the kernel with a different ABI.
var (
	_ [80 - unsafe.Sizeof(smcKeyData{})]byte
	_ [unsafe.Sizeof(smcKeyData{}) - 80]byte
	_ [28 - unsafe.Offsetof(smcKeyData{}.Info)]byte
	_ [unsafe.Offsetof(smcKeyData{}.Info) - 28]byte
	_ [48 - unsafe.Offsetof(smcKeyData{}.Bytes)]byte
	_ [unsafe.Offsetof(smcKeyData{}.Bytes) - 48]byte
)

func smcKeyCode(name string) uint32 {
	b := [4]byte{' ', ' ', ' ', ' '}
	copy(b[:], name)
	return binary.BigEndian.Uint32(b[:])
}

type smcValue struct {
	Type  uint32
	Size  uint32
	Bytes [32]byte
}

func (v smcValue) scalar() (float32, bool) {
	switch {
	case v.Type == smcKeyCode("fpe2") && v.Size >= 2:
		return float32(binary.BigEndian.Uint16(v.Bytes[:2])) / 4, true
	case v.Type == smcKeyCode("sp78") && v.Size >= 2:
		return float32(int16(binary.BigEndian.Uint16(v.Bytes[:2]))) / 256, true
	case v.Type == smcKeyCode("flt ") && v.Size == 4:
		return math.Float32frombits(binary.LittleEndian.Uint32(v.Bytes[:4])), true
	case v.Type == smcKeyCode("ui8 ") && v.Size == 1:
		return float32(v.Bytes[0]), true
	default:
		return 0, false
	}
}

func smcReadThermal(read func(string) (smcValue, bool)) ThermalReading {
	var reading ThermalReading
	scalar := func(key string) (float32, bool) {
		v, ok := read(key)
		if !ok {
			return 0, false
		}
		return v.scalar()
	}
	if count, ok := scalar("FNum"); ok {
		// Match Rust's saturating float-to-usize cast before capping at four.
		for i := 0; i < 4 && float32(i+1) <= count; i++ {
			if rpm, ok := scalar(fmt.Sprintf("F%dAc", i)); ok && rpm >= 0 && rpm < 20_000 {
				reading.Fans = append(reading.Fans, FanInfo{Label: fmt.Sprintf("Fan %d", i+1), RPM: rpm})
			}
		}
	}
	if temp, _, ok := smcCPUTemperature(read); ok {
		reading.CPUTemp = &temp
	}
	return reading
}
