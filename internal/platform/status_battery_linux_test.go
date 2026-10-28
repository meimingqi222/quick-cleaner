package platform

import "testing"

func TestBatteryLinuxUnsupported(t *testing.T) {
	if ReadBattery() != nil {
		t.Fatal("Linux battery must remain explicitly unsupported")
	}
}
