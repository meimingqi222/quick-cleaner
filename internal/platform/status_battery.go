package platform

// BatteryReading leaves unavailable native measurements nil rather than estimating them.
type BatteryReading struct {
	Percent          float32
	Charging         bool
	External         bool
	FullyCharged     bool
	CycleCount       *uint32
	DesignCycleCount *uint32
	HealthPercent    *float32
	TempC            *float32
	MinutesRemaining *uint32
}

// Matches the SYSTEM_POWER_STATUS ABI, including its reserved byte and full lifetime.
type batteryPowerStatus struct {
	ACLineStatus, BatteryFlag, BatteryLifePercent, SystemStatusFlag uint8
	BatteryLifeTime, BatteryFullLifeTime                            uint32
}

func batteryFromPowerStatus(power batteryPowerStatus) *BatteryReading {
	if power.BatteryFlag&128 != 0 || power.BatteryLifePercent > 100 {
		return nil
	}
	reading := &BatteryReading{
		Percent:  float32(power.BatteryLifePercent),
		Charging: power.BatteryFlag&8 != 0,
		External: power.ACLineStatus == 1,
	}
	reading.FullyCharged = reading.External && !reading.Charging && reading.Percent >= 100
	if power.BatteryLifeTime != ^uint32(0) {
		minutes := power.BatteryLifeTime / 60
		reading.MinutesRemaining = &minutes
	}
	return reading
}

type batteryRegistryData struct {
	installed                                    *bool
	current, maximum, rawMaximum, design         *int64
	remaining, cycles, designCycles, temperature *int64
	charging, external, fullyCharged             bool
}

func batteryFromRegistry(data batteryRegistryData) *BatteryReading {
	if data.installed != nil && !*data.installed || data.current == nil || data.maximum == nil || *data.maximum <= 0 {
		return nil
	}
	reading := &BatteryReading{
		Percent:  min(float32(100), max(float32(0), float32(float32(*data.current)/float32(*data.maximum))*100)),
		Charging: data.charging, External: data.external, FullyCharged: data.fullyCharged,
	}
	if data.rawMaximum != nil && *data.rawMaximum > 0 && data.design != nil && *data.design > 0 {
		health := min(float32(100), max(float32(0), float32(float32(*data.rawMaximum)/float32(*data.design))*100))
		reading.HealthPercent = &health
	}
	if data.remaining != nil && *data.remaining > 0 && *data.remaining < 65535 {
		minutes := uint32(*data.remaining)
		reading.MinutesRemaining = &minutes
	}
	if data.cycles != nil {
		cycles := uint32(max(int64(0), *data.cycles))
		reading.CycleCount = &cycles
	}
	if data.designCycles != nil && *data.designCycles > 0 {
		cycles := uint32(*data.designCycles)
		reading.DesignCycleCount = &cycles
	}
	if data.temperature != nil && *data.temperature > 0 {
		temp := float32(*data.temperature) / 100
		reading.TempC = &temp
	}
	return reading
}
