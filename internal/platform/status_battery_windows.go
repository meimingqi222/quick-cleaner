package platform

import (
	"syscall"
	"unsafe"
)

var batteryGetSystemPowerStatus = syscall.NewLazyDLL("kernel32.dll").NewProc("GetSystemPowerStatus")

// ReadBattery returns nil when no usable battery reading is available.
func ReadBattery() *BatteryReading {
	var power batteryPowerStatus
	ok, _, _ := batteryGetSystemPowerStatus.Call(uintptr(unsafe.Pointer(&power)))
	if ok == 0 {
		return nil
	}
	return batteryFromPowerStatus(power)
}
