package app

import (
	"context"
	"fmt"
	"runtime"
	"slices"
	"strings"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func (r *Root) loadVolumes() {
	go func() {
		volumes := platform.ListVolumes()
		rows := make([]view.DiskPageVolume, 0, len(volumes))
		for _, v := range volumes {
			row := view.DiskPageVolume{ID: v}
			if total, free, ok := platform.GetVolumeSpace(v); ok {
				row.Space = &view.DiskPageSpace{Total: total, Free: free}
			}
			rows = append(rows, row)
		}
		r.publish(func() {
			r.disk.Volumes = rows
			if len(rows) > 0 && r.disk.Volume.Mount == "" {
				r.disk.Volume = rows[0].ID
				r.disk.Space = rows[0].Space
				if rows[0].Space != nil {
					r.status.DiskTotal = rows[0].Space.Total
					r.status.DiskFree = rows[0].Space.Free
					r.dashboard.DiskSize = core.FmtSize(rows[0].Space.Total)
				}
			}
			if r.active == view.Search && !r.busy() {
				r.scanSearch()
			}
		})
	}()
}

func (r *Root) scanDisk() {
	if r.busy() {
		return
	}
	v := r.disk.Volume
	if v.Mount == "" {
		r.message("没有可用的卷；请稍后重试。", "No volume available; retry after discovery.")
		return
	}
	if r.diskOwner == nil {
		owner, err := newDiskOwner(r.ctx)
		if err != nil {
			r.failure(err)
			return
		}
		r.diskOwner = owner
	}
	owner := r.diskOwner
	r.disk.Scanning = true
	r.search.Indexing = true
	r.disk.Error = ""
	r.message(fmt.Sprintf("正在深度分析磁盘 %s: 空间占用…", v.Display()), fmt.Sprintf("Analyzing space usage on %s:…", v.Display()))
	r.start(false, nil, func(ctx context.Context) func() {
		started := time.Now()
		var result *core.ScanResult
		var err error
		if owner != nil {
			result, err = owner.Load(ctx, v)
		} else {
			result, err = scanNativeVolume(ctx, v)
		}
		if err == nil {
			err = ctx.Err()
		}
		elapsed := time.Since(started).Seconds()
		return func() {
			r.disk.Scanning = false
			r.search.Indexing = false
			if err != nil {
				r.disk.Error = err.Error()
				r.message(fmt.Sprintf("磁盘分析失败：%s", err.Error()), fmt.Sprintf("Disk analysis failed: %s", err.Error()))
				return
			}
			if result == nil || result.Tree == nil {
				r.disk.Error = "Native index returned no tree"
				return
			}
			r.tree = result.Tree
			r.searchTrees[v.Mount] = result.Tree
			r.disk.Loaded = true
			r.disk.UniqueSize = result.UniqueSize
			r.currentNode = r.tree.Root()
			r.diskSelected.Clear()
			r.disk.Partial = len(r.tree.BlockedSubtrees()) > 0
			r.rebuildDisk()
			r.search.Ready = true
			r.setSearchQuery(r.search.Query)
			r.message(
				fmt.Sprintf("磁盘分析完成：已索引 %d 个文件，占用 %s，耗时 %.1fs", result.FileCount, core.FmtSize(result.TotalSize), elapsed),
				fmt.Sprintf("Disk analysis complete — %d files indexed, %s used, took %.1fs", result.FileCount, core.FmtSize(result.TotalSize), elapsed),
			)
		}
	})
}

func (r *Root) rebuildDisk() {
	if r.tree == nil {
		return
	}
	t := r.tree
	if !t.Valid(r.currentNode) {
		r.currentNode = t.Root()
	}
	r.disk.CurrentName = t.NameOf(r.currentNode)
	r.disk.CurrentPath = t.PathOf(r.currentNode)
	r.disk.CurrentSize = t.SizeOf(r.currentNode)
	r.disk.AtRoot = r.currentNode == t.Root()
	r.disk.Crumbs = nil
	for id := r.currentNode; ; {
		r.disk.Crumbs = append([]view.DiskPageCrumb{{ID: id, Name: t.NameOf(id), Path: t.PathOf(id)}}, r.disk.Crumbs...)
		p, ok := t.ParentOf(id)
		if !ok || p == id {
			break
		}
		id = p
	}
	var nodes []core.Node
	if r.disk.Tab == view.DiskFiles {
		nodes = t.LargestFiles(200)
	} else {
		nodes = t.Children(r.currentNode)
	}
	r.disk.Rows = nil
	r.disk.SelectedCount = r.diskSelected.Len()
	r.disk.SelectedSize = r.diskSelected.TotalSize()
	picked, selectable := 0, 0
	for _, n := range nodes {
		path := t.PathOf(n.Idx)
		protected := core.IsProtected(path) || core.IsWhitelisted(path) || core.HasWhitelistEntryUnder(path)
		allowed := !protected && n.Idx != t.Root() && t.CoversSubtree(path)
		row := view.DiskPageRow{Node: n, Path: path, Protected: protected, Selected: r.diskSelected.IsSelected(path), CanSelect: allowed, CanDelete: allowed, CanEnter: n.IsDir}
		if allowed {
			selectable++
			if row.Selected {
				picked++
			}
		}
		r.disk.Rows = append(r.disk.Rows, row)
	}
	r.disk.CanSelectView = selectable > 0
	r.disk.HeaderSelected = selectable > 0 && picked == selectable
	r.disk.HeaderMixed = picked > 0 && picked < selectable
	r.disk.CanRequestDelete = r.disk.SelectedCount > 0
}

func (r *Root) diskActions() view.DiskPageActions {
	return view.DiskPageActions{StartScan: r.scanDisk, SwitchVolume: func(v core.VolumeID) {
		if r.busy() {
			return
		}
		r.disk.Volume = v
		r.disk.Loaded = false
		r.tree = nil
		for _, row := range r.disk.Volumes {
			if row.ID == v {
				r.disk.Space = row.Space
			}
		}
		r.scanDisk()
	},
		Navigate: func(id uint32) {
			if r.tree != nil && r.tree.Valid(id) && r.tree.IsDir(id) {
				r.currentNode = id
				r.rebuildDisk()
			}
		}, Up: func() {
			if r.tree != nil {
				if p, ok := r.tree.ParentOf(r.currentNode); ok {
					r.currentNode = p
					r.rebuildDisk()
				}
			}
		}, SetTab: func(t view.DiskTab) { r.disk.Tab = t; r.rebuildDisk() }, SetCrumbsExpanded: func(on bool) { r.disk.CrumbsExpanded = on },
		ToggleSelected: func(row view.DiskPageRow) {
			if r.busy() || !row.CanSelect {
				return
			}
			r.diskSelected.Toggle(row.Path, row.Node.Size)
			r.rebuildDisk()
		}, SetViewSelected: func(on bool) {
			if r.busy() {
				return
			}
			for _, row := range r.disk.Rows {
				if row.CanSelect {
					r.diskSelected.Set(row.Path, row.Node.Size, on)
				}
			}
			r.rebuildDisk()
		}, ClearSelection: func() {
			if !r.busy() {
				r.diskSelected.Clear()
				r.rebuildDisk()
			}
		},
		RequestDelete: func(row view.DiskPageRow) {
			if row.CanDelete {
				r.requestDiskPathDelete(row)
			}
		}, RequestDeleteSelected: func() {
			r.requestDiskDelete(r.diskSelected.ResolveTargets())
		},
		SetRecycleBin: func(on bool) {
			if !r.busy() {
				r.settings.DeleteToRecycleBin = on
				r.disk.ToRecycleBin = on
				r.save(r.settings)
				r.installMenu()
			}
		}, CopyPath: r.copy,
	}
}

func declutterNoun(tab view.DeclutterTab) (string, string) {
	switch tab {
	case view.DeclutterDownloads:
		return "下载项", "downloads"
	case view.DeclutterLargeFiles:
		return "大文件", "large files"
	case view.DeclutterSimilarPhotos:
		return "相似照片", "similar photos"
	case view.DeclutterDuplicates:
		return "重复文件", "duplicate files"
	default:
		return "项目", "items"
	}
}

func (r *Root) requestDiskPathDelete(row view.DiskPageRow) {
	if r.busy() || !row.CanDelete {
		return
	}
	if core.IsProtected(row.Path) {
		r.message(fmt.Sprintf("「%s」是受保护的系统路径，不能删除", row.Path), fmt.Sprintf("\"%s\" is a protected system path and cannot be deleted", row.Path))
		return
	}
	dir := row.Node.IsDir
	detail := disposalDetail(r, r.settings.DeleteToRecycleBin) + "\n\n" + r.tr("请确认它不是正在使用的程序或数据；被占用或受保护的条目会跳过并如实报告。", "Make sure this is not data of a program that is currently running; protected or in-use items are skipped and reported.") + "\n" + r.tr("操作：删除这个文件。", "Operation: delete this file.")
	if dir {
		detail = disposalDetail(r, r.settings.DeleteToRecycleBin) + "\n\n" + r.tr("请确认它不是正在使用的程序或数据；被占用或受保护的条目会跳过并如实报告。", "Make sure this is not data of a program that is currently running; protected or in-use items are skipped and reported.") + "\n" + r.tr("操作：删除整个目录及其内容。", "Operation: delete the whole folder and its contents.")
	}
	body := r.tr(fmt.Sprintf("将删除 %s（%s）。", row.Path, core.FmtSize(row.Node.Size)), fmt.Sprintf("%s (%s) will be deleted.", row.Path, core.FmtSize(row.Node.Size)))
	path := row.Path
	r.ask(r.tr("确认永久删除", "Confirm permanent deletion"), body+"\n\n"+detail, func() {
		r.requestDiskDelete([]string{path})
	})
}

func (r *Root) requestDiskDelete(paths []string) {
	if r.job != nil || r.tree == nil || !r.disk.Loaded {
		return
	}
	var targets []core.ArbitraryTarget
	for _, path := range paths {
		id, ok := r.tree.FindNodeByPath(path)
		if !ok || id == r.tree.Root() || !r.tree.CoversSubtree(path) || core.IsProtected(path) || core.IsWhitelisted(path) || core.HasWhitelistEntryUnder(path) {
			r.message("所选路径受保护或索引覆盖未知；未删除。", "Selected path is protected or index coverage is unknown; nothing was deleted.")
			return
		}
		target := core.CaptureArbitraryTarget(path)
		if target.Identity == nil {
			r.message("无法冻结目标身份；未删除。", "Target identity could not be frozen; nothing was deleted.")
			return
		}
		targets = append(targets, target)
	}
	r.requestArbitraryClean("disk", targets, func() { r.disk.Loaded = false; r.diskSelected.Clear(); r.scanDisk() })
}

func (r *Root) requestArbitraryClean(action string, targets []core.ArbitraryTarget, finish func()) {
	if len(targets) == 0 || r.job != nil {
		return
	}
	disposal := core.Permanent
	if r.settings.DeleteToRecycleBin {
		disposal = core.RecycleBin
	}
	paths := make([]string, len(targets))
	for i, t := range targets {
		paths[i] = t.Path
	}
	titleZh, titleEn := "确认永久删除选中项", "Confirm permanent deletion"
	detail := r.tr(
		fmt.Sprintf("将永久删除 %d 项，释放约 %s 磁盘空间。", len(targets), core.FmtSize(r.disk.SelectedSize)),
		fmt.Sprintf("%d items will be permanently deleted, freeing about %s.", len(targets), core.FmtSize(r.disk.SelectedSize)),
	) + "\n\n" + disposalDetail(r, disposal == core.RecycleBin) + "\n" + r.tr("请确认它不是正在使用的程序或数据；被占用或受保护的条目会跳过并如实报告。", "Make sure this is not data of a program that is currently running; protected or in-use items are skipped and reported.") + confirmScope(r, paths)
	if action == "declutter" {
		titleZh, titleEn = "确认移入废纸篓", "Confirm move to Trash"
		detail = r.tr(fmt.Sprintf("将把 %d 项移入废纸篓。", len(targets)), fmt.Sprintf("%d items will be moved to the Trash.", len(targets))) + "\n\n" + r.tr("移入废纸篓不释放磁盘空间；需要真正释放时再清空废纸篓。移走的条目可从废纸篓恢复。", "Moving to the Trash does not free disk space; empty the Trash to reclaim it. Moved items can be restored from the Trash.") + confirmScope(r, paths)
	}
	r.ask(r.tr(titleZh, titleEn), detail, func() {
		p := core.NewCleanProgress(0, 0)
		if action == "disk" {
			r.disk.Cleaning = true
			if len(paths) == 1 {
				r.message(fmt.Sprintf("正在删除 %s…", paths[0]), fmt.Sprintf("Deleting %s…", paths[0]))
			} else {
				r.message(fmt.Sprintf("正在批量删除 %d 项…", len(paths)), fmt.Sprintf("Deleting %d selected items…", len(paths)))
			}
		}
		if action == "declutter" {
			r.declutter.Cleaning = true
			r.message(fmt.Sprintf("正在把 %d 项移入废纸篓...", len(paths)), fmt.Sprintf("Moving %d items to Trash...", len(paths)))
		}
		r.start(true, p, func(context.Context) func() {
			report := core.CleanArbitraryItems(targets, disposal, p)
			return func() {
				r.disk.Cleaning = false
				r.declutter.Cleaning = false
				r.finishCleanup(action, paths, report, p)
				if action == "disk" && len(paths) == 1 && !p.Cancelled() {
					if len(report.Failed)+len(report.Manual) > 0 {
						r.message(fmt.Sprintf("删除失败：%s（被占用或权限不足）", paths[0]), fmt.Sprintf("Failed to delete %s (in use or access denied)", paths[0]))
					} else if report.Skipped > 0 {
						r.message(fmt.Sprintf("「%s」是受保护的系统路径，不能删除", paths[0]), fmt.Sprintf("\"%s\" is a protected system path and cannot be deleted", paths[0]))
					} else {
						snap := p.Snapshot()
						r.message(fmt.Sprintf("已删除 %s（%d 个文件，%s）", paths[0], snap.Files, core.FmtSize(snap.Bytes)), fmt.Sprintf("Deleted %s (%d files, %s)", paths[0], snap.Files, core.FmtSize(snap.Bytes)))
					}
				} else if action == "disk" && !p.Cancelled() {
					snap := p.Snapshot()
					skipped := len(report.Failed) + len(report.Manual) + len(report.SkippedItems)
					if skipped == 0 {
						r.message(fmt.Sprintf("批量删除完成：已删除 %d 个文件，释放 %s", snap.Files, core.FmtSize(snap.Bytes)), fmt.Sprintf("Batch delete complete — %d files deleted, %s freed", snap.Files, core.FmtSize(snap.Bytes)))
					} else {
						r.message(fmt.Sprintf("批量删除完成，释放 %s（%d 项受保护或被占用已跳过）", core.FmtSize(snap.Bytes), skipped), fmt.Sprintf("Batch delete complete, freed %s (%d skipped — protected or in use)", core.FmtSize(snap.Bytes), skipped))
					}
				} else if action == "declutter" && !p.Cancelled() {
					failed := len(report.Failed) + len(report.Manual)
					nounZh, nounEn := declutterNoun(r.declutter.Tab)
					if failed == 0 {
						r.message(fmt.Sprintf("已把 %d 个%s移入废纸篓", report.OK, nounZh), fmt.Sprintf("Moved %d %s to Trash", report.OK, nounEn))
					} else {
						r.message(fmt.Sprintf("已把 %d 个%s移入废纸篓；%d 个失败", report.OK, nounZh, failed), fmt.Sprintf("Moved %d %s to Trash; %d failed", report.OK, nounEn, failed))
					}
				}
				if p.Cancelled() {
					r.disk.Loaded = false
					r.declutter.Scanned = false
					return
				}
				finish()
			}
		})
	})
	for _, path := range paths {
		if core.UnderHomeAppSupport(path) {
			r.confirmation.AppData = true
		}
	}
}

func (r *Root) setSearchQuery(q string) {
	r.search.Query = q
	r.searchGeneration++
	gen := r.searchGeneration
	if r.searchCancel != nil {
		r.searchCancel()
	}
	ctx, cancel := context.WithCancel(r.ctx)
	r.searchCancel = cancel
	trees := make([]*core.SizeTree, 0, len(r.searchTrees))
	keys := make([]string, 0, len(r.searchTrees))
	for key := range r.searchTrees {
		keys = append(keys, key)
	}
	slices.Sort(keys)
	for _, key := range keys {
		trees = append(trees, r.searchTrees[key])
	}
	if len(trees) == 0 && r.tree != nil {
		trees = append(trees, r.tree)
	}
	r.search.Results = nil
	r.search.Searching = len(trees) != 0 && q != ""
	if len(trees) == 0 || q == "" {
		return
	}
	go func() {
		timer := time.NewTimer(120 * time.Millisecond)
		defer timer.Stop()
		select {
		case <-ctx.Done():
			return
		case <-timer.C:
		}
		var hits []core.SearchHit
		for _, tree := range trees {
			hits = append(hits, tree.SearchCancellable(ctx, q, 500-len(hits))...)
			if len(hits) >= 500 || ctx.Err() != nil {
				break
			}
		}
		if ctx.Err() != nil {
			return
		}
		r.publish(func() {
			if gen != r.searchGeneration {
				return
			}
			r.search.Results = hits
			r.search.Searching = false
			r.sortSearch()
		})
	}()
}

// Search owns all indexed volumes, not the currently displayed disk tab. macOS
// revisits the shared owner to replay changes; Windows fills missing volumes.
func (r *Root) scanSearch() {
	if r.busy() {
		return
	}
	// Unelevated Windows cannot read $MFT. An index that already exists stays
	// searchable; with none, say why instead of reporting a failed scan.
	if runtime.GOOS == "windows" && !platform.IsElevated() {
		if len(r.searchTrees) == 0 {
			r.message("文件搜索需要管理员权限才能读取 $MFT，请以管理员身份重新运行", "File search requires administrator privileges to read $MFT. Please relaunch as admin")
			return
		}
		r.search.Ready = true
		r.message("搜索索引就绪", "Search index ready")
		r.setSearchQuery(r.search.Query)
		return
	}
	var volumes []core.VolumeID
	for _, row := range r.disk.Volumes {
		if runtime.GOOS == "darwin" || r.searchTrees[row.ID.Mount] == nil {
			volumes = append(volumes, row.ID)
		}
	}
	if len(volumes) == 0 {
		r.search.Ready = len(r.searchTrees) > 0
		r.setSearchQuery(r.search.Query)
		return
	}
	if r.diskOwner == nil {
		owner, err := newDiskOwner(r.ctx)
		if err != nil {
			r.failure(err)
			return
		}
		r.diskOwner = owner
	}
	owner := r.diskOwner
	r.search.Indexing = true
	r.start(false, nil, func(ctx context.Context) func() {
		loaded := map[string]*core.SizeTree{}
		var failures []string
		for _, volume := range volumes {
			if ctx.Err() != nil {
				break
			}
			var scan *core.ScanResult
			var err error
			if owner != nil {
				scan, err = owner.Load(ctx, volume)
			} else {
				scan, err = scanNativeVolume(ctx, volume)
			}
			if err != nil {
				failures = append(failures, volume.Mount+": "+err.Error())
				continue
			}
			if scan == nil || scan.Tree == nil {
				failures = append(failures, volume.Mount+": native index returned no tree")
				continue
			}
			loaded[volume.Mount] = scan.Tree
		}
		return func() {
			r.search.Indexing = false
			for mount, tree := range loaded {
				r.searchTrees[mount] = tree
			}
			r.search.Ready = len(r.searchTrees) > 0
			if len(failures) != 0 {
				r.failure(fmt.Errorf("incomplete search index: %s", strings.Join(failures, "; ")))
			}
			r.setSearchQuery(r.search.Query)
		}
	})
}
func (r *Root) sortSearch() {
	view.SortSearchResults(r.search.Results, r.search.GroupByKind, r.search.SortColumn, r.search.Ascending)
}
func (r *Root) searchActions() view.SearchPageActions {
	return view.SearchPageActions{SetQuery: r.setSearchQuery, ToggleGroupByKind: func() { r.search.GroupByKind = !r.search.GroupByKind; r.sortSearch() }, SortBy: func(c view.SearchSortColumn) {
		if c == r.search.SortColumn {
			r.search.Ascending = !r.search.Ascending
		} else {
			r.search.SortColumn = c
			r.search.Ascending = c != view.SearchSortSize
		}
		r.sortSearch()
	}, Open: r.open, Reveal: r.reveal}
}
