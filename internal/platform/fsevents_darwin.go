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

// fseventSink receives one delivered event at a time from the stream callback.
// The history collector and the directory watcher both consume through it.
type fseventSink interface {
	add(path string, flags uint32, id uint64)
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
	collector := value.(fseventSink)
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

type fseventForwarder struct {
	out chan []string
}

func (f *fseventForwarder) add(path string, _ uint32, _ uint64) {
	if path == "" {
		return
	}
	select {
	case f.out <- []string{path}:
	default:
	}
}

// WatchDirectories starts one FSEvents stream on a dedicated run loop for each
// path. Event batches carry raw delivered paths; the caller debounces and
// re-reads the directories, so a dropped path is only a missed wake-up, not a
// missed change. Close stops the stream; Events is never closed.
func WatchDirectories(paths []string) (*FSDirWatch, error) {
	if err := loadFSEvents(); err != nil {
		return nil, err
	}
	if len(paths) == 0 {
		return nil, errors.New("watch needs at least one path")
	}
	for _, path := range paths {
		if strings.ContainsRune(path, 0) {
			return nil, errors.New("watch path contains NUL")
		}
	}
	out := make(chan []string, 64)
	watch := &FSDirWatch{Events: out, done: make(chan struct{})}
	ready := make(chan error, 1)
	go func() {
		runtime.LockOSThread()
		defer runtime.UnlockOSThread()
		cfPaths := fseventSystem.arrayCreate(0, int64(len(paths)), fseventSystem.arrayCallbacks)
		if cfPaths == 0 {
			ready <- errors.New("FSEvents path array unavailable")
			return
		}
		defer fseventSystem.release(cfPaths)
		var strings_ []uintptr
		for _, path := range paths {
			s := fseventSystem.stringCreate(0, path, 0x08000100)
			if s == 0 {
				for _, s := range strings_ {
					fseventSystem.release(s)
				}
				ready <- errors.New("FSEvents path string unavailable")
				return
			}
			strings_ = append(strings_, s)
			fseventSystem.arrayAppend(cfPaths, s)
		}
		defer func() {
			for _, s := range strings_ {
				fseventSystem.release(s)
			}
		}()
		mode := fseventSystem.stringCreate(0, "kCFRunLoopDefaultMode", 0x08000100)
		if mode == 0 {
			ready <- errors.New("FSEvents run-loop mode unavailable")
			return
		}
		defer fseventSystem.release(mode)
		sink := &fseventForwarder{out: out}
		handle := uintptr(fseventSequence.Add(1))
		fseventHandles.Store(handle, sink)
		defer fseventHandles.Delete(handle)
		streamContext := fseventContext{Info: handle}
		stream := fseventSystem.create(0, fseventSystem.callback, &streamContext, cfPaths, fseventSinceNow, 0.5, 0x10|0x02)
		if stream == 0 {
			ready <- errors.New("FSEvents stream creation failed")
			return
		}
		defer fseventSystem.streamRelease(stream)
		defer fseventSystem.invalidate(stream)
		fseventSystem.schedule(stream, fseventSystem.runLoop(), mode)
		if !fseventSystem.start(stream) {
			ready <- errors.New("FSEvents stream start failed")
			return
		}
		defer fseventSystem.stop(stream)
		ready <- nil
		for {
			select {
			case <-watch.done:
				return
			default:
			}
			// 100ms turns bound the Close latency; the stream keeps its own
			// 500ms latency budget for event batching.
			fseventSystem.run(mode, 0.1, true)
		}
	}()
	if err := <-ready; err != nil {
		close(watch.done)
		return nil, err
	}
	return watch, nil
}
