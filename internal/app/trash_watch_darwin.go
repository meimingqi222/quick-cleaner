package app

import (
	"fmt"
	"log"
	"os"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// The uninstall watcher covers every way an app can vanish:
//
//   - ~/.Trash events: a dragged .app still carries its Info.plist, so its
//     identity survives the move and a per-app residual scan names it.
//   - /Applications and ~/Applications events: a bundle deleted in place
//     leaves no plist; diffing the installed-ID snapshot between rounds
//     catches it (the bundle name comes from the previous snapshot).
//   - A periodic orphan sweep is the truth source when both channels miss:
//     ~/Library entries that no installed bundle owns.
//
// Reported IDs live in trashSeen so a bundle is announced once, whether the
// trash or the applications diff saw it first.
func (r *Root) startTrashWatch() {
	home, ok := platform.UserHome()
	if !ok {
		return
	}
	var roots []string
	for _, root := range []string{home + "/.Trash", "/Applications", home + "/Applications"} {
		if st, err := os.Stat(root); err == nil && st.IsDir() {
			roots = append(roots, root)
		}
	}
	watch, err := platform.WatchDirectories(roots)
	if err != nil {
		log.Printf("卸载残留监视不可用：%v", err)
		return
	}
	r.trashWatch = watch
	log.Printf("卸载残留监视已启动：%v", roots)
	go r.trashWatchLoop(watch, home)
}

func (r *Root) trashWatchLoop(watch *platform.FSDirWatch, home string) {
	// Baselines are snapshots, not reports: pre-existing trash and installs
	// must not announce themselves on every launch.
	baseline := platform.TrashedApps(r.ctx, home)
	for _, app := range baseline {
		r.markTrashSeen(app.ID)
	}
	// App identity is cached by path+mtime so each diff round re-reads only
	// what changed instead of spawning plutil for every bundle.
	appCache := platform.AppSnapshotCache{}
	installed := platform.InstalledAppNamesCached(r.ctx, home, appCache)
	log.Printf("卸载监视基线：废纸篓 %d 个、已装应用 %d 个", len(baseline), len(installed))

	// Trash/app-dir churn arrives in bursts; one settle timer collapses a
	// batch. Short enough to answer a drag like the competition's ~1s.
	settle := time.NewTimer(time.Hour)
	settle.Stop()
	// The orphan sweep is the catch-all for deletions both watches missed.
	// It is cheap enough for a half-hour cadence but never runs at startup.
	sweep := time.NewTimer(90 * time.Second)
	defer settle.Stop()
	defer sweep.Stop()
	for {
		select {
		case <-r.ctx.Done():
			return
		case ev, ok := <-watch.Events:
			if !ok {
				return
			}
			log.Printf("卸载监视事件：%v", ev)
			settle.Reset(800 * time.Millisecond)
		case <-settle.C:
			r.trashScanRound(home)
			installed = r.appDiffRound(installed, appCache)
		case <-sweep.C:
			r.orphanSweep()
			sweep.Reset(30 * time.Minute)
		}
	}
}

func (r *Root) trashScanRound(home string) {
	apps := platform.TrashedApps(r.ctx, home)
	for _, app := range apps {
		if r.ctx.Err() != nil {
			return
		}
		id := strings.ToLower(app.ID)
		if !r.markTrashSeen(id) {
			continue
		}
		log.Printf("检测到卸载：%s（废纸篓通道）", id)
		// Report first, verify after: the pending row surfaces in the alert
		// while the still-installed recheck and the residual walk run. A
		// restore or a still-registered copy withdraws it in reportUninstalled.
		r.reportUninstalled(platform.TrashedApp{ID: id, Name: app.Name, Path: app.Path})
	}
}

// appDiffRound catches bundles deleted in place (rm, third-party uninstallers,
// Finder "Delete Immediately"). The previous snapshot supplies the name. An
// ID that comes back means the user restored or reinstalled it; its leftover
// row and any open review for it are withdrawn.
func (r *Root) appDiffRound(prev map[string]platform.TrashedApp, cache platform.AppSnapshotCache) map[string]platform.TrashedApp {
	home, ok := platform.UserHome()
	if !ok {
		return prev
	}
	cur := platform.InstalledAppNamesCached(r.ctx, home, cache)
	for id, old := range prev {
		if r.ctx.Err() != nil {
			return cur
		}
		if _, live := cur[id]; live || !r.markTrashSeen(id) {
			continue
		}
		log.Printf("检测到卸载：%s（应用目录 diff 通道）", id)
		r.reportUninstalled(old)
	}
	r.withdrawReinstalled(cur)
	return cur
}

// withdrawReinstalled drops leftover rows and open reviews for apps that came
// back — restored from the Trash or reinstalled. Keeping the prompt for a live
// app would invite deleting its data. The shared notification set is locked;
// the rows and the open review move over in publish.
func (r *Root) withdrawReinstalled(cur map[string]platform.TrashedApp) {
	var back []string
	r.trashSeenMu.Lock()
	for id := range r.trashSeen {
		if _, live := cur[id]; live {
			back = append(back, id)
		}
	}
	if len(back) == 0 {
		r.trashSeenMu.Unlock()
		return
	}
	for _, id := range back {
		delete(r.trashSeen, id)
	}
	r.trashSeenMu.Unlock()
	cameBack := map[string]bool{}
	for _, id := range back {
		cameBack[id] = true
	}
	r.publish(func() {
		if r.closed {
			return
		}
		withdrawn := false
		for i := 0; i < len(r.trashLeftovers); i++ {
			if cameBack[r.trashLeftovers[i].id] {
				log.Printf("%s 已恢复安装，撤回残留提示", r.trashLeftovers[i].id)
				r.trashLeftovers = append(r.trashLeftovers[:i], r.trashLeftovers[i+1:]...)
				i--
				withdrawn = true
			}
		}
		if r.apps.ResidualsOpen && r.residualApp != nil && cameBack[strings.ToLower(r.residualApp.ID)] {
			r.closeResiduals()
			withdrawn = true
		}
		if withdrawn {
			r.refreshTrayLeftovers()
			r.changed()
		}
	})
}

// orphanSweep reports leftover candidates that belong to no installed app at
// all — the residual inventory for apps removed while the watcher was off.
// The entry refreshes in place and disappears once the leftovers are gone.
func (r *Root) orphanSweep() {
	result, err := r.orphanScan(r.ctx)
	if err != nil {
		log.Printf("孤儿残留巡检失败：%v", err)
	}
	if err != nil || r.ctx.Err() != nil {
		return
	}
	if result != nil {
		log.Printf("孤儿残留巡检：%d 项候选", len(result.Items))
	}
	r.publish(func() {
		if r.closed {
			return
		}
		const orphanID = "orphans"
		for i, l := range r.trashLeftovers {
			if l.id == orphanID {
				r.trashLeftovers = append(r.trashLeftovers[:i], r.trashLeftovers[i+1:]...)
				break
			}
		}
		if result != nil && len(result.Items) > 0 {
			// Owners that already have their own row keep it; the sweep must
			// not list the same leftovers twice. Reported IDs that produced no
			// leftovers are still fair game for the orphan row.
			reported := map[string]bool{}
			for _, l := range r.trashLeftovers {
				reported[l.id] = true
			}
			filtered := *result
			filtered.Items = nil
			filtered.TotalFileSize = 0
			for _, item := range result.Items {
				if owner, ok := item.Owner(); ok && reported[strings.ToLower(owner)] {
					continue
				}
				filtered.Items = append(filtered.Items, item)
				filtered.TotalFileSize += item.Size()
			}
			if len(filtered.Items) > 0 {
				r.trashLeftovers = append(r.trashLeftovers, trayLeftover{
					id:       orphanID,
					app:      core.InstalledApp{Name: r.tr("已卸载残留", "Uninstalled leftovers")},
					result:   filtered,
					selected: map[int]bool{},
				})
				r.surfaceLeftoverAlert()
			}
		}
		r.refreshTrayLeftovers()
		r.changed()
	})
}

// reportUninstalled surfaces a detection as soon as the settle timer fires —
// a pending row the alert and the tray show at once — then fills it in with
// the residual scan, which is the slow part. A trashed bundle keeps its icon
// in its plist; the diff channel has only the previous snapshot's name.
func (r *Root) reportUninstalled(app platform.TrashedApp) {
	inst := core.InstalledApp{ID: app.ID, Name: app.Name, RegistrySubpath: app.ID}
	var icon *ui.Bitmap
	if data := platform.AppIconPNG(app.Path); len(data) > 0 {
		if bitmap, err := ui.DecodeBitmap(data); err == nil {
			icon = bitmap
		}
	}
	r.publish(func() {
		if r.closed {
			return
		}
		r.dropLeftoverRow(app.ID)
		r.trashLeftovers = append(r.trashLeftovers, trayLeftover{id: app.ID, pending: true, icon: icon, app: inst})
		r.refreshTrayLeftovers()
		r.surfaceLeftoverAlert()
	})
	// Spotlight and LaunchServices lag a real drag: only a confirmed live
	// install elsewhere means the app is really still there. It is cheap
	// (Input Methods roots + mdfind + NSWorkspace), unlike InstalledBundleIDs
	// whose lsregister -dump takes seconds.
	if still, known := r.bundleStillInstalled(r.ctx, app.ID); known && still {
		log.Printf("%s 复核为仍在装（恢复或另有副本），撤回", app.ID)
		r.trashSeenMu.Lock()
		delete(r.trashSeen, app.ID)
		r.trashSeenMu.Unlock()
		r.publish(func() {
			if r.closed {
				return
			}
			r.dropLeftoverRow(app.ID)
			r.refreshTrayLeftovers()
		})
		return
	}
	result, err := r.scanAppResiduals(r.ctx, inst)
	if err != nil || r.ctx.Err() != nil {
		log.Printf("%s 残留扫描失败：%v", inst.ID, err)
	}
	if err != nil || len(result.Items) == 0 || r.ctx.Err() != nil {
		if err == nil {
			log.Printf("%s 已卸载但未发现残留", inst.ID)
		}
		r.publish(func() {
			if r.closed {
				return
			}
			r.dropLeftoverRow(app.ID)
			r.refreshTrayLeftovers()
		})
		return
	}
	log.Printf("%s 发现 %d 项残留", inst.ID, len(result.Items))
	r.publish(func() {
		if r.closed {
			return
		}
		r.dropLeftoverRow(app.ID)
		selected := map[int]bool{}
		for i := range result.DefaultSelection() {
			selected[i] = true
		}
		r.trashLeftovers = append(r.trashLeftovers, trayLeftover{id: app.ID, icon: icon, app: inst, result: result, selected: selected})
		r.refreshTrayLeftovers()
		r.message(fmt.Sprintf("检测到「%s」已卸载，找到 %d 项残留", app.Name, len(result.Items)),
			fmt.Sprintf("\"%s\" was uninstalled — %d leftovers found", app.Name, len(result.Items)))
	})
}
