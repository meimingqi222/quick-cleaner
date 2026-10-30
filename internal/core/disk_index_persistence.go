package core

import (
	"context"
	"errors"
	"log"
	"math"
	"sort"
	"sync"
	"time"
	"unicode/utf8"
)

type indexSaveRequest struct {
	scan            *ScanResult
	event, sequence uint64
	incremental     bool
}

type indexPersistence struct {
	sync.Mutex
	pending           *indexSaveRequest
	running           bool
	queued, completed uint64
	changed           chan struct{}
	err               error
	original, rebound *ScanResult
	event             uint64
}

func (m *MacDiskIndex) queuePersistence(scan *ScanResult, event uint64, incremental bool) {
	p := &m.persist
	p.Lock()
	defer p.Unlock()
	p.queued++
	// Each snapshot contains the cumulative edits from its base. Keep only
	// the newest pending snapshot while a single writer publishes in order.
	p.pending = &indexSaveRequest{scan, event, p.queued, incremental}
	if !p.running {
		p.running = true
		go m.persistSnapshots()
	}
}

func (m *MacDiskIndex) persistSnapshots() {
	p := &m.persist
	for {
		p.Lock()
		r := p.pending
		p.pending = nil
		if r == nil {
			p.running = false
			p.Unlock()
			return
		}
		p.Unlock()
		started := time.Now()
		save := func(scan *ScanResult) (DiskIndexSaveResult, error) {
			if r.incremental {
				return m.writer.PersistIncremental(scan.Volume, scan.Tree, r.event)
			}
			return m.writer.Save(scan.Volume, scan.Tree, r.event)
		}
		result, err := save(r.scan)
		log.Printf("[disk index] phase=background-persist duration=%s incremental=%v delta=%v compacted=%v event=%d error=%v", time.Since(started).Round(time.Millisecond), r.incremental, result.Delta, result.Compacted, r.event, err)
		// A failed checkpoint leaves the watermark behind, and every later
		// launch replays a longer history until it ends in a volume rebuild.
		if errors.Is(err, ErrDiskIndexCorrupt) {
			if repaired, fixed := r.scan.Tree.quarantineUnrepresentable(); fixed > 0 {
				scan := *r.scan
				scan.Tree = repaired
				started = time.Now()
				result, err = save(&scan)
				log.Printf("[disk index] phase=background-persist-quarantine nodes=%d duration=%s compacted=%v error=%v", fixed, time.Since(started).Round(time.Millisecond), result.Compacted, err)
			}
		}
		var rebound *ScanResult
		if err == nil && (result.Compacted || !r.incremental) {
			// New base IDs must be adopted together with their mapping. Never
			// change an already-published snapshot or rebind a newer scan.
			started = time.Now()
			loaded, loadErr := m.writer.Load(r.scan.Volume)
			log.Printf("[disk index] phase=background-rebind duration=%s error=%v", time.Since(started).Round(time.Millisecond), loadErr)
			if loadErr == nil && loaded.LastEventID == r.event {
				loaded.Scan.ElapsedMS = r.scan.ElapsedMS
				rebound = &loaded.Scan
			}
		}
		if err == nil && !r.incremental {
			if _, pruneErr := m.cache.PruneOrphans(r.scan.Volume); pruneErr != nil {
				log.Printf("[disk index] retired cache pruning: %v", pruneErr)
			}
		}
		p.Lock()
		p.completed, p.err = r.sequence, err
		p.original, p.rebound, p.event = r.scan, rebound, r.event
		close(p.changed)
		p.changed = make(chan struct{})
		p.Unlock()
	}
}

// adoptPersisted is called only with the owner gate held. A completed older
// save cannot replace newer in-memory edits or move the event watermark back.
func (m *MacDiskIndex) adoptPersisted() {
	p := &m.persist
	p.Lock()
	defer p.Unlock()
	if p.rebound != nil && m.cached == p.original && m.eventID == p.event {
		m.cached = p.rebound
		p.original, p.rebound = nil, nil
	}
}

// Flush waits for writes queued before this call, including base validation.
// Close stays nonblocking; callers that need durable completion must Flush.
func (m *MacDiskIndex) Flush(ctx context.Context) error {
	p := &m.persist
	p.Lock()
	target := p.queued
	for p.completed < target {
		changed := p.changed
		p.Unlock()
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-changed:
		}
		p.Lock()
	}
	err := p.err
	p.Unlock()
	select {
	case m.gate <- struct{}{}:
		m.adoptPersisted()
		<-m.gate
	case <-ctx.Done():
		return ctx.Err()
	}
	return err
}

// quarantineUnrepresentable returns a private copy the v7 writer can encode;
// the published snapshot stays untouched for concurrent readers. Times and
// counts saturate as acquisition does. A node whose name cannot be encoded is
// dropped and its parent marked uncovered, so discovery rescans it live
// instead of reading the gap as absence. Base nodes were validated at load.
func (t *SizeTree) quarantineUnrepresentable() (*SizeTree, int) {
	out := t.Clone()
	candidates := make([]uint32, 0, len(out.overrides)+len(out.entries))
	for idx := range out.overrides {
		candidates = append(candidates, idx)
	}
	sort.Slice(candidates, func(i, j int) bool { return candidates[i] < candidates[j] })
	for idx := out.baseCount; idx < out.nodeCount(); idx++ {
		candidates = append(candidates, uint32(idx))
	}
	fixed := 0
	for _, idx := range candidates {
		e := out.slot(idx)
		if e.Mtime > math.MaxUint32 || e.files > math.MaxUint32 {
			e.Mtime, e.files = min(e.Mtime, math.MaxUint32), min(e.files, math.MaxUint32)
			out.setSlot(idx, e)
			fixed++
		}
		if idx == 0 || int(idx) < out.baseCount || (len(e.Name) <= math.MaxUint16 && utf8.ValidString(e.Name) && validDiskIndexName(e.Name)) {
			continue
		}
		if e.Used {
			if parent := out.PathOf(e.Parent); parent != "" {
				out.blocked = append(out.blocked, parent)
			}
			out.RemoveSubtreeInplace(idx)
		}
		// Appended tombstones are still encoded; only their name must be valid.
		e = out.slot(idx)
		e.Name = "_"
		out.setSlot(idx, e)
		fixed++
	}
	return out, fixed
}
