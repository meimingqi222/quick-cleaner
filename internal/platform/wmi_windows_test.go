package platform

import (
	"math"
	"runtime"
	"testing"
	"unsafe"

	ole "github.com/go-ole/go-ole"
)

func TestThermalWMIVariantScalarTypes(t *testing.T) {
	for _, tc := range []struct {
		vt   ole.VT
		bits int64
		want float64
	}{
		{ole.VT_I2, 65535, -1}, {ole.VT_I4, 0xffffffff, -1}, {ole.VT_UI1, 255, 255},
		{ole.VT_UI4, 0xffffffff, 4294967295}, {ole.VT_R4, int64(math.Float32bits(73.5)), 73.5}, {ole.VT_R8, int64(math.Float64bits(81.25)), 81.25},
	} {
		v := ole.NewVariant(tc.vt, tc.bits)
		got := thermalReadVariant(&v)
		if !got.isNumber || got.number != tc.want {
			t.Errorf("%v: %+v", tc.vt, got)
		}
	}
	for _, vt := range []ole.VT{ole.VT_EMPTY, ole.VT_NULL, ole.VT_BOOL, ole.VT_I8, ole.VT_UI2} {
		v := ole.NewVariant(vt, 0)
		if got := thermalReadVariant(&v); got.isNumber || got.isText {
			t.Errorf("unsupported %v", vt)
		}
	}
	text := "CPU 温度"
	v := thermalArgumentVariant(thermalWMIArg{text: &text})
	defer v.Clear()
	if got := thermalReadVariant(&v); !got.isText || got.text != text {
		t.Fatalf("BSTR %+v", got)
	}
	if unsafe.Sizeof(v) != 24 {
		t.Fatalf("VARIANT ABI size %d", unsafe.Sizeof(v))
	}
	bstr := ole.SysAllocStringLen("CPU\x00suffix")
	embedded := ole.NewVariant(ole.VT_BSTR, int64(uintptr(unsafe.Pointer(bstr))))
	defer embedded.Clear()
	if got := thermalReadVariant(&embedded); !got.isText || got.text != "CPU" {
		t.Fatalf("Rust's NUL-terminated BSTR interpretation: %+v", got)
	}
}

func TestThermalWMIUint32ArgumentsUseSignedVariant(t *testing.T) {
	for _, n := range []uint32{0, thermalCPUFanID, thermalGPUFanID, thermalCPUTempID, 0x80000002, 0xffffffff} {
		v := thermalArgumentVariant(thermalWMIArg{number: n})
		if v.VT != ole.VT_I4 || uint32(v.Val) != n {
			t.Errorf("%x: %+v", n, v)
		}
	}
}

// This exercises the same native GetObject/GetMethod/SpawnInstance/Put/ExecMethod
// chain as Lenovo without requiring vendor firmware or administrator privileges.
func TestThermalWMINativeMethodArguments(t *testing.T) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	w := thermalConnectWMI(`root\default`)
	if w == nil {
		t.Fatal("cannot connect to native local WMI")
	}
	key := "SOFTWARE"
	v, hr := w.callNumber("StdRegProv", "StdRegProv", "EnumKey", []thermalWMIArg{{name: "hDefKey", number: 0x80000002}, {name: "sSubKeyName", text: &key}}, "ReturnValue")
	if hr < 0 || !v.isNumber || v.number != 0 {
		t.Fatalf("EnumKey: HRESULT=%08x value=%+v", uint32(hr), v)
	}
}

func TestThermalWMINativeQueryAndNamespaceCache(t *testing.T) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	w := thermalWMINamespace(`root\cimv2`)
	if w == nil || thermalWMINamespace(`root\cimv2`) != w {
		t.Fatal("native namespace connection was unavailable or not cached")
	}
	rows, hr := w.query("SELECT Caption FROM Win32_OperatingSystem", []string{"Caption", "QuickCleanerMissingProperty"})
	if hr < 0 || len(rows) != 1 || !rows[0][0].isText || rows[0][0].text == "" || rows[0][1].isText || rows[0][1].isNumber {
		t.Fatalf("native property query: HRESULT=%08x rows=%+v", uint32(hr), rows)
	}
	rows, hr = w.query("SELECT * FROM QuickCleanerMissingThermalClass", []string{"Name"})
	if hr >= 0 || len(rows) != 0 {
		t.Fatalf("invalid class must remain a diagnostic error, not end-of-enumeration: HRESULT=%08x rows=%+v", uint32(hr), rows)
	}
	const missing = `root\QuickCleanerMissingThermalNamespace`
	if thermalWMINamespace(missing) != nil {
		t.Fatal("unexpected test namespace")
	}
	thermalWMICache.Lock()
	known := thermalWMICache.unavailable[missing]
	thermalWMICache.Unlock()
	if !known {
		t.Fatal("missing namespace was not memoized process-wide")
	}
}
