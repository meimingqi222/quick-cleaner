package core

import (
	"context"
	"errors"
	"fmt"
	"log"
	"math"
	"os"
	"path/filepath"
	"slices"
	"sort"
	"sync"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

var ErrDiskIndexRebuildNeeded = errors.New("disk index needs an interactive rebuild")

type macDiskIndexSource interface {
	Current() (uint64, error)
	Changes(context.Context, string, uint64, string) (*platform.FSEventChanges, error)
	Scan(context.Context, VolumeID, bool) (ScanResult, error)
}

type macDiskIndexWriter interface {
	Load(VolumeID) (*LoadedDiskIndex, error)
	Save(VolumeID, *SizeTree, uint64) (DiskIndexSaveResult, error)
	PersistIncremental(VolumeID, *SizeTree, uint64) (DiskIndexSaveResult, error)
}

// MacDiskIndex shares one immutable snapshot across scan, search and disk views.
// Consumers must clone before mutation, then Remember the replacement. Background
// refresh can replay history but cannot initiate a full-volume rebuild.
type MacDiskIndex struct {
	cache   *DiskIndexCache
	writer  macDiskIndexWriter
	source  macDiskIndexSource
	gate    chan struct{}
	ctx     context.Context
	cancel  context.CancelFunc
	ticker  sync.Once
	cached  *ScanResult
	eventID uint64
	persist indexPersistence
}

func newMacDiskIndex(ctx context.Context, cache *DiskIndexCache, source macDiskIndexSource) *MacDiskIndex {
	ctx, cancel := context.WithCancel(ctx)
	return &MacDiskIndex{cache: cache, writer: cache, source: source, gate: make(chan struct{}, 1), ctx: ctx, cancel: cancel, persist: indexPersistence{changed: make(chan struct{})}}
}

// Close cancels the idle ticker and in-flight work without blocking the UI on a
// native syscall. Load's caller context and the owner's lifetime both apply.
func (m *MacDiskIndex) Close() { m.cancel() }

// Peek returns only an already-loaded snapshot without waiting for a scan.
func (m *MacDiskIndex) Peek(volume VolumeID) *ScanResult {
	if m.ctx.Err() != nil {
		return nil
	}
	select {
	case m.gate <- struct{}{}:
		defer func() { <-m.gate }()
	default:
		return nil
	}
	m.adoptPersisted()
	if m.cached != nil && m.cached.Volume.Mount == volume.Mount {
		return m.cached
	}
	return nil
}

// Cached returns an existing snapshot for estimates without starting a scan or
// replay. Only Load may initiate an interactive rebuild.
func (m *MacDiskIndex) Cached(ctx context.Context, volume VolumeID) (*ScanResult, error) {
	if err := m.ctx.Err(); err != nil {
		return nil, err
	}
	ctx, cancel := context.WithCancel(ctx)
	stop := context.AfterFunc(m.ctx, cancel)
	defer cancel()
	defer stop()
	select {
	case m.gate <- struct{}{}:
		defer func() { <-m.gate }()
	case <-ctx.Done():
		return nil, ctx.Err()
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	m.adoptPersisted()
	if m.cached != nil && m.cached.Volume.Mount == volume.Mount {
		m.startTicker()
		return m.cached, nil
	}
	loaded, err := m.cache.Load(volume)
	if err != nil {
		return nil, err
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	m.cached, m.eventID = &loaded.Scan, loaded.LastEventID
	m.startTicker()
	return m.cached, nil
}

func (m *MacDiskIndex) Load(ctx context.Context, volume VolumeID) (*ScanResult, error) {
	started := time.Now()
	if err := m.ctx.Err(); err != nil {
		return nil, err
	}
	ctx, cancel := context.WithCancel(ctx)
	stop := context.AfterFunc(m.ctx, cancel)
	defer cancel()
	defer stop()
	select {
	case m.gate <- struct{}{}:
		defer func() { <-m.gate }()
	case <-ctx.Done():
		return nil, ctx.Err()
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	log.Printf("[disk index] phase=wait duration=%s", time.Since(started).Round(time.Millisecond))
	var previous *ScanResult
	var event uint64
	m.adoptPersisted()
	if m.cached != nil && m.cached.Volume.Mount == volume.Mount {
		previous, event = m.cached, m.eventID
	} else {
		cacheStarted := time.Now()
		loaded, err := m.cache.Load(volume)
		log.Printf("[disk index] phase=cache-read duration=%s error=%v", time.Since(cacheStarted).Round(time.Millisecond), err)
		if err == nil {
			previous, event = &loaded.Scan, loaded.LastEventID
		}
	}
	var scan *ScanResult
	var err error
	if previous == nil {
		scan, event, err = m.full(ctx, volume)
	} else {
		scan, event, err = m.refresh(ctx, previous, event, false)
	}
	if err != nil {
		return nil, err
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	m.cached, m.eventID = scan, event
	m.startTicker()
	return scan, nil
}

// Keep a cache-first owner eligible for the same background replay as Load.
func (m *MacDiskIndex) startTicker() {
	m.ticker.Do(func() {
		go func() {
			ticker := time.NewTicker(15 * time.Minute)
			defer ticker.Stop()
			for {
				select {
				case <-m.ctx.Done():
					return
				case <-ticker.C:
					if _, err := m.Advance(m.ctx); err != nil && !errors.Is(err, ErrDiskIndexRebuildNeeded) && !errors.Is(err, context.Canceled) {
						log.Printf("[disk index] background replay: %v", err)
					}
				}
			}
		}()
	})
}

// Advance skips a busy owner or a never-loaded index. It never performs initial
// discovery, waits behind a user scan, or turns replay failure into a full scan.
func (m *MacDiskIndex) Advance(ctx context.Context) (bool, error) {
	if err := m.ctx.Err(); err != nil {
		return false, err
	}
	ctx, cancel := context.WithCancel(ctx)
	stop := context.AfterFunc(m.ctx, cancel)
	defer cancel()
	defer stop()
	select {
	case m.gate <- struct{}{}:
		defer func() { <-m.gate }()
	default:
		return false, nil
	}
	if err := ctx.Err(); err != nil {
		return false, err
	}
	m.adoptPersisted()
	if m.cached == nil {
		return false, nil
	}
	scan, event, err := m.refresh(ctx, m.cached, m.eventID, true)
	if err != nil {
		return false, err
	}
	if err := ctx.Err(); err != nil {
		return false, err
	}
	advanced := event != m.eventID
	m.cached, m.eventID = scan, event
	return advanced, nil
}

func (m *MacDiskIndex) Remember(ctx context.Context, scan *ScanResult) error {
	if scan == nil || scan.Tree == nil {
		return ErrDiskIndexCorrupt
	}
	select {
	case m.gate <- struct{}{}:
		defer func() { <-m.gate }()
	case <-ctx.Done():
		return ctx.Err()
	case <-m.ctx.Done():
		return m.ctx.Err()
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := m.ctx.Err(); err != nil {
		return err
	}
	if m.cached == nil || m.cached.Volume.Mount != scan.Volume.Mount {
		m.eventID = 0
	}
	m.cached = scan
	return nil
}

func (m *MacDiskIndex) full(ctx context.Context, volume VolumeID) (*ScanResult, uint64, error) {
	// Capture before walking, so writes during the scan are replayed next time.
	event, err := m.source.Current()
	if err != nil {
		return nil, 0, err
	}
	scanStarted := time.Now()
	log.Printf("[disk index] phase=full-scan started")
	scan, err := m.source.Scan(ctx, volume, false)
	log.Printf("[disk index] phase=full-scan duration=%s spill=%v error=%v", time.Since(scanStarted).Round(time.Millisecond), scan.Spill != nil, err)
	if err != nil {
		return nil, 0, err
	}
	persisted := false
	// A scan past the spill threshold comes back without a tree: the raw records
	// are on disk and this is the only place that both owns the cache and knows
	// the event checkpoint captured before the walk.
	if scan.Spill != nil {
		defer scan.Spill.Remove()
		spillStarted := time.Now()
		built, err := BuildMacWalkSpillIndex(m.cache, volume, scan.SpillRoot, scan.Spill, scan.SpillBlocked, event)
		log.Printf("[disk index] phase=spill-build duration=%s error=%v", time.Since(spillStarted).Round(time.Millisecond), err)
		if err != nil {
			return nil, 0, err
		}
		built.ElapsedMS = scan.ElapsedMS
		scan = *built
		persisted = true
	}
	if scan.Tree == nil || !scan.Tree.Valid(0) {
		return nil, 0, ErrDiskIndexCorrupt
	}
	if err := ctx.Err(); err != nil {
		return nil, 0, err
	}
	if !persisted {
		m.queuePersistence(&scan, event, false)
	} else if _, err := m.cache.PruneOrphans(volume); err != nil {
		log.Printf("[disk index] retired cache pruning: %v", err)
	}
	return &scan, event, nil
}

func (m *MacDiskIndex) refresh(ctx context.Context, original *ScanResult, since uint64, background bool) (*ScanResult, uint64, error) {
	rebuild := func(reason string) (*ScanResult, uint64, error) {
		log.Printf("[disk index] phase=rebuild reason=%s background=%v", reason, background)
		if err := ctx.Err(); err != nil {
			return nil, since, err
		}
		if background {
			return nil, since, ErrDiskIndexRebuildNeeded
		}
		return m.full(ctx, original.Volume)
	}
	current, err := m.source.Current()
	if err != nil || current < since {
		return rebuild(fmt.Sprintf("event-id current=%d since=%d error=%v", current, since, err))
	}
	// IDs are global sequence numbers, not a count of changes under this root.
	// Let the bounded replay report actual history loss and delivered progress.
	replayStarted := time.Now()
	changes, err := m.source.Changes(ctx, original.Volume.Mount, since, m.cache.Root())
	log.Printf("[disk index] phase=event-replay duration=%s error=%v", time.Since(replayStarted).Round(time.Millisecond), err)
	if err != nil || changes == nil {
		return rebuild(fmt.Sprintf("replay error=%v", err))
	}
	log.Printf("[disk index] events raw=%d paths=%d rescan=%d incomplete=%v reason=%s since=%d delivered=%d", changes.RawEventCount, len(changes.Paths), len(changes.MustRescan), changes.Incomplete, changes.FullScanReason, since, changes.LastEventID)
	if changes.RequiresFullScan || changes.LastEventID < since {
		return rebuild("replay " + changes.FullScanReason)
	}
	refreshed := original
	if len(changes.Paths)+len(changes.MustRescan) > 0 {
		refreshStarted := time.Now()
		updated, err := refreshMacDiskIndex(ctx, *original, changes, m.source)
		log.Printf("[disk index] phase=apply-events duration=%s error=%v", time.Since(refreshStarted).Round(time.Millisecond), err)
		if err != nil {
			return rebuild(fmt.Sprintf("apply events: %v", err))
		}
		refreshed = &updated
	}
	if err := ctx.Err(); err != nil {
		return nil, since, err
	}
	if refreshed != original || changes.LastEventID > since {
		m.queuePersistence(refreshed, changes.LastEventID, true)
	}
	return refreshed, changes.LastEventID, nil
}

func refreshMacDiskIndex(ctx context.Context, original ScanResult, changes *platform.FSEventChanges, source macDiskIndexSource) (ScanResult, error) {
	if original.Tree == nil || changes.RequiresFullScan {
		return ScanResult{}, ErrDiskIndexRebuildNeeded
	}
	mount := original.Volume.Mount
	for _, path := range changes.MustRescan {
		if platform.CanonicalFSEventPath(path) == mount {
			return ScanResult{}, ErrDiskIndexRebuildNeeded
		}
	}
	paths := append(slices.Clone(changes.Paths), changes.MustRescan...)
	sort.Strings(paths)
	paths = slices.Compact(paths)
	type fileChange struct {
		path        string
		size, mtime uint64
		removed     bool
	}
	var roots, uncertain []string
	var files []fileChange
	for _, path := range paths {
		if err := ctx.Err(); err != nil {
			return ScanResult{}, err
		}
		path = platform.CanonicalFSEventPath(path)
		if path == mount || !pathWithin(path, mount) {
			continue
		}
		info, err := os.Lstat(path)
		switch {
		case errors.Is(err, os.ErrNotExist):
			files = append(files, fileChange{path: path, removed: true})
		case err != nil:
			uncertain = append(uncertain, path)
		case info.IsDir():
			roots = append(roots, path)
		default:
			if _, ok := original.Tree.FindNodeByPath(filepath.Dir(path)); !ok {
				roots = append(roots, filepath.Dir(path))
			} else {
				mtime := min(uint64(max(int64(0), info.ModTime().Unix())), uint64(math.MaxUint32))
				files = append(files, fileChange{path: path, size: platform.AllocatedFileSize(info), mtime: mtime})
			}
		}
	}
	roots = coalesceMacIndexRoots(roots)
	rescanned := make(macIndexPathSet, len(roots))
	for _, root := range roots {
		rescanned[filepath.Clean(root)] = struct{}{}
	}
	var estimated uint64
	for _, root := range roots {
		if idx, ok := original.Tree.FindNodeByPath(root); ok {
			estimated = saturatingAdd(estimated, original.Tree.FileCountOf(idx))
		}
	}
	if len(roots) > 20_000 || (estimated > 50_000 && float64(estimated)/float64(max(uint64(1), original.Tree.FileCountOf(0))) > 0.20 && !macIndexCoversChildren(mount, roots)) {
		return ScanResult{}, ErrDiskIndexRebuildNeeded
	}
	type subtreeResult struct {
		scan *ScanResult
		err  error
	}
	log.Printf("[disk index] phase=subtree-scans roots=%d files=%d estimated=%d", len(roots), len(files), estimated)
	subtreeStarted := time.Now()
	results := make([]subtreeResult, len(roots))
	var workers sync.WaitGroup
	jobs := make(chan int)
	for range min(8, len(roots)) {
		workers.Go(func() {
			for i := range jobs {
				info, err := os.Lstat(roots[i])
				if errors.Is(err, os.ErrNotExist) {
					continue
				}
				if err != nil {
					results[i].err = err
					continue
				}
				if !info.IsDir() {
					results[i].err = ErrDiskIndexRebuildNeeded
					continue
				}
				scan, err := source.Scan(ctx, VolumeFromMountPoint(roots[i]), true)
				results[i] = subtreeResult{&scan, err}
			}
		})
	}
	for i := range roots {
		jobs <- i
	}
	close(jobs)
	workers.Wait()
	defer func() {
		for _, result := range results {
			if result.scan != nil && result.scan.Spill != nil {
				result.scan.Spill.Remove()
			}
		}
	}()
	log.Printf("[disk index] phase=subtree-scans duration=%s", time.Since(subtreeStarted).Round(time.Millisecond))
	if err := ctx.Err(); err != nil {
		return ScanResult{}, err
	}
	denied := 0
	for i, result := range results {
		if errors.Is(result.err, os.ErrPermission) {
			// The previous subtree is unknown, not absent. Keep it until a
			// successful rescan can replace both its data and coverage.
			uncertain = append(uncertain, roots[i])
			denied++
			if denied <= 5 {
				log.Printf("[disk index] subtree coverage unknown: %q error=%v", roots[i], result.err)
			}
			continue
		}
		if result.err != nil {
			return ScanResult{}, result.err
		}
	}
	if denied > 0 {
		log.Printf("[disk index] unreadable subtrees retained=%d", denied)
	}
	cloneStarted := time.Now()
	updated := original
	updated.Tree = original.Tree.Clone()
	log.Printf("[disk index] phase=clone duration=%s mapped=%v nodes=%d", time.Since(cloneStarted).Round(time.Millisecond), original.Tree.base != nil, original.Tree.nodeCount())
	previousCoverage := updated.Tree.blocked
	updated.Tree.blocked = nil
	cleared := make(macIndexPathSet)
	clearCoverage := func(path string) { cleared[filepath.Clean(path)] = struct{}{} }
	for _, change := range files {
		if rescanned.contains(change.path) {
			continue
		}
		if change.removed {
			if idx, ok := updated.Tree.FindNodeByPath(change.path); ok {
				updated.Tree.RemoveSubtreeInplace(idx)
			}
			clearCoverage(change.path)
		} else {
			updated.Tree.UpsertFileWithMtime(change.path, change.size, change.mtime)
		}
	}
	for i, root := range roots {
		if results[i].err != nil {
			continue
		}
		subtree := results[i].scan
		if subtree != nil && subtree.Tree == nil && subtree.Spill == nil {
			return ScanResult{}, ErrDiskIndexRebuildNeeded
		}
		parent, ok := updated.Tree.FindNodeByPath(filepath.Dir(root))
		if !ok || !updated.Tree.IsDir(parent) {
			continue
		}
		clearCoverage(root)
		if idx, ok := updated.Tree.FindNodeByPath(root); ok {
			updated.Tree.RemoveSubtreeInplace(idx)
		}
		if subtree != nil {
			if subtree.Spill != nil {
				if err := updated.Tree.appendSpillSubtree(ctx, parent, filepath.Base(root), subtree); err != nil {
					return ScanResult{}, err
				}
			} else {
				updated.Tree.AppendSubtree(parent, subtree.Tree, filepath.Base(root))
			}
		}
	}
	// Clear only old gaps. A successfully rescanned subtree may report new
	// gaps under the same root, which must survive publication.
	previousCoverage = slices.DeleteFunc(previousCoverage, func(path string) bool { return cleared.contains(path) })
	updated.Tree.blocked = append(previousCoverage, updated.Tree.blocked...)
	updated.Tree.blocked = append(updated.Tree.blocked, uncertain...)
	rebuildStarted := time.Now()
	updated.Tree.RebuildChildArrays()
	refreshDiskScanTotals(&updated)
	log.Printf("[disk index] phase=rebuild-arrays duration=%s", time.Since(rebuildStarted).Round(time.Millisecond))
	return updated, nil
}

// Keys and queries are canonical absolute macOS paths. Walking parents
// preserves component boundaries without comparing every path/root pair.
type macIndexPathSet map[string]struct{}

func (paths macIndexPathSet) contains(path string) bool {
	if len(paths) == 0 {
		return false
	}
	path = filepath.Clean(path)
	for {
		if _, ok := paths[path]; ok {
			return true
		}
		parent := filepath.Dir(path)
		if parent == path {
			return false
		}
		path = parent
	}
}

func coalesceMacIndexRoots(roots []string) []string {
	// A trailing separator keeps an ancestor next to its descendants even when
	// a sibling is a lexical prefix extension (a, a-x, a/child).
	sort.Slice(roots, func(i, j int) bool { return roots[i]+string(filepath.Separator) < roots[j]+string(filepath.Separator) })
	kept := roots[:0]
	for _, path := range roots {
		if len(kept) == 0 || !pathWithin(path, kept[len(kept)-1]) {
			kept = append(kept, path)
		}
	}
	return kept
}

func macIndexCoversChildren(mount string, roots []string) bool {
	entries, err := os.ReadDir(mount)
	if err != nil {
		return false
	}
	count := 0
	for _, entry := range entries {
		path := filepath.Join(mount, entry.Name())
		if info, err := os.Stat(path); err == nil && info.IsDir() {
			count++
			if !slices.ContainsFunc(roots, func(root string) bool { return pathWithin(path, root) }) {
				return false
			}
		}
	}
	return count > 0
}

func refreshDiskScanTotals(scan *ScanResult) {
	scan.TotalSize = scan.Tree.SizeOf(0)
	scan.FileCount = scan.Tree.FileCountOf(0)
	scan.DirCount = scan.Tree.CountUsedDirs()
	scan.RecordsRead = saturatingAdd(scan.FileCount, scan.DirCount)
	scan.RecordsExpected = scan.RecordsRead
	scan.UniqueSize, scan.UniqueFiles = scan.TotalSize, scan.FileCount
}

// The spill belongs to this call until a successful result transfers it;
// callers never see a failed scan's spill, so they cannot remove it.
func finishMacWalkScan(ctx context.Context, raw platform.MacWalkResult, walkErr error, started time.Time) (ScanResult, error) {
	if walkErr != nil {
		raw.Spill.Remove()
		return ScanResult{}, walkErr
	}
	scan, err := BuildMacWalkScanResult(ctx, raw)
	if err != nil {
		raw.Spill.Remove()
		return ScanResult{}, err
	}
	scan.ElapsedMS = uint64(time.Since(started).Milliseconds())
	return scan, nil
}

// BuildMacWalkScanResult preserves allocated-byte measurements and incomplete
// coverage. Missing children below an unreadable/dataless root remain unknown.
func BuildMacWalkScanResult(ctx context.Context, raw platform.MacWalkResult) (ScanResult, error) {
	if err := ctx.Err(); err != nil {
		return ScanResult{}, err
	}
	// The caller streams the spill into either a full base or a private
	// refresh candidate; do not first materialize another in-memory tree.
	if raw.Spill != nil {
		blocked := make([]string, len(raw.Incomplete))
		for i, gap := range raw.Incomplete {
			blocked[i] = gap.Path
		}
		return ScanResult{Volume: raw.Volume, ElapsedMS: 0, Spill: raw.Spill, SpillRoot: raw.Entries[0].Name, SpillBlocked: blocked}, nil
	}
	if raw.RootIndex != 0 || len(raw.Entries) == 0 || len(raw.Entries) > math.MaxUint32 || !raw.Entries[0].Used || !raw.Entries[0].IsDir {
		return ScanResult{}, platform.ErrMacWalkMalformed
	}
	entries := make([]TreeIndexEntry, len(raw.Entries))
	for i, entry := range raw.Entries {
		if i%4096 == 0 && ctx.Err() != nil {
			return ScanResult{}, ctx.Err()
		}
		if entry.Parent >= uint64(len(raw.Entries)) {
			return ScanResult{}, platform.ErrMacWalkMalformed
		}
		// Match the spill ABI: future mtimes saturate instead of wrapping.
		entries[i] = TreeIndexEntry{Parent: uint32(entry.Parent), Name: entry.Name, IsDir: entry.IsDir, Size: entry.Size, Used: entry.Used, Mtime: min(entry.Mtime, uint64(math.MaxUint32))}
	}
	result := ScanResult{Volume: raw.Volume, Tree: NewSizeTree(raw.Volume, entries)}
	for _, incomplete := range raw.Incomplete {
		result.Tree.blocked = append(result.Tree.blocked, incomplete.Path)
	}
	refreshDiskScanTotals(&result)
	if err := ctx.Err(); err != nil {
		return ScanResult{}, err
	}
	return result, nil
}
