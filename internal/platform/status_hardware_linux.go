package platform

// Linux is an inspection host, not a supported hardware-monitoring product target.
func ReadGPUs() []GpuReading      { return nil }
func ReadThermal() ThermalReading { return ThermalReading{} }
