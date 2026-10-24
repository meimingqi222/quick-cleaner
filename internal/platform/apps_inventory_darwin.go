package platform

import (
	"context"
	"os/exec"
	"runtime"
	"sync"
	"time"
	"unsafe"

	"github.com/ebitengine/purego"
	"github.com/ebitengine/purego/objc"
)

func macInventoryCommand(ctx context.Context, timeout time.Duration, program string, args ...string) (ProcessResult, bool) {
	return RunCommandTreeContext(ctx, exec.Command(program, args...), timeout)
}

var macWorkspace struct {
	sync.Mutex
	once sync.Once
	err  error
}

func macWorkspacePaths(ctx context.Context, id string) ([]string, bool) {
	macWorkspace.Lock()
	defer macWorkspace.Unlock()
	if ctx.Err() != nil {
		return nil, false
	}
	macWorkspace.once.Do(func() {
		_, macWorkspace.err = purego.Dlopen("/System/Library/Frameworks/AppKit.framework/AppKit", purego.RTLD_NOW|purego.RTLD_GLOBAL)
	})
	if macWorkspace.err != nil {
		return nil, false
	}
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	pool := objc.ID(objc.GetClass("NSAutoreleasePool")).Send(objc.RegisterName("new"))
	if pool == 0 {
		return nil, false
	}
	defer pool.Send(objc.RegisterName("drain"))
	workspace := objc.ID(objc.GetClass("NSWorkspace")).Send(objc.RegisterName("sharedWorkspace"))
	if workspace == 0 {
		return nil, false
	}
	key := objc.ID(objc.GetClass("NSString")).Send(objc.RegisterName("alloc"))
	data := []byte(id)
	key = key.Send(objc.RegisterName("initWithBytes:length:encoding:"), unsafe.Pointer(&data[0]), uint64(len(data)), uint64(4))
	if key == 0 {
		return nil, false
	}
	defer key.Send(objc.RegisterName("release"))
	urls := workspace.Send(objc.RegisterName("URLsForApplicationsWithBundleIdentifier:"), key)
	if urls == 0 {
		return nil, false
	}
	count := objc.Send[uint64](urls, objc.RegisterName("count"))
	paths := make([]string, 0, count)
	for i := uint64(0); i < count; i++ {
		if ctx.Err() != nil {
			return nil, false
		}
		url := urls.Send(objc.RegisterName("objectAtIndex:"), i)
		path := url.Send(objc.RegisterName("path"))
		if path == 0 {
			return nil, false
		}
		str := objc.Send[string](path, objc.RegisterName("UTF8String"))
		if str == "" {
			return nil, false
		}
		paths = append(paths, str)
	}
	return paths, true
}
