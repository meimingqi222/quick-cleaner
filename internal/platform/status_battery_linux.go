package platform

// ReadBattery is explicitly unsupported on Linux, matching the migration's native targets.
func ReadBattery() *BatteryReading { return nil }
