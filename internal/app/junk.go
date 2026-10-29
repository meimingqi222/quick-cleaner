package app

import (
	"context"
	"fmt"
	"log"
	"runtime"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func (r *Root) scanJunk() {
	if r.busy() {
		return
	}
	home, ok := platform.UserHome()
	if !ok {
		r.message("无法确定原始用户目录。", "Original user home is unavailable.")
		return
	}
	lastBrew := r.settings.BrewCleanupAt
	// Snapshot indexes on the caller thread. The worker must not read them later.
	// Unelevated Windows passes no acquire, so this scan cannot open $MFT.
	indexes := volumeIndexes(r)
	// macOS junk scans share the persistent disk index with the disk lens and
	// file search: it builds once and every menu reuses it. Owner creation is
	// cheap (opens the cache directory); the scan walks live if it fails.
	if runtime.GOOS == "darwin" && r.diskOwner == nil {
		if owner, err := newDiskOwner(r.ctx); err == nil {
			r.diskOwner = owner
		} else {
			log.Printf("磁盘索引不可用，垃圾扫描退回遍历通道：%v", err)
		}
	}
	owner := r.diskOwner
	elevated := runtime.GOOS == "windows" && platform.IsElevated()
	r.junk.Scanning = true
	r.message("正在扫描可清理内容…", "Scanning for cleanable content…")
	r.start(false, nil, func(ctx context.Context) func() {
		rules, err := core.CurrentRules()
		if err != nil {
			return func() { r.junk.Scanning = false; r.failure(err) }
		}
		providers := []func() []core.ScanTarget{func() []core.ScanTarget { return core.DockerScanTargets(rules) }}
		if runtime.GOOS == "darwin" {
			providers = append(providers, func() []core.ScanTarget { return core.MacOSSnapshotTargets(rules) }, func() []core.ScanTarget { return core.BrewScanTargets(rules, lastBrew) })
		}
		finishNative := startJunkProviders(ctx, providers...)
		targets := core.FilesystemScanTargets(rules, core.UserRuleRoots(home), platformName())
		// macOS：先加载/构建整盘共享索引，阶段一在树上查表，阶段二走索引 DFS。
		// 索引同时交给 searchTrees，磁盘透镜与文件搜索复用同一份，不再重扫。
		// Load 有持久化缓存 + FSEvents 增量，已有索引时只是回放增量。
		volumes := platform.ListVolumes()
		var acquiredTree *core.SizeTree
		if runtime.GOOS == "darwin" && owner != nil {
			root := core.VolumeFromMountPoint("/")
			for _, v := range volumes {
				if v.MountPoint() == "/" {
					root = v
					break
				}
			}
			if result, err := owner.Load(ctx, root); err != nil {
				log.Printf("整盘索引加载失败，本次垃圾扫描退回遍历通道：%v", err)
			} else if result != nil && result.Tree != nil {
				acquiredTree = result.Tree
				indexes = prioritizeJunkIndex(indexes, acquiredTree)
			}
		}
		targets = append(targets, finishNative()...)
		busyPaths := make([]string, len(targets))
		for i := range targets {
			busyPaths[i] = targets[i].Path
		}
		busyDone := make(chan map[string]core.Busy, 1)
		go func() { busyDone <- core.DetectInUse(busyPaths) }()
		r.armJunkBusy(busyDone)
		// Every tree acquire parses is recorded so the scan's finish can share
		// it with the disk lens and file search instead of letting it die
		// inside the plan. Acquires run in parallel during discovery.
		var acquiredMu sync.Mutex
		var acquiredTrees []*core.SizeTree
		var acquire func(context.Context, core.VolumeID) (*core.SizeTree, error)
		if elevated {
			acquire = func(ctx context.Context, v core.VolumeID) (*core.SizeTree, error) {
				var result *core.ScanResult
				var err error
				if owner != nil {
					result, err = owner.Load(ctx, v)
				} else {
					result, err = scanNativeVolume(ctx, v)
				}
				if err != nil || result == nil {
					return nil, err
				}
				acquiredMu.Lock()
				acquiredTrees = append(acquiredTrees, result.Tree)
				acquiredMu.Unlock()
				return result.Tree, nil
			}
		}
		planT0 := time.Now()
		plan := core.BeginJunkIndex(ctx, elevated, home, volumes, indexes, targets, acquire)
		if plan.Acquired == nil && acquiredTree != nil {
			plan.Acquired = acquiredTree
		}
		log.Printf("阶段零索引就绪：%s（acquired=%v）", time.Since(planT0).Round(time.Millisecond), plan.Acquired != nil)
		fixedT0 := time.Now()
		cats := core.ScanFixed(ctx, targets, plan.Query)
		log.Printf("阶段一 ScanFixed 完成：%s", time.Since(fixedT0).Round(time.Millisecond))
		var discovered []core.ScanItem
		partial := false
		if ctx.Err() == nil {
			// Ownership of cats transfers to the main thread. Discovery returns a
			// separate slice; no worker may merge into the published categories.
			acquired := plan.Acquired
			r.publish(func() {
				if ctx.Err() != nil {
					return
				}
				if acquired != nil {
					r.rememberJunkIndex(acquired)
				}
				r.junk.Categories = cats
				r.junk.Scanned = true
				r.junk.Scanning, r.junk.Discovering = false, true
				r.selectJunk(view.JunkRecommended)
				r.junkSummary()
				r.applyReadyJunkBusy()
				r.message(fmt.Sprintf("系统垃圾扫描完成，共 %s；正在后台检索项目构建产物…", r.dashboard.Cleanable), fmt.Sprintf("System junk scanned — %s; still looking for build artifacts…", r.dashboard.Cleanable))
			})
			discoverStarted := time.Now()
			if plan.Query == nil {
				log.Printf("没有可用索引，发现式扫描走遍历通道（慢得多）")
			}
			discovered, partial = plan.DiscoverWithin(ctx, rules, core.DiscoveryBudget)
			discovered = core.DropVanishedDiscovered(discovered)
			channel := "索引"
			if plan.Query == nil {
				channel = "遍历"
			}
			var discoveredBytes uint64
			for _, item := range discovered {
				discoveredBytes += item.Size
			}
			discoverElapsed := time.Since(discoverStarted)
			log.Printf("发现式扫描走 %s 通道：%s，%d 条", channel, discoverElapsed, len(discovered))
			log.Printf("阶段二 scan_discovered 完成：%s，%d 条，合计 %s", time.Since(discoverStarted), len(discovered), core.FmtSize(discoveredBytes))
		}
		cancelled := ctx.Err()
		return func() {
			r.junk.Scanning, r.junk.Discovering = false, false
			for _, tree := range acquiredTrees {
				r.rememberJunkIndex(tree)
			}
			if cancelled != nil {
				r.failure(cancelled)
				return
			}
			core.MergeDiscovered(r.junk.Categories, discovered, partial)
			for _, item := range discovered {
				if _, exists := r.junk.Selected[item.Path]; !exists {
					r.junk.Selected[item.Path] = item.Recommended
				}
			}
			r.junkSummary()
			r.applyReadyJunkBusy()
			r.message(fmt.Sprintf("扫描完成，共发现 %s 可清理", r.dashboard.Cleanable), fmt.Sprintf("Scan complete — %s cleanable", r.dashboard.Cleanable))
		}
	})
}

// armJunkBusy posts the occupancy result back to the main thread. The scan
// generation is read there, after start has stored it.
func (r *Root) armJunkBusy(busyDone <-chan map[string]core.Busy) {
	r.publish(func() {
		gen := r.generation
		go func() {
			busy := <-busyDone
			r.publish(func() {
				if gen != r.generation {
					return
				}
				r.junkBusy = busy
				r.junkBusyGen = gen
				r.junkBusySet = true
				if r.junk.Scanned {
					r.applyJunkBusy(busy)
				}
			})
		}()
	})
}

func volumeIndexes(r *Root) []*core.SizeTree {
	seen := map[*core.SizeTree]bool{}
	var out []*core.SizeTree
	add := func(tree *core.SizeTree) {
		if tree == nil || seen[tree] {
			return
		}
		seen[tree] = true
		out = append(out, tree)
	}
	add(r.tree)
	for _, tree := range r.searchTrees {
		add(tree)
	}
	return out
}

func (r *Root) applyReadyJunkBusy() {
	if r.junkBusySet && r.junkBusyGen == r.generation && r.junk.Scanned {
		r.applyJunkBusy(r.junkBusy)
	}
}

func (r *Root) applyJunkBusy(busy map[string]core.Busy) {
	if !r.junk.Scanned {
		return
	}
	wasRecommended := r.junk.RecommendedSelected
	if core.ApplyBusy(r.junk.Categories, busy) == 0 {
		return
	}
	if wasRecommended {
		r.selectJunk(view.JunkRecommended)
		return
	}
	r.junkSummary()
}

func pathUnder(path, root string) bool {
	return core.AtOrUnder(core.Norm(path), core.Norm(root))
}

func (r *Root) dropExcludedJunk(path string) int {
	removed := 0
	for i := range r.junk.Categories {
		cat := &r.junk.Categories[i]
		before := len(cat.Items)
		kept := cat.Items[:0]
		for _, item := range cat.Items {
			if pathUnder(item.Path, path) {
				delete(r.junk.Selected, item.Path)
				continue
			}
			if pathUnder(path, item.Path) && item.Recommended {
				item.Recommended = false
				delete(r.junk.Selected, item.Path)
			}
			kept = append(kept, item)
		}
		cat.Items = kept
		removed += before - len(cat.Items)
		if len(cat.Items) != before {
			var total uint64
			for _, item := range cat.Items {
				total += item.Size
			}
			cat.TotalSize = total
		}
	}
	return removed
}

func (r *Root) junkSummary() {
	var bytes uint64
	var n int
	for _, c := range r.junk.Categories {
		for _, item := range c.Items {
			r.junk.ReleaseEligible[item.Path] = core.CacheReleaseEligible(item)
		}
		bytes += c.TotalSize
		if len(c.Items) > 0 {
			n++
		}
	}
	r.dashboard.Scanned = r.junk.Scanned
	r.dashboard.HasJunk = bytes > 0
	r.dashboard.Cleanable = core.FmtSize(bytes)
	r.dashboard.Categories = n
	match := true
	for _, c := range r.junk.Categories {
		for _, i := range c.Items {
			if r.junk.Selected[i.Path] != i.Recommended {
				match = false
			}
		}
	}
	r.junk.RecommendedSelected = match
}

func (r *Root) selectJunk(mode view.JunkSelection) {
	if r.job != nil && r.job.mutation || r.confirming {
		return
	}
	for _, c := range r.junk.Categories {
		for _, i := range c.Items {
			switch mode {
			case view.JunkRecommended:
				r.junk.Selected[i.Path] = i.Recommended
			case view.JunkAll:
				r.junk.Selected[i.Path] = true
			case view.JunkInvert:
				r.junk.Selected[i.Path] = !r.junk.Selected[i.Path]
			case view.JunkNone:
				r.junk.Selected[i.Path] = false
			}
		}
	}
	r.junkSummary()
}

func (r *Root) junkActions() view.JunkPageActions {
	return view.JunkPageActions{
		Rescan:         r.scanJunk,
		ToggleExpanded: func(id core.CategoryID) { r.junk.Expanded[id] = !r.junk.Expanded[id] },
		ToggleCategory: func(id core.CategoryID) {
			if r.busy() {
				return
			}
			for _, c := range r.junk.Categories {
				if c.Category != id {
					continue
				}
				all := true
				for _, i := range c.Items {
					all = all && r.junk.Selected[i.Path]
				}
				for _, i := range c.Items {
					r.junk.Selected[i.Path] = !all
				}
			}
			r.junkSummary()
		},
		ToggleItem: func(path string) {
			if !r.busy() {
				r.junk.Selected[path] = !r.junk.Selected[path]
				r.junkSummary()
			}
		}, Select: r.selectJunk,
		ToggleExclusions: func() { r.junk.ShowExclusions = !r.junk.ShowExclusions }, ToggleFailedDetails: func() { r.junk.ShowFailedDetails = !r.junk.ShowFailedDetails },
		Exclude: func(path string) {
			if r.busy() || core.IsVirtualPath(path) {
				return
			}
			if !slices.Contains(r.settings.Whitelist, path) {
				r.settings.Whitelist = append(r.settings.Whitelist, path)
				core.ReloadWhitelist(r.settings.Whitelist)
				r.save(r.settings)
				r.junk.Exclusions = append([]string(nil), r.settings.Whitelist...)
				removed := r.dropExcludedJunk(path)
				delete(r.junk.Selected, path)
				r.junkSummary()
				r.message(fmt.Sprintf("已加入排除清单，所有清理都会跳过此路径（已从列表移走 %d 条）", removed), fmt.Sprintf("Added to exclusions — all cleanup will skip this path (%d items removed from the list)", removed))
			}
		},
		RemoveExclusion: func(path string) {
			if r.busy() {
				return
			}
			r.settings.Whitelist = slices.DeleteFunc(r.settings.Whitelist, func(p string) bool { return p == path })
			core.ReloadWhitelist(r.settings.Whitelist)
			r.save(r.settings)
			r.junk.Exclusions = append([]string(nil), r.settings.Whitelist...)
			r.message("已移除该排除项，此路径将在下次扫描后按普通路径处理", "Removed from exclusions — this path will be scanned again next time")
		},
		ReleaseCache: r.requestCacheRelease, RequestClean: r.requestJunkClean,
	}
}

func planSummary(targets []core.CleanTarget) (rules []string, operations []core.OperationKind, blocked []string) {
	seenRule := map[string]bool{}
	seenOp := map[core.OperationKind]bool{}
	seenBlock := map[string]bool{}
	for _, t := range targets {
		for _, plan := range t.Plans {
			if plan == nil {
				continue
			}
			id := plan.RuleID()
			if id != "" && !seenRule[id] {
				seenRule[id] = true
				rules = append(rules, id)
			}
			for _, target := range plan.Targets() {
				if target.Operation.Kind != "" && !seenOp[target.Operation.Kind] {
					seenOp[target.Operation.Kind] = true
					operations = append(operations, target.Operation.Kind)
				}
			}
			for _, reason := range plan.Blocked() {
				if reason != "" && !seenBlock[reason] {
					seenBlock[reason] = true
					blocked = append(blocked, reason)
				}
			}
		}
	}
	slices.Sort(rules)
	slices.Sort(blocked)
	slices.Sort(operations)
	return rules, operations, blocked
}

func (r *Root) requestJunkClean() {
	if r.busy() || !r.junk.Scanned {
		return
	}
	var targets []core.CleanTarget
	var paths []string
	var bytes, files uint64
	for _, c := range r.junk.Categories {
		for _, item := range c.Items {
			if r.junk.Selected[item.Path] {
				targets = append(targets, item.CleanTarget())
				paths = append(paths, item.Path)
				bytes += item.Size
				files += item.FileCount
			}
		}
	}
	droppedBusy := 0
	for _, c := range r.junk.Categories {
		for _, item := range c.Items {
			if r.junk.Selected[item.Path] && item.Busy != nil {
				delete(r.junk.Selected, item.Path)
				droppedBusy++
			}
		}
	}
	targets, paths, bytes, files = nil, nil, 0, 0
	for _, c := range r.junk.Categories {
		for _, item := range c.Items {
			if r.junk.Selected[item.Path] {
				targets = append(targets, item.CleanTarget())
				paths = append(paths, item.Path)
				bytes += item.Size
				files += item.FileCount
			}
		}
	}
	if len(targets) == 0 {
		if droppedBusy > 0 {
			r.message(fmt.Sprintf("%d 个勾选项正在被应用使用，已取消勾选", droppedBusy), fmt.Sprintf("%d selected items are in use and were unchecked", droppedBusy))
		}
		return
	}
	body := r.tr(
		fmt.Sprintf("将删除 %d 项，共 %s。", len(targets), core.FmtSize(bytes)),
		fmt.Sprintf("%d items will be deleted, %s in total.", len(targets), core.FmtSize(bytes)),
	)
	detail := r.tr(
		"缓存、临时文件和构建产物会永久删除；损坏登录项和旧版 IDE 数据会移入废纸篓。",
		"Caches, temporary files, and build artifacts are permanently deleted; broken login items and old IDE data are moved to Trash.",
	)
	rules, operations, blocked := planSummary(targets)
	if len(rules) > 0 {
		detail += "\n\n" + r.tr("计划规则："+strings.Join(rules, ", "), "Planned rules: "+strings.Join(rules, ", "))
	}
	if len(operations) > 0 {
		var names []string
		for _, op := range operations {
			names = append(names, op.Label(r.settings.Language))
		}
		detail += "\n\n" + r.tr("操作："+strings.Join(names, ", "), "Operations: "+strings.Join(names, ", "))
	}
	if len(blocked) > 0 {
		detail += "\n\n" + r.tr(
			fmt.Sprintf("%d 项被阻止，不会执行：%s", len(blocked), strings.Join(blocked, "; ")),
			fmt.Sprintf("%d items are blocked and will not run: %s", len(blocked), strings.Join(blocked, "; ")),
		)
	}
	detail += "\n\n" + body
	r.ask(r.tr("确认清理选中项", "Confirm cleanup"), detail, func() {
		p := core.NewCleanProgress(files, bytes)
		r.junk.Cleaning = true
		r.message(fmt.Sprintf("正在清理 %d 项…", len(paths)), fmt.Sprintf("Cleaning %d items…", len(paths)))
		r.start(true, p, func(context.Context) func() {
			report := core.CleanTargets(targets, p)
			return func() {
				r.junk.Cleaning = false
				// Cancellation leaves unattempted targets in the snapshot. Do not
				// infer success from their absence in the failure list.
				r.finishCleanup("clean", paths, report, p)
				if p.Cancelled() {
					r.junk.Scanned = false
					r.junkSummary()
					return
				}
				r.applyJunkCleanResult(paths, report)
				r.junkSummary()
			}
		})
	})
}

func (r *Root) applyJunkCleanResult(attempted []string, report core.CleanReport) {
	failed := map[string]bool{}
	for _, item := range append(append(append([]core.CleanFailure{}, report.Failed...), report.SkippedItems...), report.Manual...) {
		if item.Path != "" {
			failed[core.Norm(item.Path)] = true
		}
	}
	for i := range r.junk.Categories {
		cat := &r.junk.Categories[i]
		kept := cat.Items[:0]
		for _, item := range cat.Items {
			if slices.Contains(attempted, item.Path) && !failed[core.Norm(item.Path)] {
				delete(r.junk.Selected, item.Path)
				continue
			}
			for _, live := range report.RemainingLive {
				if core.Norm(live.Path) == core.Norm(item.Path) {
					item.Size = live.Bytes
					item.FileCount = live.FileCount
				}
			}
			kept = append(kept, item)
		}
		cat.Items = kept
		var total uint64
		for _, item := range cat.Items {
			total += item.Size
		}
		cat.TotalSize = total
	}
}

func (r *Root) finishCleanup(action string, paths []string, report core.CleanReport, p *core.CleanProgress) {
	snapshot := p.Snapshot()
	r.freed += snapshot.Bytes
	r.dashboard.Freed = core.FmtSize(r.freed)
	r.dashboard.FreedAny = r.freed > 0
	r.junk.FailedDetails = nil
	live := map[string]core.RemainingLive{}
	for _, item := range report.RemainingLive {
		live[item.Path] = item
	}
	for _, info := range report.FailInfo {
		remain := live[info.Path]
		reason := failReason(r.settings.Language, info.Reason)
		r.junk.FailedDetails = append(r.junk.FailedDetails, r.tr(
			fmt.Sprintf("%s · %s（%d 个文件） · %s", reason, core.FmtSize(remain.Bytes), remain.FileCount, info.Path),
			fmt.Sprintf("%s · %s (%d files) · %s", reason, core.FmtSize(remain.Bytes), remain.FileCount, info.Path),
		))
	}
	if len(report.FailInfo) == 0 {
		for _, f := range append(append([]core.CleanFailure{}, report.Failed...), report.Manual...) {
			r.junk.FailedDetails = append(r.junk.FailedDetails, f.Label())
		}
	}
	unresolved := len(report.Failed) + len(report.Manual) + len(report.SkippedItems)
	r.record(action, paths, report.OK, report.Skipped, uint(len(report.Failed)+len(report.Manual)), snapshot.Bytes)
	if unresolved > 0 {
		r.dashboard.StatusText = r.tr(
			fmt.Sprintf("部分清理完成：已删除 %d 个文件，释放 %s（%d 个目标未能清理）", snapshot.Files, core.FmtSize(snapshot.Bytes), unresolved),
			fmt.Sprintf("Cleanup partially complete — %d files deleted, %s freed (%d targets could not be cleaned)", snapshot.Files, core.FmtSize(snapshot.Bytes), unresolved),
		)
	} else {
		r.dashboard.StatusText = r.tr(
			fmt.Sprintf("清理完成：已删除 %d 个文件，释放 %s", snapshot.Files, core.FmtSize(snapshot.Bytes)),
			fmt.Sprintf("Cleanup complete — %d files deleted, %s freed", snapshot.Files, core.FmtSize(snapshot.Bytes)),
		)
	}
	if snapshot.Cancelled {
		r.dashboard.StatusText += " · " + r.tr("已取消，保留未完成目标", "Cancelled; unfinished targets retained")
	}
}

func failReason(lang core.Language, reason core.FailReason) string {
	zh := map[core.FailReason]string{
		core.FailInUse: "被占用", core.FailAccessDenied: "权限不足", core.FailUnverified: "占用状态未知", core.FailChanged: "扫描后已变更",
		core.FailWorktreeDirty: "保留：含未提交改动或未跟踪/忽略文件", core.FailWorktreeLocked: "保留：worktree 已锁定",
		core.FailWorktreeManaged: "保留：应用会话管理的 worktree，请在所属应用中清理", core.FailWorktreeUnverified: "Git 登记或清理状态无法核验", core.FailOther: "未能删除",
	}
	en := map[core.FailReason]string{
		core.FailInUse: "in use", core.FailAccessDenied: "access denied", core.FailUnverified: "in-use check inconclusive", core.FailChanged: "changed since scan",
		core.FailWorktreeDirty: "kept: uncommitted changes or untracked/ignored files", core.FailWorktreeLocked: "kept: worktree is locked",
		core.FailWorktreeManaged: "kept: application-managed worktree; remove through its app", core.FailWorktreeUnverified: "Git registration or cleanup could not be verified", core.FailOther: "not removed",
	}
	table := en
	if lang == core.Chinese {
		table = zh
	}
	if text, ok := table[reason]; ok {
		return text
	}
	return string(reason)
}
