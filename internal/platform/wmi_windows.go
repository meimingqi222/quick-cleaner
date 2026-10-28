package platform

import (
	"math"
	"runtime"
	"strings"
	"sync"
	"syscall"
	"unsafe"

	ole "github.com/go-ole/go-ole"
	"golang.org/x/sys/windows"
)

var thermalOLE32 = windows.NewLazySystemDLL("ole32.dll")
var thermalCoInit = thermalOLE32.NewProc("CoInitializeEx")
var thermalCoSecurity = thermalOLE32.NewProc("CoInitializeSecurity")
var thermalCoCreate = thermalOLE32.NewProc("CoCreateInstance")
var thermalCoBlanket = thermalOLE32.NewProc("CoSetProxyBlanket")
var thermalKernel32 = windows.NewLazySystemDLL("kernel32.dll")
var thermalTLSAlloc = thermalKernel32.NewProc("TlsAlloc")
var thermalTLSGet = thermalKernel32.NewProc("TlsGetValue")
var thermalTLSSet = thermalKernel32.NewProc("TlsSetValue")

type thermalWMI struct{ services *thermalCOM }
type thermalCOM struct{ vtable *[27]uintptr }

// Slots are the native IWbemLocator/IWbemServices/IWbemClassObject interfaces,
// not Automation's SWbem dispatch wrappers. HRESULT is always signed 32-bit.
func (o *thermalCOM) call(slot int, args ...any) int32 {
	return thermalNativeCall(o.vtable[slot], append([]any{unsafe.Pointer(o)}, args...)...)
}

// Keep pointer arguments typed and reachable until the syscall returns. Passing
// preconverted uintptr values through a variadic helper can lose stack pointers
// when the goroutine's stack grows while constructing the native argument list.
func thermalNativeCall(address uintptr, args ...any) int32 {
	native := make([]uintptr, len(args))
	var pinned runtime.Pinner
	defer pinned.Unpin()
	for i, arg := range args {
		switch v := arg.(type) {
		case unsafe.Pointer:
			if v != nil {
				pinned.Pin(v)
			}
			native[i] = uintptr(v)
		case int:
			native[i] = uintptr(v)
		case uintptr:
			native[i] = v
		default:
			panic("invalid native WMI argument")
		}
	}
	hr, _, _ := syscall.SyscallN(address, native...)
	runtime.KeepAlive(args)
	return int32(hr)
}
func (o *thermalCOM) release() {
	if o != nil {
		o.call(2)
	}
}
func thermalPtr[T any](p *T) unsafe.Pointer { return unsafe.Pointer(p) }

// As in Rust, namespace failures are process-wide and successful proxies are
// apartment-local, retained without Release/CoUninitialize at thread teardown.
var thermalWMICache = struct {
	sync.Mutex
	unavailable map[string]bool
	threads     map[uintptr]map[string]*thermalWMI
	tlsOnce     sync.Once
	tlsIndex    uintptr
	nextThread  uintptr
}{unavailable: make(map[string]bool), threads: make(map[uintptr]map[string]*thermalWMI)}

// The caller must remain locked to its OS thread until it finishes using w.
func thermalWMINamespace(ns string) *thermalWMI {
	thermalWMICache.Lock()
	defer thermalWMICache.Unlock()
	if thermalWMICache.unavailable[ns] {
		return nil
	}
	thermalWMICache.tlsOnce.Do(func() { thermalWMICache.tlsIndex, _, _ = thermalTLSAlloc.Call() })
	if thermalWMICache.tlsIndex == 0xffffffff {
		return nil
	}
	// Windows can recycle numeric thread IDs. A TLS token dies with its thread,
	// so a replacement thread cannot inherit a dead apartment's cached proxy.
	token, _, _ := thermalTLSGet.Call(thermalWMICache.tlsIndex)
	if token == 0 {
		thermalWMICache.nextThread++
		token = thermalWMICache.nextThread
		ok, _, _ := thermalTLSSet.Call(thermalWMICache.tlsIndex, token)
		if ok == 0 {
			return nil
		}
	}
	cache := thermalWMICache.threads[token]
	if w, ok := cache[ns]; ok {
		return w
	}
	if cache == nil {
		cache = make(map[string]*thermalWMI)
		thermalWMICache.threads[token] = cache
	}
	w := thermalConnectWMI(ns)
	cache[ns] = w
	if w == nil {
		thermalWMICache.unavailable[ns] = true
	}
	return w
}

func thermalConnectWMI(ns string) *thermalWMI {
	hr, _, _ := thermalCoInit.Call(0, 0) // COINIT_MULTITHREADED
	if int32(hr) < 0 && uint32(hr) != 0x80010106 {
		return nil
	} // RPC_E_CHANGED_MODE
	hr, _, _ = thermalCoSecurity.Call(0, ^uintptr(0), 0, 0, 0, 3, 0, 0, 0)
	if int32(hr) < 0 && uint32(hr) != 0x80010119 {
		return nil
	} // RPC_E_TOO_LATE
	clsid := ole.NewGUID("{4590F811-1D3A-11D0-891F-00AA004B2E24}")
	iid := ole.NewGUID("{DC12A687-737F-11CF-884D-00AA004B2E24}")
	var locator *thermalCOM
	status := thermalNativeCall(thermalCoCreate.Addr(), thermalPtr(clsid), 0, 1, thermalPtr(iid), thermalPtr(&locator))
	if status < 0 || locator == nil {
		return nil
	}
	defer locator.release()
	path := ole.SysAllocString(ns)
	defer ole.SysFreeString(path)
	var services *thermalCOM
	if locator.call(3, thermalPtr(path), 0, 0, 0, 0, 0, 0, thermalPtr(&services)) < 0 || services == nil {
		return nil
	}
	status = thermalNativeCall(thermalCoBlanket.Addr(), thermalPtr(services), 10, 0, 0, 3, 3, 0, 0)
	if status < 0 {
		services.release()
		return nil
	}
	return &thermalWMI{services: services}
}

