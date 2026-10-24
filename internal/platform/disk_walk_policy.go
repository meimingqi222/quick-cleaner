package platform

import (
	"context"
	"path/filepath"
	"runtime"
	"sync"
	"time"
)

const diskWalkMirror = "/System/Volumes/Data"

type diskWalkBatch struct {
	items    []diskWalkItem
	reason   MacWalkCoverageReason
	err      error
	fallback bool
}
type diskWalkEnumerate func(context.Context, string, time.Duration) diskWalkBatch
type diskWalkState struct {
	slots chan struct{}
	hung  sync.Map
}

var diskWalkProcess = diskWalkState{slots: make(chan struct{}, 8)}

type diskWalkTask struct {
	path    string
	index   uint64
	started time.Time
	cancel  context.CancelFunc
}
type diskWalkCompletion struct {
	id    uint64
	batch diskWalkBatch
}

// Hung native calls keep their process-wide slot until they actually return.
// Unlike unbounded replacement threads, abandoning a timeout cannot exceed eight
// simultaneous kernel enumerations, including across scans and cancellations.
func diskWalkAcquire(ctx context.Context, volume VolumeID, progress func(MacWalkProgress), enum diskWalkEnumerate, state *diskWalkState, workers int, soft, hard time.Duration) (MacWalkResult, error) {
	r := MacWalkResult{Volume: volume, RootIndex: MacWalkRootIndex}
	if err := ctx.Err(); err != nil {
		return r, err
	}
	root := filepath.Clean(volume.MountPoint())
	r.Entries = []MacWalkEntry{{Name: volume.Display(), IsDir: true, Used: true}}
	r.Counts.Entries = 1
	r.Counts.Directories = 1
	spill := &macWalkSpill{}
	queue := []diskWalkTask{{path: root, index: 0}}
	active := map[uint64]diskWalkTask{}
	var sequence uint64
	workers = max(1, min(8, workers))
	done := make(chan diskWalkCompletion, 8)
	covered := map[uint64]bool{}
	addCoverage := func(task diskWalkTask, reason MacWalkCoverageReason, err error) {
		if covered[task.index] {
			return
		}
		covered[task.index] = true
		r.Incomplete = append(r.Incomplete, MacWalkCoverage{task.index, task.path, reason, err})
		r.Counts.IncompleteDirectories++
		switch reason {
		case MacWalkDataless:
			r.Counts.SkippedDataless++
		case MacWalkHung:
			r.Counts.SkippedHung++
		case MacWalkTimeout:
			r.Counts.TimedOutDirectories++
		}
	}
	notify := func() {
		if progress != nil {
			progress(r.Counts)
		}
	}
	tick := time.NewTicker(min(50*time.Millisecond, max(time.Millisecond, hard/4)))
	defer tick.Stop()
	defer func() {
		for _, task := range active {
			task.cancel()
		}
		// A scan that returns early must not leave its spill behind. A finished
		// spill is detached below, so this only catches cancellation and failure.
		spill.remove()
	}()
	for len(queue) > 0 || len(active) > 0 {
		if err := ctx.Err(); err != nil {
			for _, task := range active {
				addCoverage(task, MacWalkCancelled, err)
			}
			for _, task := range queue {
				addCoverage(task, MacWalkCancelled, err)
			}
			notify()
			return r, err
		}
	dispatch:
		for len(queue) > 0 && len(active) < workers {
			task := queue[0]
			if _, hung := state.hung.Load(task.path); hung {
				queue = queue[1:]
				addCoverage(task, MacWalkHung, nil)
				continue
			}
			select {
			case state.slots <- struct{}{}:
			default:
				if len(active) > 0 {
					break dispatch
				}
				queue = queue[1:]
				addCoverage(task, MacWalkCapacity, nil)
				continue
			}
			queue = queue[1:]
			sequence++
			id := sequence
			task.started = time.Now()
			jobctx, cancel := context.WithCancel(ctx)
			task.cancel = cancel
			active[id] = task
			go func(path string, id uint64) {
				batch := enum(jobctx, path, soft)
				<-state.slots
				select {
				case done <- diskWalkCompletion{id, batch}:
				case <-jobctx.Done():
				}
			}(task.path, id)
		}
		if len(active) == 0 {
			continue
		}
		select {
		case <-ctx.Done():
			continue
		case <-tick.C:
			for id, task := range active {
				if time.Since(task.started) >= hard {
					task.cancel()
					state.hung.Store(task.path, true)
					addCoverage(task, MacWalkTimeout, context.DeadlineExceeded)
					delete(active, id)
				}
			}
			notify()
		case completed := <-done:
			task, ok := active[completed.id]
			if !ok {
				continue
			}
			task.cancel()
			delete(active, completed.id)
			batch := completed.batch
			if batch.fallback {
				r.Counts.FallbackDirectories++
			}
			if batch.reason != "" {
				addCoverage(task, batch.reason, batch.err)
			}
			for _, item := range batch.items {
				if err := ctx.Err(); err != nil {
					addCoverage(task, MacWalkCancelled, err)
					break
				}
				if !diskWalkValidName(item.Name) {
					addCoverage(task, MacWalkMalformed, ErrMacWalkMalformed)
					continue
				}
				path := filepath.Join(task.path, item.Name)
				if item.IsDir && path == diskWalkMirror {
					r.Counts.ExcludedMirrors++
					continue
				}
				e := item.MacWalkEntry
				e.Parent = task.index
				e.Used = true
				if e.IsDir {
					e.Size = 0
					r.Counts.Directories++
				} else if e.IsRegular {
					r.Counts.Files++
					r.Counts.AllocatedBytes += e.Size
				} else {
					r.Counts.OtherEntries++
				}
				idx := r.Counts.Entries
				if err := spill.keep(e, &r); err != nil {
					addCoverage(task, MacWalkUnreadable, err)
					notify()
					return r, err
				}
				r.Counts.Entries++
				child := diskWalkTask{path: path, index: idx}
				if e.IsDir {
					switch {
					case e.AttributeError != 0 || !item.flagsKnown:
						addCoverage(child, MacWalkMetadata, nil)
					case e.Flags&diskWalkDataless != 0:
						addCoverage(child, MacWalkDataless, nil)
					default:
						queue = append(queue, child)
					}
				} else if e.AttributeError != 0 || (e.IsRegular && !e.SizeKnown) {
					addCoverage(task, MacWalkMetadata, nil)
				}
			}
			notify()
		}
	}
	notify()
	if spill.file != nil {
		if err := spill.finish(); err != nil {
			return r, err
		}
		// Detach before the deferred cleanup, which removes an unfinished spill.
		r.Spill = &MacWalkSpill{inner: *spill}
		*spill = macWalkSpill{}
	}
	return r, ctx.Err()
}

// keep appends an entry, spilling the whole scan once it crosses the Rust
// threshold. The root stays in memory because coverage indexes point at it.
func (s *macWalkSpill) keep(entry MacWalkEntry, r *MacWalkResult) error {
	if s.file == nil && int(r.Counts.Entries)+1 > macWalkSpillThreshold {
		if err := s.create(); err != nil {
			// Rust keeps the memory path when the spill file cannot be created.
			r.Entries = append(r.Entries, entry)
			return nil
		}
		for _, kept := range r.Entries[1:] {
			if err := s.appendEntry(kept); err != nil {
				s.failed = true
				return err
			}
		}
		r.Entries = r.Entries[:1]
	}
	if s.file != nil {
		if err := s.appendEntry(entry); err != nil {
			s.failed = true
			return err
		}
		return nil
	}
	r.Entries = append(r.Entries, entry)
	return nil
}

func diskWalkWorkers() int { return max(1, min(8, runtime.NumCPU())) }
