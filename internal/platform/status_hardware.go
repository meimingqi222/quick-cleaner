package platform

type FanInfo struct {
	Label string
	RPM   float32
}

type GpuReading struct {
	ID                  string
	Name                *string
	Utilization         *float32
	RendererUtilization *float32
	VRAMInUse           *uint64
	TempC               *float32
}

type ThermalReading struct {
	Fans    []FanInfo
	CPUTemp *float32
}
