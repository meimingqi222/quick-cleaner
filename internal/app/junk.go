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
		planT0 := time.Now()
		var acquiredTree *core.SizeTree
		if runtime.GOOS == "darwin" && owner != nil {
			root := core.VolumeFromMountPoint("/")
			for _, v := range volumes {
				if v.MountPoint() == "/" {
					root = v
					break
				}
			}
			indexT0 := time.Now()
			if result, err := owner.Load(ctx, root); err != nil {
				log.Printf("整盘索引加载失败，本次垃圾扫描退回遍历通道：%v", err)
			} else if result != nil && result.Tree != nil {
				acquiredTree = result.Tree
				indexes = prioritizeJunkIndex(indexes, acquiredTree)
			}
			log.Printf("整盘索引加载/更新完成：%s（acquired=%v）", time.Since(indexT0).Round(time.Millisecond), acquiredTree != nil)
		}
		targets = append(targets, finishNative()...)
		busyPaths := make([]string, len(targets))
		for i := range targets {
			busyPaths[i] = targets[i].Path
		}
		busyDone := make(chan map[string]core.Busy, 1)
		owned := append([]core.ScanTarget(nil), targets...)
		go func() {
			busy := core.DetectInUse(busyPaths)
			for path, b := range r.runningOwnerBusy(owned) {
				if _, exists := busy[path]; !exists {
					busy[path] = b
				}
			}
			busyDone <- busy
		}()
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
// generation is read there, after start has stored it. Badges arrive late on
// purpose: occupancy never gates publishing, recommendation or selection.
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
				r.applyReadyJunkBusy()
			})
		}()
	})
}

func (r *Root) applyReadyJunkBusy() {
	if r.junkBusySet && r.junkBusyGen == r.generation && r.junk.Scanned {
		core.ApplyBusy(r.junk.Categories, r.junkBusy)
	}
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
			if !item.CleanupAvailable() {
				delete(r.junk.Selected, item.Path)
			}
		}
		cleanable := c.CleanableSize()
		bytes += cleanable
		if cleanable > 0 {
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
			if !i.CleanupAvailable() {
				delete(r.junk.Selected, i.Path)
				continue
			}
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
					if i.CleanupAvailable() {
						all = all && r.junk.Selected[i.Path]
					}
				}
				for _, i := range c.Items {
					r.junk.Selected[i.Path] = i.CleanupAvailable() && !all
				}
			}
			r.junkSummary()
		},
		ToggleItem: func(path string) {
			if !r.busy() {
				for _, category := range r.junk.Categories {
					for _, item := range category.Items {
						if item.Path == path && item.CleanupAvailable() {
							r.junk.Selected[path] = !r.junk.Selected[path]
							r.junkSummary()
							return
						}
					}
				}
			}
		}, Select: r.selectJunk,
		ToggleExclusions: func() { r.junk.ShowExclusions = !r.junk.ShowExclusions }, ToggleFailedDetails: func() { r.junk.ShowFailedDetails = !r.junk.ShowFailedDetails },
		ToggleSkippedDetails: func() { r.junk.ShowSkippedDetails = !r.junk.ShowSkippedDetails },
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
		RetryRelease: r.requestCacheRelease, RequestClean: r.requestJunkClean,
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
	if r.busy() || !r.junk.Scanned || r.junk.Preparing {
		return
	}
	var items []core.ScanItem
	droppedUnavailable := 0
	for _, c := range r.junk.Categories {
		for _, item := range c.Items {
			if !r.junk.Selected[item.Path] {
				continue
			}
			if !item.CleanupAvailable() {
				delete(r.junk.Selected, item.Path)
				droppedUnavailable++
				continue
			}
			items = append(items, item)
		}
	}
	if len(items) == 0 {
		if droppedUnavailable > 0 {
			r.message(fmt.Sprintf("%d 个勾选项当前不可清理，已跳过", droppedUnavailable), fmt.Sprintf("%d selected items are currently unavailable; skipped", droppedUnavailable))
		}
		return
	}
	// Listing owner processes is cheap but still system I/O: keep it off the
	// UI thread, and drop the answer if anything else started meanwhile.
	r.junk.Preparing = true
	gen := r.generation
	find := r.findRunningApps
	go func() {
		apps := find(items)
		r.publish(func() {
			r.junk.Preparing = false
			if gen != r.generation || r.busy() {
				return
			}
			r.confirmJunkClean(items, apps)
		})
	}()
}

