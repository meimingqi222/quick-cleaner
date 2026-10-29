package core

import (
	"context"
	"log"
	"path/filepath"
	"sort"
	"sync"
	"time"
)

// JunkIndex is the volume-index plan for one junk scan.
// Query is nil when no index is available; ScanFixed then measures live trees.
// Acquired is the dominant volume opened for this scan, if this scan opened one.
// An unelevated plan never calls acquire, so it cannot open $MFT.
type JunkIndex struct {
	Query    FileIndexQuery
	Acquired *SizeTree
	home     string
	volumes  []VolumeID
	trees    []*SizeTree
	acquire  func(context.Context, VolumeID) (*SizeTree, error)
}

// BeginJunkIndex uses indexes the caller already holds. When elevated is false,
// acquire is ignored. When elevated is true, acquire runs only for a volume
// that has no index yet: the dominant target volume now, and the rest during
// Discover. A nil or failed acquire leaves that volume on the bounded walk.
func BeginJunkIndex(ctx context.Context, elevated bool, home string, volumes []VolumeID, indexes []*SizeTree, targets []ScanTarget, acquire func(context.Context, VolumeID) (*SizeTree, error)) JunkIndex {
	plan := JunkIndex{home: home, volumes: append([]VolumeID(nil), volumes...)}
	if elevated {
		plan.acquire = acquire
	}
	seen := map[string]bool{}
	for _, tree := range indexes {
		if tree == nil {
			continue
		}
		key := normMount(tree.Volume().MountPoint())
		if key == "" || seen[key] {
			continue
		}
		seen[key] = true
		plan.trees = append(plan.trees, tree)
	}
	if plan.acquire != nil && ctx.Err() == nil {
		if volume, ok := dominantJunkVolume(targets, volumes); ok && treeForMount(plan.trees, volume.MountPoint()) == nil {
			tree, err := plan.acquire(ctx, volume)
			if err == nil && tree != nil {
				plan.trees = append(plan.trees, tree)
				plan.Acquired = tree
			}
		}
	}
	if len(plan.trees) > 0 {
		plan.Query = indexQuery{trees: append([]*SizeTree(nil), plan.trees...), slots: newMeasurementSlots()}
	}
	return plan
}

// DiscoveryBudget bounds phase two, matching Rust. A walk without an index
// has no natural end on a large tree; what it found by then is real, so the
// categories are only marked partial.
const DiscoveryBudget = 240 * time.Second

// DiscoverWithin runs Discover under budget. expired reports that the budget,
// not the caller, stopped it; a cancelled scan is not shown, so it is false.
func (plan JunkIndex) DiscoverWithin(ctx context.Context, rules *RuleSnapshot, budget time.Duration) ([]ScanItem, bool) {
	bounded, cancel := context.WithTimeout(ctx, budget)
	defer cancel()
	items := plan.Discover(bounded, rules)
	expired := ctx.Err() == nil && bounded.Err() == context.DeadlineExceeded
	if expired {
		log.Printf("阶段二 scan_discovered 预算耗尽（%s），结果按「部分统计」上报", budget)
	}
	return items, expired
}

// Discover walks every held volume index, then acquires any elevated volume
// that still has none. Volumes that stay unindexed use code roots, including
// roots that are not under the user home.
func (plan JunkIndex) Discover(ctx context.Context, rules *RuleSnapshot) []ScanItem {
	if rules == nil || ctx.Err() != nil {
		return nil
	}
	seen := map[string]bool{}
	var out []ScanItem
	add := func(items []ScanItem) {
		for _, item := range items {
			key := Norm(item.Path)
			if key == "" || seen[key] {
				continue
			}
			seen[key] = true
			out = append(out, item)
		}
	}
	held := append([]*SizeTree(nil), plan.trees...)
	covered := map[string]bool{}
	for _, tree := range held {
		if ctx.Err() != nil {
			return out
		}
		covered[normMount(tree.Volume().MountPoint())] = true
		log.Printf("卷 %s: 复用阶段一已解析的 MFT 树，省去一次全盘解析", tree.Volume())
		add(DiscoverBuildArtifacts(ctx, rules, nil, tree))
	}
	var live, pending []VolumeID
	for _, volume := range plan.volumes {
		key := normMount(volume.MountPoint())
		if key == "" || covered[key] {
			continue
		}
		pending = append(pending, volume)
	}
	if plan.acquire != nil && len(pending) > 1 && ctx.Err() == nil {
		// Remaining volumes parse in parallel: each acquire opens its own raw
		// volume handle, and results merge in volume order so dedup stays
		// deterministic. A cancelled ctx aborts every parse together.
		results := make([]struct {
			tree  *SizeTree
			items []ScanItem
		}, len(pending))
		var wg sync.WaitGroup
		for i, volume := range pending {
			wg.Add(1)
			go func(i int, volume VolumeID) {
				defer wg.Done()
				tree, err := plan.acquire(ctx, volume)
				if err != nil || tree == nil || ctx.Err() != nil {
					return
				}
				results[i].tree = tree
				results[i].items = DiscoverBuildArtifacts(ctx, rules, nil, tree)
			}(i, volume)
		}
		wg.Wait()
		for i, volume := range pending {
			if results[i].tree == nil {
				live = append(live, volume)
				continue
			}
			held = append(held, results[i].tree)
			add(results[i].items)
		}
	} else {
		for _, volume := range pending {
			if ctx.Err() != nil {
				return out
			}
			key := normMount(volume.MountPoint())
			if plan.acquire != nil {
				tree, err := plan.acquire(ctx, volume)
				if err == nil && tree != nil {
					covered[key] = true
					held = append(held, tree)
					add(DiscoverBuildArtifacts(ctx, rules, nil, tree))
					continue
				}
			}
			live = append(live, volume)
		}
	}
	home := plan.home
	if junkIndexCovers(held, home) {
		home = ""
	}
	if home != "" || len(live) > 0 {
		add(DiscoverBuildArtifacts(ctx, rules, CodeRoots(home, live), nil))
	}
	return out
}

