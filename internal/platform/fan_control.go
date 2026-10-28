package platform

import (
	"encoding/binary"
	"fmt"
	"math"
	"time"
)

type FanModeKind uint8

const (
	FanAuto FanModeKind = iota
	FanPercent
)

// The zero value selects the system's automatic policy.
type FanMode struct {
	Kind    FanModeKind
	Percent uint8
}

func (m FanMode) IsSupported() bool {
	return m.Kind == FanAuto || m.Kind == FanPercent && (m.Percent == 60 || m.Percent == 100)
}

type FanErrorKind uint8

const (
	FanNeedsRoot FanErrorKind = iota
	FanCanceled
	FanNotInstalled
	FanNeedsUpgrade
	FanOther
)

// Kind, not the error text, controls authorization and installation fallback.
type FanError struct {
	Kind   FanErrorKind
	Detail string
}

func (e FanError) Error() string {
	switch e.Kind {
	case FanCanceled:
		return "canceled"
	case FanNotInstalled:
		return "风扇守护进程未安装"
	case FanNeedsUpgrade:
		return "风扇守护进程需要更新"
	default:
		return e.Detail
	}
}

type fanThermalState uint8

const (
	fanThermalNominal fanThermalState = iota
	fanThermalFair
	fanThermalSerious
	fanThermalCritical
)

type fanCoolingState struct {
	heldTemp, lastDuty *float32
}

func (s *fanCoolingState) duty(requested, temp float32, apple bool, thermal fanThermalState) float32 {
	held := temp
	if s.heldTemp != nil && temp < *s.heldTemp && temp > *s.heldTemp-2 {
		held = *s.heldTemp
	}
	s.heldTemp = &held
	start, full := float32(70), float32(95)
	if apple {
		start, full = 85, 100
	}
	ramp := max(float32(0), min(float32(1), (held-start)/(full-start)))
	desired := requested + (1-requested)*ramp
	if thermal == fanThermalFair {
		desired = max(desired, .75)
	}
	if s.lastDuty != nil && desired < *s.lastDuty {
		desired = max(desired, *s.lastDuty-.05)
	}
	s.lastDuty = &desired
	return desired
}

func smcEncodeScalar(kind uint32, value float32) (smcValue, bool) {
	v := smcValue{Type: kind}
	// Rust's float-to-integer conversions saturate, including NaN -> 0.
	rounded := func(scale, low, high float32) int64 {
		x := float32(math.Round(float64(value * scale)))
		if math.IsNaN(float64(x)) {
			return 0
		}
		return int64(max(low, min(high, x)))
	}
	switch kind {
	case smcKeyCode("fpe2"):
		v.Size = 2
		binary.BigEndian.PutUint16(v.Bytes[:2], uint16(rounded(4, 0, 65535)))
	case smcKeyCode("sp78"):
		v.Size = 2
		binary.BigEndian.PutUint16(v.Bytes[:2], uint16(int16(rounded(256, -32768, 32767))))
	case smcKeyCode("flt "):
		v.Size = 4
		binary.LittleEndian.PutUint32(v.Bytes[:4], math.Float32bits(value))
	case smcKeyCode("ui8 "):
		v.Size = 1
		v.Bytes[0] = uint8(rounded(1, 0, 255))
	default:
		return smcValue{}, false
	}
	return v, true
}

func smcCPUTemperature(read func(string) (smcValue, bool)) (temp float32, apple, ok bool) {
	valid := func(key string) (float32, bool) {
		v, exists := read(key)
		t, decodable := v.scalar()
		return t, exists && decodable && t >= 0 && t < 150
	}
	if t, exists := valid("TC0P"); exists {
		return t, false, true
	}
	for _, key := range []string{"Tp01", "Tp05", "Tp09", "Tp0D"} {
		if t, exists := valid(key); exists && (!ok || t > temp) {
			temp, ok = t, true
		}
	}
	return temp, true, ok
}

// SMC transport and thermal pressure are supplied by the native adapter. The
// same policy runs against protocol fixtures without touching real fan hardware.
type smcFanControl struct {
	read    func(string) (smcValue, bool)
	write   func(string, smcValue, float32) error
	thermal func() (fanThermalState, bool)
	sleep   func(time.Duration)
	state   *fanCoolingState
}

func (c smcFanControl) modeKey(i int) string {
	for _, key := range []string{fmt.Sprintf("F%dMd", i), fmt.Sprintf("F%dmd", i)} {
		if _, ok := c.read(key); ok {
			return key
		}
	}
	return ""
}