// confirmJunkClean shows one confirmation. Running owner apps appear in it as
// checked boxes: checked apps are asked to quit before cleaning, unchecked
// apps' items are left out of this run.
func (r *Root) confirmJunkClean(items []core.ScanItem, apps []core.RunningApp) {
	var bytes uint64
	var targets []core.CleanTarget
	for _, item := range items {
		targets = append(targets, item.CleanTarget())
		bytes += item.Size
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
		r.runJunkClean(items, apps)
	})
	for _, app := range apps {
		label := fmt.Sprintf("%s · %s", app.Name.Get(r.settings.Language), core.FmtSize(app.Bytes))
		r.confirmation.Apps = append(r.confirmation.Apps, view.ConfirmApp{Label: label, Checked: true})
	}
}

func (r *Root) toggleConfirmApp(id string, index int) {
	if !r.confirming || id != r.confirmation.ID || index < 0 || index >= len(r.confirmation.Apps) {
		return
	}
	r.confirmation.Apps[index].Checked = !r.confirmation.Apps[index].Checked
	r.changed()
}

func (r *Root) runJunkClean(items []core.ScanItem, apps []core.RunningApp) {
	// resolveConfirmation closes the dialog but keeps its last snapshot, so
	// the boxes the user left checked are still readable here.
	var closing []core.RunningApp
	skipped := map[string]bool{}
	var skippedNames []string
	for i, app := range apps {
		if i < len(r.confirmation.Apps) && r.confirmation.Apps[i].Checked {
			closing = append(closing, app)
			continue
		}
		skippedNames = append(skippedNames, app.Name.Get(r.settings.Language))
		for _, path := range app.Paths {
			skipped[path] = true
		}
	}
	var targets []core.CleanTarget
	var paths []string
	var bytes, files uint64
	for _, item := range items {
		if skipped[item.Path] {
			continue
		}
		targets = append(targets, item.CleanTarget())
		paths = append(paths, item.Path)
		bytes += item.Size
		files += item.FileCount
	}
	appNote := func() string {
		if len(skippedNames) == 0 {
			return ""
		}
		return r.tr(
			fmt.Sprintf("「%s」正在运行，其缓存 %d 项本次跳过", strings.Join(skippedNames, "」、「"), len(items)-len(targets)),
			fmt.Sprintf("%s still running; %d of its cache items skipped", strings.Join(skippedNames, ", "), len(items)-len(targets)),
		)
	}
	if len(targets) == 0 {
		r.dashboard.StatusText = appNote()
		r.changed()
		return
	}
	p := core.NewCleanProgress(files, bytes)
	r.junk.Cleaning = true
	r.message(fmt.Sprintf("正在清理 %d 项…", len(paths)), fmt.Sprintf("Cleaning %d items…", len(paths)))
	closeApps := r.closeApps
	r.start(true, p, func(context.Context) func() {
		var stillRunning []core.Text
		if len(closing) > 0 {
			stillRunning = closeApps(closing, p.Cancelled)
		}
		report := core.CleanTargets(targets, p)
		occupants := identifyCleanupOccupants(report)
		return func() {
			r.junk.Cleaning = false
			// Cancellation leaves unattempted targets in the snapshot. Do not
			// infer success from their absence in the failure list.
			r.finishCleanup("clean", paths, report, p, occupants)
			var notes []string
			if note := appNote(); note != "" {
				notes = append(notes, note)
			}
			if len(stillRunning) > 0 {
				var names []string
				for _, name := range stillRunning {
					names = append(names, name.Get(r.settings.Language))
				}
				notes = append(notes, r.tr(
					fmt.Sprintf("「%s」未在 8 秒内退出，仍被占用的文件已跳过", strings.Join(names, "」、「")),
					fmt.Sprintf("%s did not quit within 8 s; files it still holds were skipped", strings.Join(names, ", ")),
				))
			}
			if len(notes) > 0 {
				sep := r.tr("；", "; ")
				r.dashboard.StatusText += sep + strings.Join(notes, sep)
			}
			if p.Cancelled() {
				r.junk.Scanned = false
				r.junkSummary()
				return
			}
			r.applyJunkCleanResult(paths, report)
			r.junkSummary()
		}
	})
}

