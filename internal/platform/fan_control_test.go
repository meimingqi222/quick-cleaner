package platform

import (
	"errors"
	"math"
	"strings"
	"testing"
	"time"
)

func fanFixture() (smcFanControl, map[string]smcValue) {
	values := map[string]smcValue{
		"FNum": smcTestFloat(2), "Tp01": smcTestFloat(70), "Tp05": smcTestFloat(79),
		"F0Mx": smcTestFloat(5000), "F0Mn": smcTestFloat(3500),
		"F1Mx": smcTestFloat(6000), "F1Mn": smcTestFloat(1200),
		"F0Md": smcTestFloat(0), "F1md": smcTestFloat(0),
		"F0Tg": smcTestFloat(0), "F1Tg": smcTestFloat(0),
	}
	return smcFanControl{
		read: func(key string) (smcValue, bool) { v, ok := values[key]; return v, ok },
		write: func(key string, metadata smcValue, n float32) error {
			v, ok := smcEncodeScalar(metadata.Type, n)
			if !ok || v.Size != metadata.Size {
				return errors.New("invalid scalar")
			}
			values[key] = v
			return nil
		},
		thermal: func() (fanThermalState, bool) { return fanThermalNominal, true },
		sleep:   func(time.Duration) {}, state: &fanCoolingState{},
	}, values
}

func assertFanValues(t *testing.T, values map[string]smcValue, want map[string]float32) {
	t.Helper()
	for key, expected := range want {
		v, ok := values[key]
		n, decoded := v.scalar()
		if !ok || !decoded || math.Abs(float64(n-expected)) > .002 {
			t.Fatalf("%s = %v, want %v", key, n, expected)
		}
	}
}

func TestFanControlProfilesFirmwareFloorAndAutomaticReset(t *testing.T) {
	c, values := fanFixture()
	if err := c.set(FanMode{Kind: FanPercent, Percent: 60}); err != nil {
		t.Fatal(err)
	}
	assertFanValues(t, values, map[string]float32{"F0Md": 1, "F1md": 1, "F0Tg": 3500, "F1Tg": 3600})
	// Hottest P-core, not the first readable sensor, determines the ramp.
	values["Tp05"] = smcTestFloat(95)
	if err := c.set(FanMode{Kind: FanPercent, Percent: 60}); err != nil {
		t.Fatal(err)
	}
	assertFanValues(t, values, map[string]float32{"F0Tg": 4333.3333, "F1Tg": 5200})
	if err := c.set(FanMode{Kind: FanAuto}); err != nil {
		t.Fatal(err)
	}
	assertFanValues(t, values, map[string]float32{"F0Md": 0, "F1md": 0, "F0Tg": 0, "F1Tg": 0})
	// Intel package sensors take precedence and use the 70–95 °C curve.
	values["TC0P"] = smcTestFloat(82.5)
	if err := c.set(FanMode{Kind: FanPercent, Percent: 60}); err != nil {
		t.Fatal(err)
	}
	assertFanValues(t, values, map[string]float32{"F0Tg": 4000, "F1Tg": 4800})
}

func TestFanControlHysteresisAndDownwardSlew(t *testing.T) {
	c, values := fanFixture()
	delete(values, "Tp05")
	for _, step := range []struct{ temp, target float32 }{
		{95, 5200}, {94, 5200}, {93, 4900}, {100, 6000}, {70, 5700},
	} {
		values["Tp01"] = smcTestFloat(step.temp)
		if err := c.set(FanMode{Kind: FanPercent, Percent: 60}); err != nil {
			t.Fatal(err)
		}
		assertFanValues(t, values, map[string]float32{"F1Tg": step.target})
	}
	if err := c.set(FanMode{Kind: FanAuto}); err != nil {
		t.Fatal(err)
	}
	if err := c.set(FanMode{Kind: FanPercent, Percent: 60}); err != nil {
		t.Fatal(err)
	}
	assertFanValues(t, values, map[string]float32{"F1Tg": 3600})
}

