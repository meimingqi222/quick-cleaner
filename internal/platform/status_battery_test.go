package platform

import (
	"testing"
	"unsafe"
)

func batteryInt(v int64) *int64 { return &v }

func TestBatteryPowerStatus(t *testing.T) {
	if unsafe.Sizeof(batteryPowerStatus{}) != 12 || unsafe.Offsetof(batteryPowerStatus{}.BatteryLifeTime) != 4 {
		t.Fatal("SYSTEM_POWER_STATUS ABI mismatch")
	}
	for flag := 0; flag <= 255; flag++ {
		for percent := 0; percent <= 255; percent++ {
			got := batteryFromPowerStatus(batteryPowerStatus{BatteryFlag: uint8(flag), BatteryLifePercent: uint8(percent), BatteryLifeTime: ^uint32(0)})
			if flag&128 != 0 || percent > 100 {
				if got != nil {
					t.Fatalf("flag=%d percent=%d: expected unavailable", flag, percent)
				}
				continue
			}
			if got == nil || got.Percent != float32(percent) || got.Charging != (flag&8 != 0) || got.External || got.FullyCharged || got.MinutesRemaining != nil {
				t.Fatalf("flag=%d percent=%d: %+v", flag, percent, got)
			}
			if got.CycleCount != nil || got.DesignCycleCount != nil || got.HealthPercent != nil || got.TempC != nil {
				t.Fatal("invented Windows measurements")
			}
		}
	}
	for _, ac := range []uint8{0, 1, 2, 255} {
		for _, flag := range []uint8{0, 8} {
			for _, seconds := range []uint32{0, 59, 60, 3599, ^uint32(0) - 1} {
				got := batteryFromPowerStatus(batteryPowerStatus{ACLineStatus: ac, BatteryFlag: flag, BatteryLifePercent: 100, BatteryLifeTime: seconds})
				if got.External != (ac == 1) || got.FullyCharged != (ac == 1 && flag == 0) || got.MinutesRemaining == nil || *got.MinutesRemaining != seconds/60 {
					t.Fatalf("AC=%d flag=%d seconds=%d: %+v", ac, flag, seconds, got)
				}
			}
		}
	}
}

func TestBatteryRegistryRequiredCapacity(t *testing.T) {
	installed := false
	for _, data := range []batteryRegistryData{
		{}, {current: batteryInt(1)}, {maximum: batteryInt(100)},
		{current: batteryInt(1), maximum: batteryInt(0)},
		{current: batteryInt(1), maximum: batteryInt(-1)},
		{installed: &installed, current: batteryInt(1), maximum: batteryInt(100)},
	} {
		if batteryFromRegistry(data) != nil {
			t.Fatalf("expected unavailable: %+v", data)
		}
	}
	for _, tc := range []struct {
		current, maximum int64
		percent          float32
	}{
		{63, 100, 63}, {3150, 5000, 63}, {-1, 100, 0}, {120, 100, 100}, {0, 100, 0},
	} {
		got := batteryFromRegistry(batteryRegistryData{current: batteryInt(tc.current), maximum: batteryInt(tc.maximum)})
		if got == nil || got.Percent != tc.percent || got.Charging || got.External || got.FullyCharged || got.CycleCount != nil || got.DesignCycleCount != nil || got.HealthPercent != nil || got.TempC != nil || got.MinutesRemaining != nil {
			t.Fatalf("capacity %+v: %+v", tc, got)
		}
	}
}

func TestBatteryRegistryOptionalMeasurements(t *testing.T) {
	data := batteryRegistryData{
		current: batteryInt(40), maximum: batteryInt(100),
		rawMaximum: batteryInt(4500), design: batteryInt(5000),
		remaining: batteryInt(120), cycles: batteryInt(25), designCycles: batteryInt(1000), temperature: batteryInt(2985),
		charging: true, external: true, fullyCharged: true,
	}
	got := batteryFromRegistry(data)
	if got == nil || !got.Charging || !got.External || !got.FullyCharged || *got.HealthPercent != 90 || *got.MinutesRemaining != 120 || *got.CycleCount != 25 || *got.DesignCycleCount != 1000 || *got.TempC != float32(29.85) {
		t.Fatalf("full reading: %+v", got)
	}
	for _, n := range []int64{-1, 0, 1, 65534, 65535, 65536, 1<<32 + 9} {
		data.remaining, data.cycles, data.designCycles, data.temperature = batteryInt(n), batteryInt(n), batteryInt(n), batteryInt(n)
		got := batteryFromRegistry(data)
		if (got.MinutesRemaining != nil) != (n > 0 && n < 65535) || got.MinutesRemaining != nil && *got.MinutesRemaining != uint32(n) {
			t.Fatalf("time sentinel %d: %+v", n, got)
		}
		if got.CycleCount == nil || *got.CycleCount != uint32(max(int64(0), n)) {
			t.Fatalf("cycle conversion %d", n)
		}
		if (got.DesignCycleCount != nil) != (n > 0) || got.DesignCycleCount != nil && *got.DesignCycleCount != uint32(n) {
			t.Fatalf("design cycles %d", n)
		}
		if (got.TempC != nil) != (n > 0) || got.TempC != nil && *got.TempC != float32(n)/100 {
			t.Fatalf("temperature %d", n)
		}
	}
	for _, raw := range []int64{-1, 0, 4500, 6000} {
		for _, design := range []int64{-1, 0, 5000} {
			data.rawMaximum, data.design = batteryInt(raw), batteryInt(design)
			got := batteryFromRegistry(data)
			if (got.HealthPercent != nil) != (raw > 0 && design > 0) {
				t.Fatalf("health availability raw=%d design=%d", raw, design)
			}
			if got.HealthPercent != nil && *got.HealthPercent != min(float32(100), float32(float32(raw)/float32(design))*100) {
				t.Fatalf("health ratio raw=%d design=%d", raw, design)
			}
		}
	}
}