func (c smcFanControl) restore(count int) error {
	var first error
	write := func(key string, required bool) {
		v, ok := c.read(key)
		if !ok {
			if required && first == nil {
				first = FanError{Kind: FanOther, Detail: key + " 不可读"}
			}
			return
		}
		if err := c.write(key, v, 0); err != nil && first == nil {
			first = err
		}
	}
	for i := 0; i < count; i++ {
		if key := c.modeKey(i); key != "" {
			write(key, true)
		}
		write(fmt.Sprintf("F%dTg", i), false)
	}
	write("Ftst", false)
	return first
}

func (c smcFanControl) unlock() error {
	v, ok := c.read("Ftst")
	if !ok {
		return nil
	}
	if err := c.write("Ftst", v, 1); err != nil {
		return err
	}
	for range 32 {
		c.sleep(250 * time.Millisecond)
		key := c.modeKey(0)
		if key == "" {
			return nil
		}
		v, ok := c.read(key)
		mode, decoded := v.scalar()
		if !ok || !decoded || mode != 3 {
			return nil
		}
	}
	return FanError{Kind: FanOther, Detail: "thermalmonitord 未让出风扇控制权（8s 超时）"}
}

func (c smcFanControl) set(mode FanMode) error {
	if !mode.IsSupported() {
		return FanError{Kind: FanOther, Detail: "仅支持自动、降温或全速风扇模式"}
	}
	v, ok := c.read("FNum")
	n, decoded := v.scalar()
	count := 0
	if ok && decoded {
		for count < 4 && float32(count+1) <= n {
			count++
		}
	}
	if count == 0 {
		if mode.Kind == FanAuto {
			return nil
		}
		return FanError{Kind: FanOther, Detail: "未检测到可控风扇"}
	}
	if mode.Kind == FanAuto {
		*c.state = fanCoolingState{}
		return c.restore(count)
	}
	duty := float32(mode.Percent) / 100
	if mode.Percent == 100 {
		*c.state = fanCoolingState{}
	} else {
		temp, apple, readable := smcCPUTemperature(c.read)
		thermal, known := c.thermal()
		reason := ""
		if !readable || !known {
			reason = "关键温度信号不可用"
		} else if thermal >= fanThermalSerious {
			reason = "系统热压力已达到严重等级"
		}
		if reason != "" {
			*c.state = fanCoolingState{}
			if err := c.restore(count); err != nil {
				return FanError{Kind: FanOther, Detail: fmt.Sprintf("%s，且恢复系统自动调速失败：%v", reason, err)}
			}
			return FanError{Kind: FanOther, Detail: reason + "，已恢复系统自动调速"}
		}
		duty = c.state.duty(duty, temp, apple, thermal)
	}
	err := c.apply(count, duty)
	if err != nil {
		*c.state = fanCoolingState{}
		_ = c.restore(count)
	}
	return err
}

func (c smcFanControl) apply(count int, duty float32) error {
	if err := c.unlock(); err != nil {
		return err
	}
	for i := 0; i < count; i++ {
		mxKey, mnKey := fmt.Sprintf("F%dMx", i), fmt.Sprintf("F%dMn", i)
		v, ok := c.read(mxKey)
		if !ok {
			return FanError{Kind: FanOther, Detail: mxKey + " 不可读"}
		}
		upper, ok := v.scalar()
		if !ok {
			return FanError{Kind: FanOther, Detail: mxKey + " 无法解码"}
		}
		var lower float32
		if v, ok := c.read(mnKey); ok {
			lower, _ = v.scalar()
		}
		target := max(lower, min(upper, upper*duty))
		key := c.modeKey(i)
		if key == "" {
			return FanError{Kind: FanOther, Detail: fmt.Sprintf("F%dMd 不可读", i)}
		}
		v, ok = c.read(key)
		if !ok {
			return FanError{Kind: FanOther, Detail: key + " 不可读"}
		}
		current, _ := v.scalar()
		if current != 1 {
			if err := c.write(key, v, 1); err != nil {
				return err
			}
		}
		key = fmt.Sprintf("F%dTg", i)
		v, ok = c.read(key)
		if !ok {
			return FanError{Kind: FanOther, Detail: key + " 不可读"}
		}
		if err := c.write(key, v, target); err != nil {
			return err
		}
	}
	return nil
}