func (r *Root) applyJunkCleanResult(attempted []string, report core.CleanReport) {
	busy := map[string]core.Busy{}
	for _, info := range report.FailInfo {
		switch info.Reason {
		case core.FailInUse:
			busy[info.Path] = core.Busy{Open: true}
		case core.FailUnverified:
			busy[info.Path] = core.Busy{Unknown: true}
		}
	}
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
	core.ApplyBusy(r.junk.Categories, busy)
}

func (r *Root) finishCleanup(action string, paths []string, report core.CleanReport, p *core.CleanProgress, occupants map[string][]platform.Occupant) {
	snapshot := p.Snapshot()
	r.freed += snapshot.Bytes
	r.dashboard.Freed = core.FmtSize(r.freed)
	r.dashboard.FreedAny = r.freed > 0
	r.junk.FailedDetails, r.junk.SkippedDetails = nil, nil
	live := map[string]core.RemainingLive{}
	for _, item := range report.RemainingLive {
		live[item.Path] = item
	}
	skippedTargets := 0
	for _, info := range report.FailInfo {
		remain := live[info.Path]
		owners := occupants[info.Path]
		if len(owners) == 0 {
			owners = occupants[core.Norm(info.Path)]
		}
		detail, skipped := r.junkDetail(info, remain, owners)
		if skipped {
			skippedTargets++
			r.junk.SkippedDetails = append(r.junk.SkippedDetails, detail)
		} else {
			r.junk.FailedDetails = append(r.junk.FailedDetails, detail)
		}
	}
	if len(report.FailInfo) == 0 {
		for _, f := range append(append([]core.CleanFailure{}, report.Failed...), report.Manual...) {
			r.junk.FailedDetails = append(r.junk.FailedDetails, view.JunkDetail{Text: f.Label()})
		}
	}
	unresolved := len(report.Failed) + len(report.Manual) + len(report.SkippedItems)
	skippedLeaves := uint(0)
	for _, info := range report.FailInfo {
		skippedLeaves += info.SkippedLeaves
	}
	failedCount := uint(len(report.Failed) + len(report.Manual))
	if skippedLeaves < failedCount {
		failedCount -= skippedLeaves
	} else {
		failedCount = 0
	}
	r.record(action, paths, report.OK, report.Skipped+skippedLeaves, failedCount, snapshot.Bytes)
	failedTargets := len(report.FailInfo) - skippedTargets
	switch {
	case unresolved > 0 && skippedTargets > 0 && failedTargets == 0 && len(report.Manual) == 0 && len(report.SkippedItems) == 0:
		r.dashboard.StatusText = r.tr(
			fmt.Sprintf("清理完成：已删除 %d 个文件，释放 %s（%d 个目标正在使用或无法确认，已跳过）", snapshot.Files, core.FmtSize(snapshot.Bytes), skippedTargets),
			fmt.Sprintf("Cleanup complete — %d files deleted, %s freed (%d target(s) in use; skipped)", snapshot.Files, core.FmtSize(snapshot.Bytes), skippedTargets),
		)
	case unresolved > 0:
		var zhParts, enParts []string
		if failedTargets > 0 {
			zhParts = append(zhParts, fmt.Sprintf("%d 个目标未能清理", failedTargets))
			enParts = append(enParts, fmt.Sprintf("%d target(s) could not be cleaned", failedTargets))
		}
		if manual := len(report.Manual); manual > 0 {
			zhParts = append(zhParts, fmt.Sprintf("%d 项需手动处理", manual))
			enParts = append(enParts, fmt.Sprintf("%d item(s) need manual action", manual))
		}
		if rule := len(report.SkippedItems); rule > 0 {
			zhParts = append(zhParts, fmt.Sprintf("%d 项按规则跳过", rule))
			enParts = append(enParts, fmt.Sprintf("%d item(s) skipped by rules", rule))
		}
		if skippedTargets > 0 {
			zhParts = append(zhParts, fmt.Sprintf("%d 个目标正在使用或无法确认，已跳过", skippedTargets))
			enParts = append(enParts, fmt.Sprintf("%d target(s) in use or unverified; skipped", skippedTargets))
		}
		if len(zhParts) == 0 {
			zhParts = append(zhParts, fmt.Sprintf("%d 项未能清理", unresolved))
			enParts = append(enParts, fmt.Sprintf("%d item(s) could not be cleaned", unresolved))
		}
		r.dashboard.StatusText = r.tr(
			fmt.Sprintf("部分清理完成：已删除 %d 个文件，释放 %s（%s）", snapshot.Files, core.FmtSize(snapshot.Bytes), strings.Join(zhParts, "；")),
			fmt.Sprintf("Cleanup partially complete — %d files deleted, %s freed (%s)", snapshot.Files, core.FmtSize(snapshot.Bytes), strings.Join(enParts, "; ")),
		)
	default:
		r.dashboard.StatusText = r.tr(
			fmt.Sprintf("清理完成：已删除 %d 个文件，释放 %s", snapshot.Files, core.FmtSize(snapshot.Bytes)),
			fmt.Sprintf("Cleanup complete — %d files deleted, %s freed", snapshot.Files, core.FmtSize(snapshot.Bytes)),
		)
	}
	if snapshot.Cancelled {
		r.dashboard.StatusText += " · " + r.tr("已取消，保留未完成目标", "Cancelled; unfinished targets retained")
	}
}