func thermalReadVariant(v *ole.VARIANT) thermalValue {
	switch v.VT {
	case ole.VT_I2:
		return thermalValue{number: float64(int16(v.Val)), isNumber: true}
	case ole.VT_I4:
		return thermalValue{number: float64(int32(v.Val)), isNumber: true}
	case ole.VT_UI1:
		return thermalValue{number: float64(uint8(v.Val)), isNumber: true}
	case ole.VT_UI4:
		return thermalValue{number: float64(uint32(v.Val)), isNumber: true}
	case ole.VT_R4:
		return thermalValue{number: float64(math.Float32frombits(uint32(v.Val))), isNumber: true}
	case ole.VT_R8:
		return thermalValue{number: math.Float64frombits(uint64(v.Val)), isNumber: true}
	case ole.VT_BSTR:
		if v.Val != 0 {
			text, _, _ := strings.Cut(v.ToString(), "\x00")
			return thermalValue{text: text, isText: true}
		}
	}
	return thermalValue{}
}

func thermalWMIProperty(object *thermalCOM, name string) thermalValue {
	wide, err := windows.UTF16PtrFromString(name)
	if err != nil {
		return thermalValue{}
	}
	var v ole.VARIANT
	if object.call(4, thermalPtr(wide), 0, thermalPtr(&v), 0, 0) < 0 {
		return thermalValue{}
	}
	defer v.Clear()
	return thermalReadVariant(&v)
}

func (w *thermalWMI) query(wql string, props []string) ([][]thermalValue, int32) {
	language, query := ole.SysAllocString("WQL"), ole.SysAllocString(wql)
	defer ole.SysFreeString(language)
	defer ole.SysFreeString(query)
	var enumerator *thermalCOM
	hr := w.services.call(20, thermalPtr(language), thermalPtr(query), 0x30, 0, thermalPtr(&enumerator))
	if hr < 0 || enumerator == nil {
		return nil, hr
	}
	defer enumerator.release()
	var rows [][]thermalValue
	for {
		var object *thermalCOM
		var returned uint32
		hr = enumerator.call(4, 0xffffffff, 1, thermalPtr(&object), thermalPtr(&returned))
		// A semisynchronous access-denied failure is reported by Next, not ExecQuery.
		if hr < 0 {
			if len(rows) == 0 {
				return nil, hr
			}
			break
		}
		if returned == 0 || object == nil {
			break
		}
		row := make([]thermalValue, len(props))
		for i, p := range props {
			row[i] = thermalWMIProperty(object, p)
		}
		object.release()
		rows = append(rows, row)
	}
	return rows, 0
}

func (w *thermalWMI) firstInstancePath(class string) string {
	rows, _ := w.query("SELECT * FROM "+class, []string{"__PATH"})
	for _, row := range rows {
		if row[0].isText && row[0].text != "" {
			return row[0].text
		}
	}
	return ""
}

type thermalWMIArg struct {
	name   string
	number uint32
	text   *string
}

func thermalArgumentVariant(arg thermalWMIArg) ole.VARIANT {
	if arg.text != nil {
		return ole.NewVariant(ole.VT_BSTR, int64(uintptr(thermalPtr(ole.SysAllocString(*arg.text)))))
	}
	// CIM uint32 is VT_I4, NOT VT_UI4 (P4). Preserve its low 32 bits.
	return ole.NewVariant(ole.VT_I4, int64(int32(arg.number)))
}

func (w *thermalWMI) callNumber(class, path, method string, args []thermalWMIArg, output string) (thermalValue, int32) {
	var instance *thermalCOM
	if len(args) != 0 {
		className := ole.SysAllocString(class)
		defer ole.SysFreeString(className)
		var object *thermalCOM
		hr := w.services.call(6, thermalPtr(className), 0, 0, thermalPtr(&object), 0)
		if hr < 0 || object == nil {
			return thermalValue{}, hr
		}
		defer object.release()
		wide, err := windows.UTF16PtrFromString(method)
		if err != nil {
			return thermalValue{}, -1
		}
		var signature *thermalCOM
		hr = object.call(19, thermalPtr(wide), 0, thermalPtr(&signature), 0)
		if hr < 0 || signature == nil {
			return thermalValue{}, hr
		}
		defer signature.release()
		hr = signature.call(15, 0, thermalPtr(&instance))
		if hr < 0 || instance == nil {
			return thermalValue{}, hr
		}
		defer instance.release()
		for _, arg := range args {
			name, err := windows.UTF16PtrFromString(arg.name)
			if err != nil {
				return thermalValue{}, -1
			}
			v := thermalArgumentVariant(arg)
			// Put on an instance inherits the CIM type from the method signature.
			hr = instance.call(5, thermalPtr(name), 0, thermalPtr(&v), 0)
			v.Clear()
			if hr < 0 {
				return thermalValue{}, hr
			}
		}
	}
	objectPath, methodName := ole.SysAllocString(path), ole.SysAllocString(method)
	defer ole.SysFreeString(objectPath)
	defer ole.SysFreeString(methodName)
	var out *thermalCOM
	hr := w.services.call(24, thermalPtr(objectPath), thermalPtr(methodName), 0, 0, thermalPtr(instance), thermalPtr(&out), 0)
	if hr < 0 || out == nil {
		return thermalValue{}, hr
	}
	defer out.release()
	return thermalWMIProperty(out, output), 0
}
