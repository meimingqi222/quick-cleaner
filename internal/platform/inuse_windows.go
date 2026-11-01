package platform

import (
	"io/fs"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"
	"syscall"
	"time"
)

const (
	// The old 4096 cap turned ordinary browser caches (Edge Code Cache ~7.6k
	// entries) into "occupancy unknown" leftovers even though nothing held them.
	// Measured here: exclusive opens cost ~0.25 ms/file with this batching, so
	// 16k entries fit the 5 s budget. Larger trees (go-build ~78k) still exceed
	// the budget and stay fail-closed; a Restart Manager-only probe measured
	// 10.7 s for 4.2k files, so it is not a cheap substitute.
	inUseDirMaxEntries = 1 << 14
	inUseDirDeadline   = 5 * time.Second
	inUseBatchSize     = 1024
	inUseBatchWorkers  = 8
)

// Sharing violations can come from another probe of the same file through a
// hardlink. A retry must exclude our probes across all concurrent directories.
var inUseProbeMu sync.RWMutex

// Raw sharing evidence never calls database policy (which itself calls IsOpen).
func IsOpen(path string) (bool, bool) {
	md, err := os.Lstat(path)
	if err != nil || md.Mode()&os.ModeSymlink != 0 {
		return false, false
	}
	if !md.IsDir() {
		return inUseFileOpen(path)
	}
	locked, known := scanDirLocks(path, inUseDirMaxEntries, inUseDirDeadline, 1)
	return len(locked) > 0, known
}

// scanDirLocks probes directory files in parallel batches. It stops after
// stopAfter locked files (or the whole tree when stopAfter is zero). A tree
// that was only partially read is known=false, never clear. Finding a lock is
// definitive even when another entry in the same batch was unreadable.
func scanDirLocks(root string, maxEntries int, deadline time.Duration, stopAfter int) ([]string, bool) {
	deadlineAt := time.Now().Add(deadline)
	count := 0
	unknown := false
	var locked []string
	batch := make([]string, 0, inUseBatchSize)
	flush := func() {
		if len(batch) == 0 {
			return
		}
		found, complete := inUseProbeBatch(batch)
		locked = append(locked, found...)
		if !complete {
			unknown = true
		}
		batch = batch[:0]
	}
	_ = filepath.WalkDir(root, func(p string, d fs.DirEntry, err error) error {
		count++
		if err != nil || count > maxEntries || time.Now().After(deadlineAt) || d.Type()&os.ModeSymlink != 0 {
			unknown = true
			return fs.SkipAll
		}
		if d.IsDir() {
			return nil
		}
		if !d.Type().IsRegular() {
			unknown = true
			return fs.SkipAll
		}
		batch = append(batch, p)
		if len(batch) >= inUseBatchSize {
			flush()
		}
		if (unknown && len(locked) == 0) || (stopAfter > 0 && len(locked) >= stopAfter) {
			return fs.SkipAll
		}
		return nil
	})
	flush()
	if len(locked) == 0 && unknown {
		return nil, false
	}
	return locked, true
}

func inUseProbeBatch(paths []string) ([]string, bool) {
	jobs := make(chan string, len(paths))
	for _, p := range paths {
		jobs <- p
	}
	close(jobs)
	var mu sync.Mutex
	var locked []string
	var unknown atomic.Bool
	var wg sync.WaitGroup
	for range min(inUseBatchWorkers, len(paths)) {
		wg.Go(func() {
			for p := range jobs {
				b, k := inUseFileOpen(p)
				if b {
					mu.Lock()
					locked = append(locked, p)
					mu.Unlock()
				}
				if !k {
					unknown.Store(true)
				}
			}
		})
	}
	wg.Wait()
	return locked, !unknown.Load()
}

func inUseFileOpen(path string) (bool, bool) {
	inUseProbeMu.RLock()
	busy, known := inUseFileOpenRaw(path)
	inUseProbeMu.RUnlock()
	if !busy {
		return busy, known
	}
	inUseProbeMu.Lock()
	defer inUseProbeMu.Unlock()
	return inUseFileOpenRaw(path)
}

func inUseFileOpenRaw(path string) (bool, bool) {
	p, err := syscall.UTF16PtrFromString(path)
	if err != nil {
		return false, false
	}
	h, err := syscall.CreateFile(p, syscall.GENERIC_READ, 0, nil, syscall.OPEN_EXISTING, syscall.FILE_ATTRIBUTE_NORMAL, 0)
	if err == nil {
		syscall.CloseHandle(h)
		return false, true
	}
	if err == syscall.Errno(32) || err == syscall.Errno(33) {
		return true, true
	}
	return false, false
}

// Scan-time badges must stay cheap: probing every file of every target with
// exclusive opens delayed results by seconds and only produced badges. The
// cleaner enforces occupancy per leaf at execution time.
func DetectInUse([]string) map[string]InUseBusyInfo { return map[string]InUseBusyInfo{} }