// A target is neutral only when all its failed leaves are in-use/unverified.
// The core summary prioritizes real errors in mixed targets.
func (r *Root) junkDetail(info core.TargetFailInfo, remain core.RemainingLive, owners []platform.Occupant) (view.JunkDetail, bool) {
	zhLabel := failReason(core.Chinese, info.Reason)
	enLabel := failReason(core.English, info.Reason)
	system := false
	switch info.Reason {
	case core.FailInUse:
		zhLabel, enLabel, system = r.occupancyLabel(info.Path, owners)
	case core.FailUnverified:
		zhLabel = "无法确认占用或清理授权，已跳过"
		enLabel = "Occupancy or cleanup authority unverified; skipped"
	}
	skipped := info.Reason == core.FailInUse || info.Reason == core.FailUnverified
	zhText := fmt.Sprintf("%s · %s（%d 个文件） · %s", zhLabel, core.FmtSize(remain.Bytes), remain.FileCount, info.Path)
	enText := fmt.Sprintf("%s · %s (%d files) · %s", enLabel, core.FmtSize(remain.Bytes), remain.FileCount, info.Path)
	release := info.Reason == core.FailInUse && !system && r.junk.ReleaseEligible[info.Path]
	return view.JunkDetail{Text: r.tr(zhText, enText), Path: info.Path, Release: release}, skipped
}

func (r *Root) occupancyLabel(path string, owners []platform.Occupant) (zh, en string, system bool) {
	if len(owners) == 0 {
		if app := r.junkBusyApp(path); app != "" {
			return app + " 正在使用", app + " is running", false
		}
		return "被占用（未能识别应用）", "In use (app not identified)", false
	}
	var names []string
	anySystem, onlySystem := false, true
	seen := map[string]bool{}
	for _, owner := range owners {
		if owner.System {
			anySystem = true
			continue
		}
		onlySystem = false
		if owner.Name == "" || seen[owner.Name] {
			continue
		}
		seen[owner.Name] = true
		names = append(names, owner.Name)
	}
	if onlySystem {
		return "系统、其他用户或受保护进程占用，已跳过", "System, another user, or a protected process; skipped", true
	}
	if len(names) > 3 {
		zh = "「" + strings.Join(names[:3], "」、「") + "」等应用正在使用"
		en = "\"" + strings.Join(names[:3], "\", \"") + "\" and others are running"
	} else if len(names) > 0 {
		verb := "is running"
		if len(names) > 1 {
			verb = "are running"
		}
		zh = "「" + strings.Join(names, "」、「") + "」正在使用"
		en = "\"" + strings.Join(names, "\", \"") + "\" " + verb
	}
	if len(names) == 0 {
		zh, en = "被占用（未能识别应用）", "In use (app not identified)"
	}
	if anySystem {
		zh += "（另有系统进程占用）"
		en += " (plus a system process)"
	}
	return zh, en, anySystem
}

func (r *Root) junkBusyApp(path string) string {
	for _, category := range r.junk.Categories {
		for _, item := range category.Items {
			if item.Path == path && item.Busy != nil && item.Busy.App != "" {
				return item.Busy.App
			}
		}
	}
	return ""
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