func TestFanControlUnsafeSignalsRestoreAutomaticButFullSpeedDoesNotNeedSensors(t *testing.T) {
	for _, failure := range []string{"missing-temperature", "unknown-pressure", "serious", "critical"} {
		t.Run(failure, func(t *testing.T) {
			c, values := fanFixture()
			if err := c.set(FanMode{Kind: FanPercent, Percent: 100}); err != nil {
				t.Fatal(err)
			}
			if failure == "missing-temperature" {
				delete(values, "Tp01")
				delete(values, "Tp05")
			}
			c.thermal = func() (fanThermalState, bool) {
				switch failure {
				case "serious":
					return fanThermalSerious, true
				case "critical":
					return fanThermalCritical, true
				case "unknown-pressure":
					return 0, false
				default:
					return fanThermalNominal, true
				}
			}
			if err := c.set(FanMode{Kind: FanPercent, Percent: 60}); err == nil || !strings.Contains(err.Error(), "已恢复系统自动调速") {
				t.Fatalf("unsafe mode: %v", err)
			}
			assertFanValues(t, values, map[string]float32{"F0Md": 0, "F1md": 0, "F0Tg": 0, "F1Tg": 0})
			if err := c.set(FanMode{Kind: FanPercent, Percent: 100}); err != nil {
				t.Fatal(err)
			}
			assertFanValues(t, values, map[string]float32{"F0Tg": 5000, "F1Tg": 6000})
		})
	}
}

func TestFanControlWriteFailureAndUnlockTimeoutRestoreAllFans(t *testing.T) {
	for _, failure := range []string{"write", "restore", "timeout"} {
		t.Run(failure, func(t *testing.T) {
			c, values := fanFixture()
			var cause error = FanError{Kind: FanNeedsRoot, Detail: "denied"}
			write := c.write
			c.write = func(key string, v smcValue, n float32) error {
				if failure == "write" && key == "F1Tg" && n != 0 || failure == "restore" && key == "F0Md" && n == 0 {
					return cause
				}
				return write(key, v, n)
			}
			if failure == "restore" {
				values["F0Md"], values["F1md"] = smcTestFloat(1), smcTestFloat(1)
				if err := c.set(FanMode{Kind: FanAuto}); !errors.Is(err, cause) {
					t.Fatal(err)
				}
				assertFanValues(t, values, map[string]float32{"F1md": 0, "F1Tg": 0, "F0Tg": 0})
				return
			}
			var waited time.Duration
			if failure == "timeout" {
				values["Ftst"], values["F0Md"] = smcTestFloat(0), smcTestFloat(3)
				c.sleep = func(d time.Duration) { waited += d }
			}
			err := c.set(FanMode{Kind: FanPercent, Percent: 100})
			if failure == "write" && !errors.Is(err, cause) {
				t.Fatal(err)
			}
			if failure == "timeout" && (err == nil || waited != 8*time.Second) {
				t.Fatalf("err=%v waited=%s", err, waited)
			}
			assertFanValues(t, values, map[string]float32{"F0Md": 0, "F1md": 0, "F0Tg": 0, "F1Tg": 0})
		})
	}
}

func TestFanControlNoFansAndUnsupportedMode(t *testing.T) {
	c, values := fanFixture()
	delete(values, "FNum")
	if err := c.set(FanMode{}); err != nil {
		t.Fatal(err)
	}
	if err := c.set(FanMode{Kind: FanPercent, Percent: 100}); err == nil {
		t.Fatal("fanless machine cannot enter manual mode")
	}
	for _, n := range []uint8{0, 1, 59, 61, 99, 101, 255} {
		if err := c.set(FanMode{Kind: FanPercent, Percent: n}); err == nil {
			t.Fatalf("accepted %d", n)
		}
	}
}

func TestFanScalarEncoding(t *testing.T) {
	for _, tt := range []struct {
		kind  string
		value float32
		bytes []byte
	}{
		{"fpe2", 1500.375, []byte{0x17, 0x72}}, {"sp78", -.5, []byte{0xff, 0x80}},
		{"sp78", -1000, []byte{0x80, 0}}, {"ui8 ", 2.5, []byte{3}},
		{"fpe2", float32(math.Inf(1)), []byte{0xff, 0xff}}, {"ui8 ", float32(math.NaN()), []byte{0}},
	} {
		v, ok := smcEncodeScalar(smcKeyCode(tt.kind), tt.value)
		if !ok || v.Size != uint32(len(tt.bytes)) {
			t.Fatal(tt)
		}
		for i, b := range tt.bytes {
			if v.Bytes[i] != b {
				t.Fatalf("%s %v = %x want %x", tt.kind, tt.value, v.Bytes[:v.Size], tt.bytes)
			}
		}
	}
}
