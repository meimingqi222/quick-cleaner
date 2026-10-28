package platform

import (
	"math"
	"reflect"
	"testing"
)

func TestThermalRanges(t *testing.T) {
	for _, tc := range []struct {
		v         float64
		rpm, temp bool
	}{
		{-1, false, false}, {0, true, false}, {0.99, true, false}, {1, true, true},
		{149.999, true, true}, {150, true, false}, {19999.99, true, false}, {20000, false, false},
		{math.NaN(), false, false}, {math.Inf(1), false, false}, {math.Inf(-1), false, false},
	} {
		if thermalRPMValid(tc.v) != tc.rpm || thermalTempValid(tc.v) != tc.temp {
			t.Errorf("range(%v)", tc.v)
		}
	}
}

func TestThermalFallbackDoesNotMergeVendors(t *testing.T) {
	temp := float32(72)
	fan := ThermalReading{Fans: []FanInfo{{Label: "chosen", RPM: 0}}}
	for _, tc := range []struct {
		sources [3]ThermalReading
		calls   []int
		want    ThermalReading
	}{
		{[3]ThermalReading{fan, {}, {}}, []int{0, 3}, ThermalReading{Fans: fan.Fans, CPUTemp: &temp}},
		{[3]ThermalReading{{CPUTemp: &temp}, fan, {}}, []int{0}, ThermalReading{CPUTemp: &temp}},
		{[3]ThermalReading{{}, fan, {}}, []int{0, 1, 3}, ThermalReading{Fans: fan.Fans, CPUTemp: &temp}},
		{[3]ThermalReading{{}, {}, {CPUTemp: &temp}}, []int{0, 1, 2}, ThermalReading{CPUTemp: &temp}},
		{[3]ThermalReading{}, []int{0, 1, 2, 3}, ThermalReading{CPUTemp: &temp}},
	} {
		var calls []int
		f := func(i int) func() ThermalReading {
			return func() ThermalReading { calls = append(calls, i); return tc.sources[i] }
		}
		r := thermalReadSources(f(0), f(1), f(2), func() *float32 { calls = append(calls, 3); return &temp })
		if !reflect.DeepEqual(calls, tc.calls) || !reflect.DeepEqual(r, tc.want) {
			t.Errorf("calls %v reading %+v", calls, r)
		}
	}
}

func TestThermalMonitorSensorMatchingAndNamespaceOrder(t *testing.T) {
	row := func(name string, v float64, kind, id string) []thermalValue {
		return []thermalValue{{text: name, isText: true}, {number: v, isNumber: true}, {text: kind, isText: true}, {text: id, isText: true}}
	}
	rows := [][]thermalValue{
		row("renamed", 65, "Temperature", "/intelcpu/0/temperature/0"),
		row("hot", 80, "Temperature", "/amdcpu/0/temperature/1"),
		row("CPU Package", 75, "Temperature", ""),
		row("GPU", 99, "Temperature", "/gpu/0"),
		row("", 0, "Fan", ""), row("Case", 1200.5, "Fan", ""),
		row("sentinel", 20000, "Fan", ""), row("CPU", 150, "Temperature", ""),
	}
	var namespaces []string
	r := thermalReadMonitor(func(ns string) [][]thermalValue { namespaces = append(namespaces, ns); return rows })
	if len(namespaces) != 1 || namespaces[0] != `root\LibreHardwareMonitor` || len(r.Fans) != 2 || r.Fans[0].Label != "Fan 1" || r.Fans[0].RPM != 0 || r.Fans[1].RPM != 1200.5 || r.CPUTemp == nil || *r.CPUTemp != 80 {
		t.Fatalf("%v %+v", namespaces, r)
	}
	namespaces = nil
	r = thermalReadMonitor(func(ns string) [][]thermalValue {
		namespaces = append(namespaces, ns)
		if len(namespaces) == 1 {
			return [][]thermalValue{row("GPU", 90, "Temperature", "/gpu")}
		}
		return rows
	})
	if len(namespaces) != 2 || namespaces[1] != `root\OpenHardwareMonitor` || r.CPUTemp == nil {
		t.Fatalf("fallback %v %+v", namespaces, r)
	}
	for _, tc := range []struct {
		id, name string
		want     bool
	}{
		{"/intelcpuX", "renamed", true}, {"/amdcpu/0", "renamed", true}, {"/IntelCPU", "renamed", false}, {"", "cPu Package", true}, {"/gpu", "GPU", false},
	} {
		if thermalIsCPU(tc.id, tc.name) != tc.want {
			t.Errorf("CPU matching %+v", tc)
		}
	}
}

func TestThermalLenovoZeroAndEarlyFailure(t *testing.T) {
	var calls []string
	r := thermalReadLegacy(func(method string) (float64, bool) { calls = append(calls, method); return 0, true })
	if len(r.Fans) != 2 || r.CPUTemp != nil || len(calls) != 3 {
		t.Fatalf("legacy zero %+v %v", r, calls)
	}
	calls = nil
	r = thermalReadLegacy(func(method string) (float64, bool) { calls = append(calls, method); return 20000, true })
	if thermalHasData(r) || !reflect.DeepEqual(calls, []string{"GetFan1Speed"}) {
		t.Fatalf("legacy sentinel %+v %v", r, calls)
	}
	calls = nil
	r = thermalReadLegacy(func(method string) (float64, bool) {
		calls = append(calls, method)
		switch method {
		case "GetFan1Speed":
			return 1300, true
		case "GetCPUTemp":
			return 71, true
		}
		return 0, false
	})
	if len(r.Fans) != 1 || r.CPUTemp == nil || *r.CPUTemp != 71 || len(calls) != 3 {
		t.Fatalf("legacy second failure %+v %v", r, calls)
	}
	var ids []uint32
	r = thermalReadV2(func(id uint32) (float64, bool) {
		ids = append(ids, id)
		switch id {
		case thermalCPUFanID:
			return 0, true
		case thermalGPUFanID:
			return 1800, true
		default:
			return 68, true
		}
	})
	if !reflect.DeepEqual(ids, []uint32{0x04030001, 0x04030002, 0x05040000}) || len(r.Fans) != 1 || r.Fans[0].Label != "Fan 2" || r.CPUTemp == nil || *r.CPUTemp != 68 {
		t.Fatalf("v2 %+v %v", r, ids)
	}
}

func TestThermalACPIKelvinHottestAndInvalid(t *testing.T) {
	r := thermalZoneTemperature(map[string]float64{"cpu": 343.15, "board": 323.15, "invalid": 0, "sentinel": 423.15, "nan": math.NaN()})
	if r == nil || *r != 70 {
		t.Fatalf("ACPI %v", r)
	}
	if thermalZoneTemperature(map[string]float64{"cold": 273.15, "bad": math.Inf(1)}) != nil {
		t.Fatal("fabricated ACPI temperature")
	}
}
