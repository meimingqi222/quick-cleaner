package app

import (
	"context"
	"errors"
	"fmt"
	"io/fs"
	"log"
	"os"
	"path/filepath"
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
				r.scanSearch(false)
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
	savedPath := r.disk.CurrentPath
	r.disk.Scanning = true
	r.search.Indexing = true
	r.disk.Error = ""
	r.message(fmt.Sprintf("正在深度分析磁盘 %s: 空间占用…", v.Display()), fmt.Sprintf("Analyzing space usage on %s:…", v.Display()))
	r.start(false, nil, func(ctx context.Context) func() {
		started := time.Now()
		var result *core.ScanResult
		var err error
		// Windows: a tree scanned this session (junk prescan, search, an
		// earlier lens visit) is reused while fresh instead of re-parsing
		// $MFT. Older trees are stale enough that rows would lie.
		if owner == nil && time.Since(r.searchTreesAt[v.Mount]) < diskIndexReuseTTL {
			if cached := r.searchTrees[v.Mount]; cached != nil {
				log.Printf("磁盘透镜复用会话内索引 %s（%.0fs 前解析）", v.Mount, time.Since(r.searchTreesAt[v.Mount]).Seconds())
				result = &core.ScanResult{Volume: v, Tree: cached, TotalSize: cached.SizeOf(cached.Root()), FileCount: cached.CountUsedFiles()}
			}
		}
		if result == nil {
			if owner != nil {
				result, err = owner.Load(ctx, v)
			} else {
				result, err = scanNativeVolume(ctx, v)
			}
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
			r.searchTreesAt[v.Mount] = time.Now()
			r.disk.Loaded = true
			r.disk.UniqueSize = result.UniqueSize
			r.currentNode = r.tree.Root()
			if savedPath != "" && strings.HasPrefix(savedPath, v.Mount) {
				if ids := r.tree.FindPath(savedPath); len(ids) > 0 {
					r.currentNode = ids[len(ids)-1]
				}
			}
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

// diskIndexReuseTTL is how old a session-cached volume index may be before
// the disk lens re-parses it. $MFT drifts constantly; two minutes keeps a
// second parse off the fast path without showing noticeably stale rows.
const diskIndexReuseTTL = 2 * time.Minute

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
	r.disk.Rows = make([]view.DiskPageRow, 0, len(nodes))
	for _, n := range nodes {
		path := t.PathOf(n.Idx)
		protected := core.IsProtected(path) || core.IsWhitelisted(path) || core.HasWhitelistEntryUnder(path)
		allowed := !protected && n.Idx != t.Root() && t.CoversSubtree(path)
		r.disk.Rows = append(r.disk.Rows, view.DiskPageRow{Node: n, Path: path, Protected: protected, CanSelect: allowed, CanDelete: allowed, CanEnter: n.IsDir})
	}
	r.refreshDiskSelection()
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
			r.refreshDiskSelection()
		}, SetViewSelected: func(on bool) {
			if r.busy() {
				return
			}
			paths := make([]string, 0, len(r.disk.Rows))
			sizes := make([]uint64, 0, len(r.disk.Rows))
			for _, row := range r.disk.Rows {
				if row.CanSelect {
					paths = append(paths, row.Path)
					sizes = append(sizes, row.Node.Size)
				}
			}
			r.diskSelected.SetMany(paths, sizes, on)
			r.refreshDiskSelection()
		}, ClearSelection: func() {
			if !r.busy() {
				r.diskSelected.Clear()
				r.refreshDiskSelection()
			}
		},
		RequestDelete: func(row view.DiskPageRow) {
			if row.CanDelete {
				r.requestDiskPathDelete(row)
			}
		}, RequestDeleteSelected: r.requestSelectedDiskDelete,
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

// requestSelectedDiskDelete resolves exclusions into removable paths off the UI
// thread: a directory being split around an excluded child is walked recursively,
// which on a large folder froze the window before the confirmation appeared.
func (r *Root) requestSelectedDiskDelete() {
	if r.busy() || r.diskSelected.IsEmpty() {
		return
	}
	r.start(false, nil, func(ctx context.Context) func() {
		paths := r.diskSelected.ResolveTargets()
		targets, gone, failed := captureDiskTargets(ctx, paths)
		return func() { r.requestDiskTargets(paths, targets, gone, failed) }
	})
}

// captureDiskTargets freezes the identity of every path still on disk. A path
// confirmed missing has nothing to delete and is returned in gone; the index
// lags the disk, so a select-all in a churning folder like Temp always has a
// few. A path that exists but cannot be frozen is returned in failed: unknown
// identity refuses the whole batch, it is never skipped like a missing path.
func captureDiskTargets(ctx context.Context, paths []string) (targets []core.ArbitraryTarget, gone []string, failed string) {
	return captureDiskTargetsUsing(ctx, paths, core.CaptureArbitraryTarget)
}

func captureDiskTargetsUsing(ctx context.Context, paths []string, capture func(string) core.ArbitraryTarget) (targets []core.ArbitraryTarget, gone []string, failed string) {
	for _, path := range paths {
		if ctx.Err() != nil {
			return nil, nil, path
		}
		target := capture(path)
		if target.Identity != nil {
			targets = append(targets, target)
			continue
		}
		if _, err := os.Lstat(path); errors.Is(err, fs.ErrNotExist) {
			gone = append(gone, path)
			continue
		}
		return nil, nil, path
	}
	return targets, gone, ""
}

func (r *Root) requestDiskDelete(paths []string) {
	targets, gone, failed := captureDiskTargets(context.Background(), paths)
	r.requestDiskTargets(paths, targets, gone, failed)
}

func (r *Root) requestDiskTargets(paths []string, targets []core.ArbitraryTarget, gone []string, failed string) {
	if r.job != nil || r.tree == nil || !r.disk.Loaded {
		return
	}
	if failed != "" {
		r.message(fmt.Sprintf("无法冻结目标身份（%s）；未删除。", failed), fmt.Sprintf("Target identity could not be frozen (%s); nothing was deleted.", failed))
		return
	}
	// Size only what will be deleted: the selection total also counts rows
	// already gone from disk.
	var size uint64
	for _, target := range targets {
		path := target.Path
		id, ok := r.tree.FindNodeByPath(path)
		if !ok || id == r.tree.Root() || !r.tree.CoversSubtree(path) || core.IsProtected(path) || core.IsWhitelisted(path) || core.HasWhitelistEntryUnder(path) {
			r.message("所选路径受保护或索引覆盖未知；未删除。", "Selected path is protected or index coverage is unknown; nothing was deleted.")
			return
		}
		size += r.tree.SizeOf(id)
	}
	if len(targets) == 0 {
		if len(gone) > 0 {
			r.pruneDiskPaths(gone)
			r.message(fmt.Sprintf("所选 %d 项已不在磁盘上，已从列表移除。", len(gone)), fmt.Sprintf("%d selected items are no longer on disk and were removed from the list.", len(gone)))
		}
		return
	}
	r.requestArbitraryClean("disk", targets, size, "", func(core.CleanReport) { r.pruneDiskPaths(paths) })
}

// pruneDiskPaths removes the deleted paths from the in-memory index instead
// of rescanning the volume: ancestors lose their sizes, the search index
// (the same tree) stays consistent, and the position keeps the deepest
// surviving ancestor—matching the Rust reference's prune_deleted_from_mft.
// Which paths are really gone is checked on a worker: a select-all delete in
// a large folder costs one stat per row.
//
// The file-search goroutine walks this same tree. Cancelling it only signals,
// so the worker waits for it to exit, and new searches are held until the
// edit is applied; otherwise reader and writer race on the tree slots.
// Free space is re-read from the system: a path that left for the Trash still
// occupies the volume, so pruned sizes say nothing about free space.
func (r *Root) pruneDiskPaths(paths []string) {
	if r.tree == nil || len(paths) == 0 {
		return
	}
	reader := r.holdSearch()
	volume, space := r.disk.Volume, r.volumeSpace
	// A mutation job: even when cancelled its finish runs, so the confirmed
	// part is applied and searches are released.
	started := r.start(true, nil, func(ctx context.Context) func() {
		if reader != nil {
			<-reader
		}
		gone := make([]string, 0, len(paths))
		for _, path := range paths {
			if ctx.Err() != nil {
				break
			}
			if _, err := os.Stat(path); os.IsNotExist(err) {
				gone = append(gone, path)
			}
		}
		total, free, ok := space(volume)
		return func() {
			r.applyDiskPrune(gone)
			if ok {
				r.setVolumeSpace(volume, total, free)
			}
			r.releaseSearch()
		}
	})
	if !started {
		r.releaseSearch()
	}
}

// holdSearch stops the running search and keeps new ones from starting until
// releaseSearch. It returns a channel closed when the stopped search exits.
func (r *Root) holdSearch() <-chan struct{} {
	reader := r.searchDone
	if r.searchCancel != nil {
		r.searchCancel()
		r.searchCancel = nil
	}
	r.searchDone = nil
	r.searchGeneration++
	r.searchHeld = true
	return reader
}

func (r *Root) releaseSearch() {
	r.searchHeld = false
	if r.search.Query != "" {
		r.setSearchQuery(r.search.Query)
	} else {
		r.search.Searching = false
	}
}

func (r *Root) setVolumeSpace(volume core.VolumeID, total, free uint64) {
	space := &view.DiskPageSpace{Total: total, Free: free}
	for i := range r.disk.Volumes {
		if r.disk.Volumes[i].ID == volume {
			r.disk.Volumes[i].Space = space
		}
	}
	if r.disk.Volume == volume {
		r.disk.Space = space
	}
}

// applyDiskPrune removes paths already confirmed gone from disk. Callers go
// through pruneDiskPaths, which has already stopped every tree reader.
func (r *Root) applyDiskPrune(gone []string) {
	t := r.tree
	if t == nil {
		return
	}
	// Record the position's ancestor chain while every node is valid:
	// ParentOf may refuse an already-removed slot.
	var chain []uint32
	for id := r.currentNode; t.Valid(id); {
		chain = append(chain, id)
		p, ok := t.ParentOf(id)
		if !ok || p == id {
			break
		}
		id = p
	}
	for _, path := range gone {
		if idx, ok := t.FindNodeByPath(path); ok && idx != t.Root() {
			t.RemoveSubtreeInplace(idx)
		}
	}
	for _, id := range chain {
		if t.Valid(id) {
			r.currentNode = id
			break
		}
	}
	if !t.Valid(r.currentNode) {
		r.currentNode = t.Root()
	}
	r.diskSelected.Clear()
	r.rebuildDisk()
}

func (r *Root) requestArbitraryClean(action string, targets []core.ArbitraryTarget, size uint64, warning string, finish func(core.CleanReport)) {
	if len(targets) == 0 || r.job != nil {
		return
	}
	// Declutter always promises Trash ("restorable"), as the Rust reference
	// did; only the disk lens follows the delete-to-recycle-bin setting.
	disposal := core.Permanent
	if r.settings.DeleteToRecycleBin || action == "declutter" {
		disposal = core.RecycleBin
	}
	paths := make([]string, len(targets))
	for i, t := range targets {
		paths[i] = t.Path
	}
	titleZh, titleEn := "确认永久删除选中项", "Confirm permanent deletion"
	detail := r.tr(
		fmt.Sprintf("将永久删除 %d 项，释放约 %s 磁盘空间。", len(targets), core.FmtSize(size)),
		fmt.Sprintf("%d items will be permanently deleted, freeing about %s.", len(targets), core.FmtSize(size)),
	) + "\n\n" + disposalDetail(r, disposal == core.RecycleBin) + "\n" + r.tr("请确认它不是正在使用的程序或数据；被占用或受保护的条目会跳过并如实报告。", "Make sure this is not data of a program that is currently running; protected or in-use items are skipped and reported.") + confirmScope(r, paths)
	if action == "declutter" {
		titleZh, titleEn = "确认移入废纸篓", "Confirm move to Trash"
		detail = r.tr(fmt.Sprintf("将把 %d 项移入废纸篓。", len(targets)), fmt.Sprintf("%d items will be moved to the Trash.", len(targets))) + "\n\n" + r.tr("移入废纸篓不释放磁盘空间；需要真正释放时再清空废纸篓。移走的条目可从废纸篓恢复。", "Moving to the Trash does not free disk space; empty the Trash to reclaim it. Moved items can be restored from the Trash.") + confirmScope(r, paths)
	}
	if warning != "" {
		detail = warning + "\n\n" + detail
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
			report := r.cleanArbitrary(targets, disposal, p)
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
				finish(report)
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
		r.searchCancel = nil
	}
	if r.searchHeld {
		// A prune is editing the tree; releaseSearch reruns this query.
		r.search.Searching = q != ""
		return
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
	// Cancelling the previous search does not stop it at once. Its done channel
	// is chained into this one, so searchDone closing means every earlier
	// reader has left the tree too; pruneDiskPaths waits on that alone.
	previous := r.searchDone
	done := make(chan struct{})
	r.searchDone = done
	go func() {
		defer func() {
			if previous != nil {
				<-previous
			}
			close(done)
		}()
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

// Search owns all indexed volumes, not the currently displayed disk tab.
// Entering the menu only fills volumes that have no index yet; force (the
// Scan button) revisits the shared owner on macOS to replay changes.
func (r *Root) scanSearch(force bool) {
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
		if r.searchTrees[row.ID.Mount] == nil || (force && runtime.GOOS == "darwin") {
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
				r.searchTreesAt[mount] = time.Now()
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

// refreshDiskSelection updates only the selection state of the current rows.
// A checkbox click cannot change a row's protection, whitelist or coverage, so
// it must not re-run those per-row checks (about 50 ms on a 7400-entry folder).
// Rows of one folder share a parent: its state is resolved once and each row
// checks only its own entry.
func (r *Root) refreshDiskSelection() {
	parent, parentSelected := "", false
	if r.tree != nil && r.disk.Tab != view.DiskFiles {
		parent = filepath.Clean(r.disk.CurrentPath)
		parentSelected = r.diskSelected.IsSelected(parent)
	}
	r.disk.SelectedCount = r.diskSelected.Len()
	r.disk.SelectedSize = r.diskSelected.TotalSize()
	picked, selectable := 0, 0
	for i := range r.disk.Rows {
		row := &r.disk.Rows[i]
		if parent != "" && filepath.Dir(row.Path) == parent {
			row.Selected = r.diskSelected.ChildSelected(row.Path, parentSelected)
		} else {
			row.Selected = r.diskSelected.IsSelected(row.Path)
		}
		if row.CanSelect {
			selectable++
			if row.Selected {
				picked++
			}
		}
	}
	r.disk.CanSelectView = selectable > 0
	r.disk.HeaderSelected = selectable > 0 && picked == selectable
	r.disk.HeaderMixed = picked > 0 && picked < selectable
	r.disk.CanRequestDelete = r.disk.SelectedCount > 0
}