func dominantJunkVolume(targets []ScanTarget, volumes []VolumeID) (VolumeID, bool) {
	counts := map[string]int{}
	bestMount := map[string]VolumeID{}
	for _, target := range targets {
		volume, ok := volumeContaining(target.Path, volumes)
		if !ok {
			continue
		}
		key := normMount(volume.MountPoint())
		counts[key]++
		bestMount[key] = volume
	}
	var best VolumeID
	bestN := 0
	var keys []string
	for key := range counts {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	for _, key := range keys {
		if counts[key] > bestN {
			bestN = counts[key]
			best = bestMount[key]
		}
	}
	return best, bestN > 0
}

func volumeContaining(path string, volumes []VolumeID) (VolumeID, bool) {
	var best VolumeID
	bestLen := -1
	for _, volume := range volumes {
		mount := normMount(volume.MountPoint())
		if mount == "" || !junkMountContains(mount, path) {
			continue
		}
		if len(mount) > bestLen {
			best = volume
			bestLen = len(mount)
		}
	}
	return best, bestLen >= 0
}

func treeForMount(trees []*SizeTree, mount string) *SizeTree {
	for _, tree := range trees {
		if tree != nil && sameMount(tree.Volume().MountPoint(), mount) {
			return tree
		}
	}
	return nil
}

func junkIndexCovers(trees []*SizeTree, path string) bool {
	if path == "" {
		return false
	}
	for _, tree := range trees {
		if tree != nil && tree.CoversSubtree(path) {
			return true
		}
	}
	return false
}

func normMount(mount string) string {
	mount = filepath.Clean(mount)
	// Norm trims trailing separators, which would erase the Unix root volume.
	if mount == "/" {
		return mount
	}
	mount = Norm(mount)
	if mount == "" || mount == "." {
		return ""
	}
	return mount
}

func junkMountContains(mount, path string) bool {
	if mount == "/" {
		return filepath.IsAbs(path)
	}
	return AtOrUnder(Norm(path), mount)
}

func sameMount(a, b string) bool {
	left, right := normMount(a), normMount(b)
	return left != "" && left == right
}

// indexQuery measures a path from the longest indexed mount that contains it.
// A path outside every index is measured from the live tree.
type indexQuery struct {
	trees []*SizeTree
	slots chan struct{}
}

func (q indexQuery) treeFor(path string) *SizeTree {
	var best *SizeTree
	bestLen := -1
	for _, tree := range q.trees {
		if tree == nil {
			continue
		}
		mount := normMount(tree.Volume().MountPoint())
		if mount == "" || !junkMountContains(mount, path) || len(mount) <= bestLen {
			continue
		}
		best = tree
		bestLen = len(mount)
	}
	return best
}

func (q indexQuery) MeasurePath(ctx context.Context, path string) (PathMeasurement, bool) {
	return (&FSIndexEngine{tree: q.treeFor(path), slots: q.slots}).MeasurePath(ctx, path)
}

func (q indexQuery) QueryFiles(ctx context.Context, f QueryFilter) []IndexedFile {
	if len(f.Roots) == 0 {
		return NewFSIndexEngine(nil).QueryFiles(ctx, f)
	}
	var out []IndexedFile
	for _, root := range f.Roots {
		if ctx.Err() != nil {
			break
		}
		one := f
		one.Roots = []string{root}
		out = append(out, NewFSIndexEngine(q.treeFor(root)).QueryFiles(ctx, one)...)
	}
	return out
}

func (q indexQuery) QueryLargeFiles(ctx context.Context, roots []string, minSize uint64, limit int) []IndexedFile {
	if limit <= 0 {
		return nil
	}
	files := q.QueryFiles(ctx, NewQueryFilter(roots).WithMaxDepth(20).WithSizeRange(minSize, ^uint64(0)))
	sort.SliceStable(files, func(i, j int) bool { return files[i].Size > files[j].Size })
	if len(files) > limit {
		files = files[:limit]
	}
	return files
}

func (q indexQuery) QueryDuplicateBuckets(ctx context.Context, roots []string, minSize uint64) map[uint64][]DuplicateCandidate {
	files := q.QueryFiles(ctx, NewQueryFilter(roots).WithMaxDepth(20).WithSizeRange(minSize, ^uint64(0)))
	buckets := map[uint64][]DuplicateCandidate{}
	for _, file := range files {
		if ctx.Err() != nil {
			break
		}
		buckets[file.Size] = append(buckets[file.Size], DuplicateCandidate{file.Path, file.Mtime})
	}
	for size, items := range buckets {
		if len(items) < 2 {
			delete(buckets, size)
		}
	}
	return buckets
}

var _ FileIndexQuery = indexQuery{}
