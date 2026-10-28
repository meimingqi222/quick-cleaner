package platform

import (
	"runtime"
	"sync"
)

var thermalLegacyPath, thermalV2Path struct {
	once sync.Once
	path string
}

var thermalZones struct {
	sync.Mutex
	opened bool
	query  *pdhQuery
}

// ReadThermal reads only already-published sensors; it never installs a driver.
func ReadThermal() ThermalReading {
	// Every cached WMI proxy belongs to its OS thread's COM apartment.
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	return thermalReadSources(func() ThermalReading {
		return thermalReadMonitor(func(ns string) [][]thermalValue {
			w := thermalWMINamespace(ns)
			if w == nil {
				return nil
			}
			rows, _ := w.query("SELECT Name, Value, SensorType, Identifier FROM Sensor", []string{"Name", "Value", "SensorType", "Identifier"})
			return rows
		})
	}, func() ThermalReading {
		thermalLegacyPath.once.Do(func() {
			if w := thermalWMINamespace(thermalLenovoNamespace); w != nil {
				thermalLegacyPath.path = w.firstInstancePath(thermalLenovoClass)
			}
		})
		if thermalLegacyPath.path == "" {
			return ThermalReading{}
		}
		w := thermalWMINamespace(thermalLenovoNamespace)
		if w == nil {
			return ThermalReading{}
		}
		return thermalReadLegacy(func(method string) (float64, bool) {
			v, _ := w.callNumber("", thermalLegacyPath.path, method, nil, "Data")
			return v.number, v.isNumber
		})
	}, func() ThermalReading {
		thermalV2Path.once.Do(func() {
			if w := thermalWMINamespace(thermalLenovoNamespace); w != nil {
				thermalV2Path.path = w.firstInstancePath(thermalLenovoOtherClass)
			}
		})
		if thermalV2Path.path == "" {
			return ThermalReading{}
		}
		w := thermalWMINamespace(thermalLenovoNamespace)
		if w == nil {
			return ThermalReading{}
		}
		return thermalReadV2(func(id uint32) (float64, bool) {
			v, _ := w.callNumber(thermalLenovoOtherClass, thermalV2Path.path, "GetFeatureValue", []thermalWMIArg{{name: "IDs", number: id}}, "value")
			return v.number, v.isNumber
		})
	}, thermalReadACPI)
}

func thermalReadACPI() *float32 {
	thermalZones.Lock()
	defer thermalZones.Unlock()
	if !thermalZones.opened {
		thermalZones.opened = true
		thermalZones.query = openPDHQuery([]string{`\Thermal Zone Information(*)\Temperature`})
	}
	if thermalZones.query == nil || !thermalZones.query.collect() {
		return nil
	}
	return thermalZoneTemperature(thermalZones.query.values(0))
}
