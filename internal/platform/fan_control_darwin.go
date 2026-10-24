package platform

import (
	"fmt"
	"os"
	"runtime"
	"sync"
	"time"
	"unsafe"

	"github.com/ebitengine/purego"
	"github.com/ebitengine/purego/objc"
)

var nativeFanControl struct {
	sync.Mutex
	state fanCoolingState
}

var fanFoundation struct {
	sync.Once
	err error
}

func readFanThermalState() (fanThermalState, bool) {
	fanFoundation.Do(func() {
		_, fanFoundation.err = purego.Dlopen("/System/Library/Frameworks/Foundation.framework/Foundation", purego.RTLD_NOW|purego.RTLD_GLOBAL)
	})
	if fanFoundation.err != nil {
		return 0, false
	}
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	pool := objc.ID(objc.GetClass("NSAutoreleasePool")).Send(objc.RegisterName("new"))
	defer pool.Send(objc.RegisterName("drain"))
	info := objc.ID(objc.GetClass("NSProcessInfo")).Send(objc.RegisterName("processInfo"))
	if info == 0 {
		return 0, false
	}
	raw := objc.Send[int64](info, objc.RegisterName("thermalState"))
	return fanThermalState(raw), raw >= 0 && raw <= 3
}

func (api *smcAPI) write(conn uint32, key string, metadata smcValue, value float32) error {
	v, ok := smcEncodeScalar(metadata.Type, value)
	if !ok {
		return FanError{Kind: FanOther, Detail: fmt.Sprintf("不支持的 SMC 值类型 %#x", metadata.Type)}
	}
	if v.Size != metadata.Size {
		return FanError{Kind: FanOther, Detail: fmt.Sprintf("键 %s 的长度 %d 与编码长度 %d 不一致", key, metadata.Size, v.Size)}
	}
	input := smcKeyData{Key: smcKeyCode(key), Info: smcKeyInfo{Size: v.Size, Type: v.Type}, Command: 6, Bytes: v.Bytes}
	var output smcKeyData
	size := unsafe.Sizeof(output)
	kr := api.call(conn, 2, &input, unsafe.Sizeof(input), &output, &size)
	if kr == 0 && output.Result == 0 && size == unsafe.Sizeof(output) {
		return nil
	}
	kind := FanOther
	if uint32(kr) == 0xe00002c1 || uint32(kr) == 0xe00002c2 || kr == 0 && output.Result != 0 && os.Geteuid() != 0 {
		kind = FanNeedsRoot
	}
	return FanError{Kind: kind, Detail: fmt.Sprintf("SMC write %s = %#x, result=%#x", key, uint32(kr), output.Result)}
}

func FanControlSupported() bool { return true }

func SetFanMode(mode FanMode) error {
	if !mode.IsSupported() {
		return FanError{Kind: FanOther, Detail: "仅支持自动、降温或全速风扇模式"}
	}
	if !loadSMCIOKit() {
		return FanError{Kind: FanOther, Detail: "无法打开 AppleSMC 连接"}
	}
	nativeFanControl.Lock()
	defer nativeFanControl.Unlock()
	matcher := smcIOKit.matching("AppleSMC")
	if matcher == 0 {
		return FanError{Kind: FanOther, Detail: "无法打开 AppleSMC 连接"}
	}
	device := smcIOKit.service(0, matcher)
	if device == 0 {
		return FanError{Kind: FanOther, Detail: "无法打开 AppleSMC 连接"}
	}
	var conn uint32
	kr := smcIOKit.open(device, smcIOKit.taskSelf(), 0, &conn)
	smcIOKit.release(device)
	if kr != 0 || conn == 0 {
		return FanError{Kind: FanOther, Detail: "无法打开 AppleSMC 连接"}
	}
	defer smcIOKit.close(conn)
	control := smcFanControl{
		read:    func(key string) (smcValue, bool) { return smcIOKit.read(conn, key) },
		write:   func(key string, v smcValue, value float32) error { return smcIOKit.write(conn, key, v, value) },
		thermal: readFanThermalState,
		sleep:   time.Sleep,
		state:   &nativeFanControl.state,
	}
	return control.set(mode)
}
