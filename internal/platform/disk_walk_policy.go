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

// diskWalkDirRef is a directory entry as the worker prepared it: pos is its
// index inside the batch (the coordinator turns it into the global index) and
// reason marks children that are recorded but never queued.
type diskWalkDirRef struct {
	pos    int
	path   string
	reason MacWalkCoverageReason
}

// diskWalkPrepared is a worker-finished batch: entries carry the final Parent,
// counts are pre-aggregated, and every string operation already ran on the
// worker. The coordinator only assigns indexes and appends.
type diskWalkPrepared struct {
	entries    []MacWalkEntry
	subdirs    []diskWalkDirRef // reason == "" means queue for enumeration
	parentMeta bool             // a file had missing/errored attributes
	dirs       uint64
	files      uint64
	others     uint64
	allocated  uint64
	mirrors    uint64
	reason     MacWalkCoverageReason
	err        error
	fallback   bool
}

// diskWalkJoinPath joins a validated name under an absolute directory. It
// skips filepath.Clean: names cannot contain "/" and dir never ends in one
// except the root itself.
func diskWalkJoinPath(dir, name string) string {
	if dir == "/" {
		return "/" + name
	}
	return dir + "/" + name
}

// diskWalkPrepare mirrors Rust's prepare_batch: filter the data-volume mirror,
// classify each entry, aggregate counters, join subdirectory paths. Everything
// per-entry runs on the worker so the coordinator's hot loop stays a memcpy.
func diskWalkPrepare(dir string, parent uint64, batch diskWalkBatch) diskWalkPrepared {
	p := diskWalkPrepared{reason: batch.reason, err: batch.err, fallback: batch.fallback}
	p.entries = make([]MacWalkEntry, 0, len(batch.items))
	for _, item := range batch.items {
		e := item.MacWalkEntry
		e.Parent = parent
		e.Used = true
		if e.IsDir {
			path := diskWalkJoinPath(dir, e.Name)
			if path == diskWalkMirror {
				p.mirrors++
				continue
			}
			e.Size = 0
			p.dirs++
			pos := len(p.entries)
			switch {
			case e.AttributeError != 0 || !item.flagsKnown:
				p.subdirs = append(p.subdirs, diskWalkDirRef{pos: pos, path: path, reason: MacWalkMetadata})
			case e.Flags&diskWalkDataless != 0:
				p.subdirs = append(p.subdirs, diskWalkDirRef{pos: pos, path: path, reason: MacWalkDataless})
			default:
				p.subdirs = append(p.subdirs, diskWalkDirRef{pos: pos, path: path})
			}
		} else if e.IsRegular {
			p.files++
			p.allocated += e.Size
			if e.AttributeError != 0 || !e.SizeKnown {
				p.parentMeta = true
			}
		} else {
			p.others++
			if e.AttributeError != 0 {
				p.parentMeta = true
			}
		}
		p.entries = append(p.entries, e)
	}
	return p
}

var diskWalkProcess = diskWalkState{slots: make(chan struct{}, 8)}

type diskWalkTask struct {
	path    string
	index   uint64
	started time.Time
	cancel  context.CancelFunc
}
type diskWalkCompletion struct {
	id       uint64
	prepared diskWalkPrepared
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
			go func(path string, id uint64, parent uint64) {
				batch := enum(jobctx, path, soft)
				// Release the enumeration slot before preparing so a hung or
				// slow prepare cannot hold back kernel enumerations elsewhere.
				<-state.slots
				prepared := diskWalkPrepare(path, parent, batch)
				select {
				case done <- diskWalkCompletion{id, prepared}:
				case <-jobctx.Done():
				}
			}(task.path, id, task.index)
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
			batch := completed.prepared
			if batch.fallback {
				r.Counts.FallbackDirectories++
			}
			if batch.reason != "" {
				addCoverage(task, batch.reason, batch.err)
			}
			if err := ctx.Err(); err != nil {
				addCoverage(task, MacWalkCancelled, err)
				continue
			}
			r.Counts.Directories += batch.dirs
			r.Counts.Files += batch.files
			r.Counts.OtherEntries += batch.others
			r.Counts.AllocatedBytes += batch.allocated
			r.Counts.ExcludedMirrors += batch.mirrors
			if batch.parentMeta {
				addCoverage(task, MacWalkMetadata, nil)
			}
			base := r.Counts.Entries
			if err := spill.keepAll(batch.entries, &r); err != nil {
				addCoverage(task, MacWalkUnreadable, err)
				notify()
				return r, err
			}
			r.Counts.Entries += uint64(len(batch.entries))
			for _, sub := range batch.subdirs {
				child := diskWalkTask{path: sub.path, index: base + uint64(sub.pos)}
				if sub.reason != "" {
					addCoverage(child, sub.reason, nil)
				} else {
					queue = append(queue, child)
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

// keepAll appends a prepared batch, spilling the whole scan once it crosses
// the Rust threshold. The root stays in memory because coverage indexes point
// at it.
func (s *macWalkSpill) keepAll(entries []MacWalkEntry, r *MacWalkResult) error {
	if s.file == nil && int(r.Counts.Entries)+len(entries) > macWalkSpillThreshold {
		if err := s.create(); err != nil {
			// Rust keeps the memory path when the spill file cannot be created.
			r.Entries = append(r.Entries, entries...)
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
		for i := range entries {
			if err := s.appendEntry(entries[i]); err != nil {
				s.failed = true
				return err
			}
		}
		return nil
	}
	r.Entries = append(r.Entries, entries...)
	return nil
}

func diskWalkWorkers() int { return max(1, min(8, runtime.NumCPU())) }
