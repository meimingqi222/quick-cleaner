package platform

import (
	"runtime"
	"strings"
	"sync"
	"unsafe"

	"github.com/ebitengine/purego"
	"github.com/ebitengine/purego/objc"
)

var iconAppKit struct {
	sync.Mutex
	once sync.Once
	err  error
}

// AppIconFromBundle reads only bundle files and is safe for parallel callers.
func AppIconFromBundle(path string) []byte { return iconMacBundle(path) }

// AppIconPNG tries bundle files before the serialized NSWorkspace fallback.
func AppIconPNG(path string) []byte {
	if b := AppIconFromBundle(path); b != nil {
		return b
	}
	if path == "" || strings.ContainsRune(path, 0) {
		return nil
	}
	iconAppKit.Lock()
	defer iconAppKit.Unlock()
	iconAppKit.once.Do(func() {
		_, iconAppKit.err = purego.Dlopen("/System/Library/Frameworks/AppKit.framework/AppKit", purego.RTLD_NOW|purego.RTLD_GLOBAL)
	})
	if iconAppKit.err != nil {
		return nil
	}
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	pool := objc.ID(objc.GetClass("NSAutoreleasePool")).Send(objc.RegisterName("new"))
	if pool == 0 {
		return nil
	}
	defer pool.Send(objc.RegisterName("drain"))
	ws := objc.ID(objc.GetClass("NSWorkspace")).Send(objc.RegisterName("sharedWorkspace"))
	if ws == 0 {
		return nil
	}
	s := objc.ID(objc.GetClass("NSString")).Send(objc.RegisterName("alloc"))
	data := []byte(path)
	s = s.Send(objc.RegisterName("initWithBytes:length:encoding:"), unsafe.Pointer(&data[0]), uint64(len(data)), uint64(4))
	if s == 0 {
		return nil
	}
	icon := ws.Send(objc.RegisterName("iconForFile:"), s)
	s.Send(objc.RegisterName("release"))
	runtime.KeepAlive(data)
	if icon == 0 {
		return nil
	}
	cls := objc.GetClass("NSBitmapImageRep")
	reps := icon.Send(objc.RegisterName("representations"))
	var best objc.ID
	bestScore := uint64(^uint64(0))
	for i, n := uint64(0), objc.Send[uint64](reps, objc.RegisterName("count")); i < n; i++ {
		rep := reps.Send(objc.RegisterName("objectAtIndex:"), i)
		if rep == 0 || !objc.Send[bool](rep, objc.RegisterName("isKindOfClass:"), cls) {
			continue
		}
		w := objc.Send[int64](rep, objc.RegisterName("pixelsWide"))
		if w <= 0 {
			continue
		}
		score := w - 64
		if score < 0 {
			score = -score
		}
		if uint64(score) < bestScore {
			bestScore = uint64(score)
			best = rep
		}
	}
	if best != 0 {
		if b := iconNSDataPNG(best); b != nil {
			return iconMacDownscale(b)
		}
	}
	tiff := icon.Send(objc.RegisterName("TIFFRepresentation"))
	if tiff == 0 {
		return nil
	}
	rep := objc.ID(cls).Send(objc.RegisterName("imageRepWithData:"), tiff)
	if rep == 0 {
		return nil
	}
	return iconMacDownscale(iconNSDataPNG(rep))
}

func iconNSDataPNG(rep objc.ID) []byte {
	dict := objc.ID(objc.GetClass("NSDictionary")).Send(objc.RegisterName("dictionary"))
	data := rep.Send(objc.RegisterName("representationUsingType:properties:"), uint64(4), dict)
	if data == 0 {
		return nil
	}
	n := objc.Send[uint64](data, objc.RegisterName("length"))
	p := objc.Send[unsafe.Pointer](data, objc.RegisterName("bytes"))
	if n == 0 || p == nil || n > uint64(^uint(0)>>1) {
		return nil
	}
	return append([]byte(nil), unsafe.Slice((*byte)(p), int(n))...)
}
