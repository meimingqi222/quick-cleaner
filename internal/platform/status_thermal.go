package platform

import (
	"fmt"
	"strings"
)

const (
	thermalLenovoClass             = "LENOVO_GAMEZONE_DATA"
	thermalLenovoOtherClass        = "LENOVO_OTHER_METHOD"
	thermalLenovoNamespace         = `root\WMI`
	thermalCPUFanID         uint32 = 0x04030001
	thermalGPUFanID         uint32 = 0x04030002
	thermalCPUTempID        uint32 = 0x05040000
)

type thermalValue struct {
	number   float64
	text     string
	isNumber bool
	isText   bool
}

func thermalRPMValid(v float64) bool       { return v >= 0 && v < 20000 }
func thermalTempValid(v float64) bool      { return v >= 1 && v < 150 }
func thermalHasData(r ThermalReading) bool { return len(r.Fans) != 0 || r.CPUTemp != nil }

func thermalReadSources(lhm, legacy, v2 func() ThermalReading, acpi func() *float32) ThermalReading {
	r := lhm()
	if !thermalHasData(r) {
		r = legacy()
	}
	if !thermalHasData(r) {
		r = v2()
	}
	if r.CPUTemp == nil {
		r.CPUTemp = acpi()
	}
	return r
}

func thermalIsCPU(identifier, name string) bool {
	// Rust's fallback folds ASCII only; non-ASCII case mappings are not sensor aliases.
	lower := []byte(name)
	for i, c := range lower {
		if c >= 'A' && c <= 'Z' {
			lower[i] += 'a' - 'A'
		}
	}
	return strings.HasPrefix(identifier, "/intelcpu") || strings.HasPrefix(identifier, "/amdcpu") || strings.Contains(string(lower), "cpu")
}

func thermalReadMonitor(query func(string) [][]thermalValue) ThermalReading {
	for _, ns := range []string{`root\LibreHardwareMonitor`, `root\OpenHardwareMonitor`} {
		var r ThermalReading
		best := 0.0
		for _, row := range query(ns) {
			if len(row) < 4 || !row[1].isNumber {
				continue
			}
			name, kind, id := row[0].text, row[2].text, row[3].text
			v := row[1].number
			switch {
			case kind == "Fan" && thermalRPMValid(v):
				if name == "" {
					name = fmt.Sprintf("Fan %d", len(r.Fans)+1)
				}
				r.Fans = append(r.Fans, FanInfo{Label: name, RPM: float32(v)})
			case kind == "Temperature" && thermalIsCPU(id, name) && thermalTempValid(v):
				// Compare before narrowing to f32, as in the Rust implementation.
				if r.CPUTemp == nil || v > best {
					best = v
					t := float32(v)
					r.CPUTemp = &t
				}
			}
		}
		if thermalHasData(r) {
			return r
		}
	}
	return ThermalReading{}
}

func thermalReadLegacy(number func(string) (float64, bool)) ThermalReading {
	var r ThermalReading
	for i, method := range []string{"GetFan1Speed", "GetFan2Speed"} {
		v, ok := number(method)
		if !ok || !thermalRPMValid(v) {
			if i == 0 {
				return ThermalReading{}
			}
			break
		}
		r.Fans = append(r.Fans, FanInfo{Label: fmt.Sprintf("Fan %d", i+1), RPM: float32(v)})
	}
	if t, ok := number("GetCPUTemp"); ok && thermalTempValid(t) {
		v := float32(t)
		r.CPUTemp = &v
	}
	return r
}

func thermalReadV2(feature func(uint32) (float64, bool)) ThermalReading {
	var r ThermalReading
	for i, id := range []uint32{thermalCPUFanID, thermalGPUFanID} {
		if v, ok := feature(id); ok && v > 0 && thermalRPMValid(v) {
			r.Fans = append(r.Fans, FanInfo{Label: fmt.Sprintf("Fan %d", i+1), RPM: float32(v)})
		}
	}
	if t, ok := feature(thermalCPUTempID); ok && thermalTempValid(t) {
		v := float32(t)
		r.CPUTemp = &v
	}
	return r
}

func thermalZoneTemperature(zones map[string]float64) *float32 {
	var result *float32
	best := 0.0
	for _, kelvin := range zones {
		c := kelvin - 273.15
		if thermalTempValid(c) && (result == nil || c > best) {
			best = c
			v := float32(c)
			result = &v
		}
	}
	return result
}
