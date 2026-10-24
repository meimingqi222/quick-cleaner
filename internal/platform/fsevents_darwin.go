package platform

import (
	"context"
	"errors"
	"runtime"
	"strings"
	"sync"
	"sync/atomic"
	"time"
	"unicode/utf8"
	"unsafe"

	"github.com/ebitengine/purego"
	"golang.org/x/sys/unix"
)

var fseventSystem struct {
	sync.Once
	err            error
	callback       uintptr
	arrayCallbacks uintptr
	stringCreate   func(uintptr, string, uint32) uintptr
	arrayCreate    func(uintptr, int64, uintptr) uintptr
	arrayAppend    func(uintptr, uintptr)
	release        func(uintptr)
	runLoop        func() uintptr
	run            func(uintptr, float64, bool) int32
	create         func(uintptr, uintptr, *fseventContext, uintptr, uint64, float64, uint32) uintptr
	schedule       func(uintptr, uintptr, uintptr)
	start          func(uintptr) bool
	stop           func(uintptr)
	invalidate     func(uintptr)
	streamRelease  func(uintptr)
	flush          func(uintptr)
	latest         func(uintptr) uint64
	current        func() uint64
}

type fseventContext struct {
	Version                                int64
	Info, Retain, Release, CopyDescription uintptr
}

var fseventHandles sync.Map
var fseventSequence atomic.Uint64

func loadFSEvents() error {
	fseventSystem.Do(func() {
		for _, library := range []struct {
			path     string
			bindings []struct {
				name string
				fn   any
			}
		}{
			{"/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation", []struct {
				name string
				fn   any
			}{
				{"CFStringCreateWithCString", &fseventSystem.stringCreate},
				{"CFArrayCreateMutable", &fseventSystem.arrayCreate},
				{"CFArrayAppendValue", &fseventSystem.arrayAppend},
				{"CFRelease", &fseventSystem.release},
				{"CFRunLoopGetCurrent", &fseventSystem.runLoop},
				{"CFRunLoopRunInMode", &fseventSystem.run},
			}},
			{"/System/Library/Frameworks/CoreServices.framework/CoreServices", []struct {
				name string
				fn   any
			}{
				{"FSEventStreamCreate", &fseventSystem.create},
				{"FSEventStreamScheduleWithRunLoop", &fseventSystem.schedule},
				{"FSEventStreamStart", &fseventSystem.start},
				{"FSEventStreamStop", &fseventSystem.stop},
				{"FSEventStreamInvalidate", &fseventSystem.invalidate},
				{"FSEventStreamRelease", &fseventSystem.streamRelease},
				{"FSEventStreamFlushSync", &fseventSystem.flush},
				{"FSEventStreamGetLatestEventId", &fseventSystem.latest},
				{"FSEventsGetCurrentEventId", &fseventSystem.current},
			}},
		} {
			handle, err := purego.Dlopen(library.path, purego.RTLD_NOW|purego.RTLD_LOCAL)
			if err != nil {
				fseventSystem.err = err
				return
			}
			for _, binding := range library.bindings {
				address, err := purego.Dlsym(handle, binding.name)
				if err != nil {
					fseventSystem.err = err
					return
				}
				purego.RegisterFunc(binding.fn, address)
			}
			if strings.Contains(library.path, "CoreFoundation.framework") {
				fseventSystem.arrayCallbacks, fseventSystem.err = purego.Dlsym(handle, "kCFTypeArrayCallBacks")
				if fseventSystem.err != nil {
					return
				}
			}
		}
		fseventSystem.callback = purego.NewCallback(receiveFSEvents)
	})
	return fseventSystem.err
}

func receiveFSEvents(_ uintptr, handle uintptr, count uintptr, paths unsafe.Pointer, flags *uint32, ids *uint64) {
	value, ok := fseventHandles.Load(handle)
	if !ok || paths == nil || flags == nil {
		return
	}
	collector := value.(*fseventCollector)
	pathValues := unsafe.Slice((**byte)(paths), count)
	flagValues := unsafe.Slice(flags, count)
	var idValues []uint64
	if ids != nil {
		idValues = unsafe.Slice(ids, count)
	}
	for i, ptr := range pathValues {
		var id uint64
		if idValues != nil {
			id = idValues[i]
		}
		var path string
		if ptr != nil {
			path = unix.BytePtrToString(ptr)
			if !utf8.ValidString(path) {
				path = ""
			}
		}
		collector.add(path, flagValues[i], id)
	}
}

func CurrentFSEventID() (uint64, error) {
	if err := loadFSEvents(); err != nil {
		return 0, err
	}
	return fseventSystem.current(), nil
}

// ReadFSEventChanges replays at most five seconds of history. cacheDir must come
// from the same cache-path authority as persistence, including test overrides.
// An error means no usable batch; callers must not advance their stored ID.
func ReadFSEventChanges(ctx context.Context, root string, since uint64, cacheDir string) (*FSEventChanges, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if strings.ContainsRune(root, 0) {
		return nil, errors.New("FSEvents root contains NUL")
	}
	if err := loadFSEvents(); err != nil {
		return nil, err
	}
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	path := fseventSystem.stringCreate(0, root, 0x08000100)
	if path == 0 {
		return nil, errors.New("FSEvents root string unavailable")
	}
	defer fseventSystem.release(path)
	paths := fseventSystem.arrayCreate(0, 1, fseventSystem.arrayCallbacks)
	if paths == 0 {
		return nil, errors.New("FSEvents path array unavailable")
	}
	defer fseventSystem.release(paths)
	fseventSystem.arrayAppend(paths, path)
	mode := fseventSystem.stringCreate(0, "kCFRunLoopDefaultMode", 0x08000100)
	if mode == 0 {
		return nil, errors.New("FSEvents run-loop mode unavailable")
	}
	defer fseventSystem.release(mode)
	collector := &fseventCollector{root: CanonicalFSEventPath(root)}
	handle := uintptr(fseventSequence.Add(1))
	fseventHandles.Store(handle, collector)
	defer fseventHandles.Delete(handle)
	streamContext := fseventContext{Info: handle}
	// FileEvents and NoDefer; without UseCFTypes eventPaths is char**.
	stream := fseventSystem.create(0, fseventSystem.callback, &streamContext, paths, since, 0, 0x10|0x02)
	if stream == 0 {
		return nil, errors.New("FSEvents stream creation failed")
	}
	defer fseventSystem.streamRelease(stream)
	defer fseventSystem.invalidate(stream)
	fseventSystem.schedule(stream, fseventSystem.runLoop(), mode)
	if !fseventSystem.start(stream) {
		return nil, errors.New("FSEvents stream start failed")
	}
	defer fseventSystem.stop(stream)
	deadline := time.Now().Add(5 * time.Second)
	for !collector.historyDone && !collector.rootRescan && time.Now().Before(deadline) {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if fseventSystem.run(mode, 0.1, true) == 1 && since == fseventSinceNow {
			break
		}
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	latest := since
	if !collector.rootRescan && (collector.historyDone || since == fseventSinceNow) {
		// Never flush incomplete history: it can block well beyond the replay
		// budget, and its latest ID would silently skip undelivered events.
		fseventSystem.flush(stream)
		latest = fseventSystem.latest(stream)
	}
	if collector.rootRescan {
		children := fseventDirectChildren(root)
		return collector.changes(since, latest, cacheDir, children, fseventSystem.current())
	}
	return collector.changes(since, latest, cacheDir, nil, 0)
}
