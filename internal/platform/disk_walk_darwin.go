//go:build darwin

package platform

import (
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"time"
	"unsafe"

	"github.com/ebitengine/purego"
	"golang.org/x/sys/unix"
)

type diskWalkAttrList struct {
	Count, Reserved                       uint16
	Common, Volume, Directory, File, Fork uint32
}

var diskWalkLib struct {
	sync.Once
	err   error
	bulk  func(int32, *diskWalkAttrList, unsafe.Pointer, uintptr, uint64) int32
	errno func() *int32
}

var diskWalkBuffers = sync.Pool{
	New: func() any { return new([256 * 1024]byte) },
}

func diskWalkLoad() error {
	diskWalkLib.Do(func() {
		h, err := purego.Dlopen("/usr/lib/libSystem.B.dylib", purego.RTLD_NOW|purego.RTLD_LOCAL)
		if err != nil {
			diskWalkLib.err = err
			return
		}
		for _, binding := range []struct {
			name string
			fn   any
		}{{"getattrlistbulk", &diskWalkLib.bulk}, {"__error", &diskWalkLib.errno}} {
			p, e := purego.Dlsym(h, binding.name)
			if e != nil {
				diskWalkLib.err = e
				return
			}
			purego.RegisterFunc(binding.fn, p)
		}
	})
	return diskWalkLib.err
}

// WalkMacVolume returns raw acquisition and explicit incomplete coverage.
// progress runs serially on the calling goroutine and should not block.
// A nil error does not imply full coverage: always inspect Incomplete.
func WalkMacVolume(ctx context.Context, volume VolumeID, progress func(MacWalkProgress)) (MacWalkResult, error) {
	return diskWalkMacVolume(ctx, volume, progress, diskWalkWorkers())
}

// WalkMacVolumeFewThreads limits local traversal to two workers for callers
// already scanning changed subtrees in parallel. The process-wide cap remains.
func WalkMacVolumeFewThreads(ctx context.Context, volume VolumeID, progress func(MacWalkProgress)) (MacWalkResult, error) {
	return diskWalkMacVolume(ctx, volume, progress, 2)
}

func diskWalkMacVolume(ctx context.Context, volume VolumeID, progress func(MacWalkProgress), workers int) (MacWalkResult, error) {
	if err := ctx.Err(); err != nil {
		return MacWalkResult{}, err
	}
	if !filepath.IsAbs(volume.MountPoint()) {
		return MacWalkResult{}, errors.New("macOS walk root must be absolute")
	}
	// Loading symbols does not enumerate or mutate any filesystem content.
	_ = diskWalkLoad() // Missing bulk support is handled by native fd fallback.
	r, err := diskWalkAcquire(ctx, volume, progress, diskWalkNative, &diskWalkProcess, workers, 15*time.Second, 30*time.Second)
	if err == nil {
		for _, c := range r.Incomplete {
			if c.Index == 0 && c.Reason == MacWalkUnreadable {
				return r, c.Err
			}
		}
	}
	return r, err
}

func diskWalkNative(ctx context.Context, path string, budget time.Duration) diskWalkBatch {
	started := time.Now()
	fd, err := unix.Open(path, unix.O_RDONLY|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
	if err != nil {
		return diskWalkBatch{reason: MacWalkUnreadable, err: err}
	}
	var f *os.File
	defer func() {
		if f != nil {
			_ = f.Close()
		} else {
			_ = unix.Close(fd)
		}
	}()
	var st unix.Stat_t
	if err = unix.Fstat(fd, &st); err != nil {
		return diskWalkBatch{reason: MacWalkUnreadable, err: err}
	}
	if st.Flags&diskWalkDataless != 0 {
		return diskWalkBatch{reason: MacWalkDataless}
	}
	if diskWalkLib.err != nil {
		f = os.NewFile(uintptr(fd), path)
		return diskWalkFallback(ctx, f, started, budget)
	}
	al := diskWalkAttrList{Count: 5, Common: diskWalkReturned | diskWalkName | diskWalkType | diskWalkModtime | diskWalkFlags | diskWalkError, File: diskWalkAlloc}
	slab := diskWalkBuffers.Get().(*[256 * 1024]byte)
	// A timed-out acquisition may still be in the syscall: only its worker
	// can return these bytes after the native call has actually finished.
	defer diskWalkBuffers.Put(slab)
	buf := slab[:]
	batch := diskWalkBatch{}
	for {
		if err = ctx.Err(); err != nil {
			batch.reason = MacWalkCancelled
			batch.err = err
			return batch
		}
		if time.Since(started) >= budget {
			batch.reason = MacWalkBudget
			return batch
		}
		// errno is thread-local: the read must stay on the call's OS thread.
		runtime.LockOSThread()
		n := diskWalkLib.bulk(int32(fd), &al, unsafe.Pointer(&buf[0]), uintptr(len(buf)), 0)
		var callErr error
		if n < 0 {
			callErr = unix.Errno(*diskWalkLib.errno())
		}
		runtime.UnlockOSThread()
		if n < 0 {
			if len(batch.items) == 0 && (errors.Is(callErr, unix.ENOTSUP) || errors.Is(callErr, unix.ENOSYS) || errors.Is(callErr, unix.EINVAL)) {
				f = os.NewFile(uintptr(fd), path)
				if _, err = f.Seek(0, io.SeekStart); err != nil {
					batch.reason = MacWalkUnreadable
					batch.err = err
					return batch
				}
				return diskWalkFallback(ctx, f, started, budget)
			}
			batch.reason = MacWalkUnreadable
			batch.err = callErr
			return batch
		}
		if n == 0 {
			return batch
		}
		items, e := diskWalkParseBatch(buf, int(n))
		if len(batch.items) == 0 {
			batch.items = items
		} else {
			batch.items = append(batch.items, items...)
		}
		if e != nil {
			batch.reason = MacWalkMalformed
			batch.err = e
			return batch
		}
	}
}

// Unsupported filesystems use descriptor-relative, no-follow metadata. This is
// still native acquisition; no find/du subprocess and no symlink traversal.
func diskWalkFallback(ctx context.Context, f *os.File, started time.Time, budget time.Duration) diskWalkBatch {
	batch := diskWalkBatch{fallback: true}
	for {
		if err := ctx.Err(); err != nil {
			batch.reason = MacWalkCancelled
			batch.err = err
			return batch
		}
		if time.Since(started) >= budget {
			batch.reason = MacWalkBudget
			return batch
		}
		names, err := f.Readdirnames(256)
		for _, name := range names {
			if !diskWalkValidName(name) {
				continue
			}
			var st unix.Stat_t
			if e := unix.Fstatat(int(f.Fd()), name, &st, unix.AT_SYMLINK_NOFOLLOW); e != nil {
				batch.reason = MacWalkUnreadable
				batch.err = e
				continue
			}
			e := diskWalkItem{flagsKnown: true}
			e.Name = name
			e.Used = true
			e.Flags = st.Flags
			e.Mtime = uint64(max(int64(0), st.Mtim.Sec))
			e.IsDir = st.Mode&unix.S_IFMT == unix.S_IFDIR
			e.IsRegular = st.Mode&unix.S_IFMT == unix.S_IFREG
			if e.IsRegular {
				e.Size = uint64(max(int64(0), st.Blocks)) * 512
				e.SizeKnown = true
			}
			batch.items = append(batch.items, e)
		}
		if err == io.EOF {
			return batch
		}
		if err != nil {
			batch.reason = MacWalkUnreadable
			batch.err = err
			return batch
		}
	}
}
