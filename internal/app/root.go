// Package app owns the product lifecycle and turns presentation events into
// validated core work. Presentation state is confined to the MyGo main thread.
package app

import (
	"context"
	"fmt"
	"runtime"
	"runtime/debug"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

const Version = "0.0.18"

type diskOwner interface {
	Load(context.Context, core.VolumeID) (*core.ScanResult, error)
	Close()
}

type Root struct {
	ctx                   context.Context
	cancel                context.CancelFunc
	post                  func(func())
	invalidate            func()
	window                *mygo.Window
	tray                  *mygo.Tray
	popover               *mygo.Window
	trayController        *view.TrayOverviewController
	trayCancel            context.CancelFunc
	closed, confirming    bool
	job                   *work
	generation            uint64
	fanGeneration         atomic.Uint64
	statusRefresh         chan struct{}
	settings              core.Settings
	save                  func(core.Settings)
	record                func(string, []string, uint, uint, uint, uint64)
	confirmation          view.ConfirmationDialog
	accept                func()
	permission            view.PermissionDialog
	updates               *view.UpdateController
	update                view.UpdateSnapshot
	active                view.View
	dashboard             view.Dashboard
	junk                  view.JunkPage
	apps                  view.AppsPage
	dev                   view.DevEnvPage
	status                view.StatusPage
	disk                  view.DiskPage
	declutter             view.DeclutterPage
	search                view.SearchPage
	installed             []core.InstalledApp
	appIcons              map[string]*ui.Bitmap
	appIconGen            uint64
	appIconTried          map[string]bool
	residual              core.ResidualScanResult
	residualApp           *core.InstalledApp
	residualAuthority     func(context.Context, core.InstalledApp, []core.ResidualItem) bool
	uninstallApp          func(core.InstalledApp) error
	scanAppResiduals      func(context.Context, core.InstalledApp) (core.ResidualScanResult, error)
	orphanScan            func(context.Context) (*core.ResidualScanResult, error)
	bundleStillInstalled  func(context.Context, string) (bool, bool)
	appsLoaded, devLoaded bool
	tree                  *core.SizeTree
	searchTrees           map[string]*core.SizeTree
	searchTreesAt         map[string]time.Time
	diskOwner             diskOwner
	currentNode           uint32
	diskSelected          *core.DiskSelectionState
	declutterAuthority    map[string]core.ArbitraryTarget
	searchCancel          context.CancelFunc
	searchGeneration      uint64
	junkBusy              map[string]core.Busy
	junkBusyGen           uint64
	junkBusySet           bool
	freed                 uint64

	// Trash-watch leftovers (macOS): apps the user uninstalled by hand while
	// the app was running, waiting for the user to open the residual review.
	// leftoverAlert is the floating detection panel; leftoverAlertSnapshot is
	// what its content draws.
	trashWatch            *platform.FSDirWatch
	trashSeenMu           sync.Mutex
	trashSeen             map[string]bool
	trashLeftovers        []trayLeftover
	leftoverAlert         *mygo.Window
	leftoverAlertSnapshot view.LeftoverAlert

	// quitting is set once a quit passes OnBeforeQuit. MyGo then closes every
	// window and abandons the quit if one refuses; the main window must not
	// hide to the tray then.
	quitting bool

	// windowHidden tracks the main window being hidden or minimized. The
	// status sampler runs only while statusWanted (Status page, window shown,
	// not paused); statusWake starts a sample when that becomes true.
	windowHidden bool
	statusWanted atomic.Bool
	statusWake   chan struct{}
}

type work struct {
	cancel   context.CancelFunc
	progress *core.CleanProgress
	mutation bool
}

// NewRoot does not scan or persist. The host supplies a FIFO main-thread
// dispatcher; workers never write presentation snapshots directly.
func NewRoot(settings core.Settings, post func(func()), invalidate func()) *Root {
	ctx, cancel := context.WithCancel(context.Background())
	r := &Root{ctx: ctx, cancel: cancel, post: post, invalidate: invalidate, settings: settings,
		save: func(s core.Settings) { s.Save() }, record: core.RecordHistory, diskSelected: core.NewDiskSelectionState(), declutterAuthority: map[string]core.ArbitraryTarget{}}
	r.appIcons = map[string]*ui.Bitmap{}
	r.searchTrees = map[string]*core.SizeTree{}
	r.searchTreesAt = map[string]time.Time{}
	r.trashSeen = map[string]bool{}
	r.residualAuthority = appResidualsAllowed
	r.uninstallApp = uninstallApp
	owner, _ := newDiskOwner(ctx)
	r.diskOwner = owner
	r.scanAppResiduals = func(ctx context.Context, app core.InstalledApp) (core.ResidualScanResult, error) {
		return scanAppResiduals(withAvailableSizes(ctx, owner, nil), app)
	}
	r.orphanScan = func(ctx context.Context) (*core.ResidualScanResult, error) {
		return scanOrphans(withAvailableSizes(ctx, owner, nil))
	}
	r.bundleStillInstalled = platform.BundleIsStillInstalled
	r.dashboard = view.Dashboard{Language: settings.Language, MacOS: runtime.GOOS == "darwin", Elevated: platform.IsElevated(), Version: "v" + Version, Cleanable: "0 B", Freed: "0 B"}
	r.junk = view.JunkPage{Selected: map[string]bool{}, Expanded: map[core.CategoryID]bool{}, ReleaseEligible: map[string]bool{}, Exclusions: append([]string(nil), settings.Whitelist...)}
	r.apps = view.AppsPage{MacOS: runtime.GOOS == "darwin", Sort: core.DefaultAppSortState(), TotalSize: "0 B", FilteredSize: "0 B"}
	r.dev = view.DevEnvPage{Windows: runtime.GOOS == "windows", Selected: map[string]bool{}, Packages: map[string][]core.DevAssetItem{}, ChangedLabels: map[string]string{}}
	r.status.ExpandedGroups = map[string]bool{}
	r.disk.Windows, r.disk.ToRecycleBin = runtime.GOOS == "windows", settings.DeleteToRecycleBin
	r.declutter.Platform = platformName()
	r.declutter.ExpandedPhotoGroups = map[int]bool{}
	return r
}

func platformName() string {
	if runtime.GOOS == "darwin" {
		return "macos"
	}
	return runtime.GOOS
}
func (r *Root) tr(zh, en string) string { return core.NewText(zh, en).Get(r.settings.Language) }
func (r *Root) changed() {
	if r.invalidate != nil {
		r.invalidate()
	}
}
func (r *Root) message(zh, en string) { r.dashboard.StatusText = r.tr(zh, en); r.changed() }
func (r *Root) failure(err error) {
	if err != nil {
		r.dashboard.StatusText = err.Error()
		r.changed()
	}
}
func (r *Root) unavailable() {
	r.message("此操作尚未接入；未执行任何更改。", "This operation is not integrated yet; no changes were made.")
}
func (r *Root) busy() bool {
	return r.closed || r.confirming || r.job != nil || r.update.Status.Kind == core.UpdateInstalling
}

func (r *Root) publish(fn func()) {
	r.post(func() {
		if !r.closed {
			fn()
			r.changed()
		}
	})
}

// start serializes product work, including command-based operations that cannot
// be interrupted safely. Cancel requests stop traversal, not already-issued I/O.
func (r *Root) start(mutation bool, progress *core.CleanProgress, run func(context.Context) func()) bool {
	if r.busy() {
		return false
	}
	ctx, cancel := context.WithCancel(r.ctx)
	r.generation++
	gen := r.generation
	r.job = &work{cancel: cancel, progress: progress, mutation: mutation}
	go func() {
		var finish func()
		var panicked any
		var stack []byte
		func() {
			defer func() {
				if rec := recover(); rec != nil {
					panicked = rec
					stack = debug.Stack()
				}
			}()
			finish = run(ctx)
		}()
		if panicked != nil {
			core.LogPanic("worker", panicked, stack)
		}
		r.publish(func() {
			if gen != r.generation {
				return
			}
			r.job = nil
			if panicked != nil {
				r.clearScanFlags()
				r.failure(fmt.Errorf("internal error: %v", panicked))
				cancel()
				return
			}
			if !mutation && ctx.Err() != nil {
				r.clearScanFlags()
				r.failure(ctx.Err())
				cancel()
				return
			}
			cancel()
			if finish != nil {
				finish()
			}
		})
	}()
	r.changed()
	return true
}

func (r *Root) clearScanFlags() {
	r.junk.Scanning, r.junk.Discovering = false, false
	r.apps.Scanning, r.apps.ResidualScanning = false, false
	r.dev.Scanning, r.dev.LoadingSite = false, ""
	r.disk.Scanning, r.search.Indexing = false, false
	r.declutter.Scanning = false
}

func (r *Root) Cancel() {
	if r.searchCancel != nil {
		r.searchCancel()
		r.searchGeneration++
		r.search.Searching = false
	}
	if r.job != nil {
		r.job.cancel()
		if r.job.progress != nil {
			r.job.progress.RequestCancel()
		}
		r.message("正在停止；已发出的系统操作可能需要等待。", "Stopping; an already-issued native operation may need to finish.")
	}
}

func (r *Root) Close() {
	if r.closed {
		return
	}
	r.fanGeneration.Add(1)
	r.closed = true
	r.cancel()
	if r.job != nil {
		r.job.cancel()
		if r.job.progress != nil {
			r.job.progress.RequestCancel()
		}
	}
	if r.searchCancel != nil {
		r.searchCancel()
	}
	if r.trashWatch != nil {
		r.trashWatch.Close()
	}
	if r.diskOwner != nil {
		r.diskOwner.Close()
	}
	if r.updates != nil {
		r.updates.Close()
	}
	if r.trayCancel != nil {
		r.trayCancel()
	}
	if r.trayController != nil {
		r.trayController.Close()
	}
	if r.popover != nil {
		r.popover.Destroy()
	}
	if r.leftoverAlert != nil {
		r.leftoverAlert.Destroy()
	}
	if r.tray != nil {
		r.tray.Destroy()
	}
}

func (r *Root) ask(title, detail string, accept func()) {
	r.askKind(view.ConfirmDelete, title, detail, accept)
}

func (r *Root) askConfirm(kind view.ConfirmationKind, title, body, detail core.Text, accept func()) {
	if r.busy() {
		return
	}
	r.confirming = true
	r.generation++
	r.confirmation = view.ConfirmationDialog{
		Open:     true,
		Language: r.settings.Language,
		ID:       fmt.Sprint(r.generation),
		Kind:     kind,
		Title:    title,
		Body:     body,
		Detail:   detail,
	}
	r.accept = accept
	r.changed()
}

func (r *Root) askKind(kind view.ConfirmationKind, title, detail string, accept func()) {
	r.askConfirm(kind, core.SameText(title), core.Text{}, core.SameText(detail), accept)
}

func (r *Root) resolveConfirmation(id string, accepted bool) {
	if r.closed || !r.confirming || id != r.confirmation.ID {
		return
	}
	fn := r.accept
	r.accept = nil
	r.confirming = false
	r.confirmation.Open = false
	if accepted && fn != nil {
		fn()
	}
	r.changed()
}

func (r *Root) Navigate(v view.View) {
	if r.closed || v > view.Search {
		return
	}
	r.active = v
	switch v {
	case view.Junk:
		if !r.junk.Scanned && !r.busy() {
			r.scanJunk()
		}
	case view.Apps:
		if !r.appsLoaded && !r.busy() {
			r.scanApps()
		}
	case view.Disk:
		if !r.disk.Loaded && r.disk.Error == "" && !r.busy() {
			r.scanDisk()
		}
	case view.Search:
		if !r.busy() {
			r.scanSearch(false)
		}
	}
	r.changed()
}

func (r *Root) Scan() {
	if r.busy() {
		return
	}
	switch r.active {
	case view.Apps:
		if r.apps.Tab == view.AppsPageDevEnvironments {
			r.scanDev()
		} else {
			r.scanApps()
		}
	case view.Disk:
		r.scanDisk()
	case view.Search:
		// A refresh reloads every volume through the shared owner (FSEvents
		// replay on macOS). Windows drops its copies so all mounts reload.
		if runtime.GOOS != "darwin" {
			r.searchTrees = map[string]*core.SizeTree{}
			r.searchTreesAt = map[string]time.Time{}
		}
		r.scanSearch(true)
	case view.Declutter:
		r.scanDeclutter()
	default:
		r.scanJunk()
	}
}

func (r *Root) Render(c *ui.Context) {
	r.syncStatusSampling()
	defer r.renderDialogs(c)
	l := r.settings.Language
	r.dashboard.Language = l
	r.dashboard.Scanning = r.junk.Scanning || r.junk.Discovering || r.apps.Scanning || r.dev.Scanning || r.disk.Scanning || r.declutter.Scanning || r.search.Indexing
	r.dashboard.Cleaning = r.job != nil && r.job.mutation
	r.dashboard.Stop = r.Cancel
	r.dashboard.CleanProgress = nil
	if r.job != nil && r.job.progress != nil {
		snap := r.job.progress.Snapshot()
		r.dashboard.CleanProgress = &snap
	}
	r.dashboard.StatusTab = r.status.Tab
	r.junk.Language, r.apps.Language, r.dev.Language, r.status.Language, r.disk.Language, r.declutter.Language, r.search.Language = l, l, l, l, l, l, l
	a := view.DashboardActions{Navigate: r.Navigate, Scan: r.Scan, SetLanguage: func(l view.Language) {
		r.settings.Language = l
		r.save(r.settings)
		r.installMenu()
		r.refreshUpdateChrome()
		r.changed()
	}, OpenPermissions: r.permissions, CheckUpdates: r.onVersionClick,
		SetStatusTab: func(t view.StatusTab) { r.status.Tab = t }, ToggleMonitoring: func() {
			r.dashboard.MonitoringPaused = !r.dashboard.MonitoringPaused
			r.syncStatusSampling()
			if !r.dashboard.MonitoringPaused {
				r.refreshStatus()
			}
		}}
	if r.active == view.Overview {
		r.dashboard.Render(c, a)
		return
	}
	r.dashboard.RenderPage(c, r.active, a, func() {
		switch r.active {
		case view.Junk:
			r.junk.RenderContent(c, r.junkActions())
		case view.Apps:
			page := r.apps
			if r.confirming || r.permission.Open || r.updates != nil && r.update.ShowDialog {
				page.ResidualsOpen = false
			}
			page.RenderContent(c, r.appsActions())
		case view.Status:
			r.status.RenderContent(c, r.statusActions())
		case view.Disk:
			r.disk.RenderContent(c, r.diskActions())
		case view.Declutter:
			r.declutter.RenderContent(c, r.declutterActions())
		case view.Search:
			r.search.RenderContent(c, r.searchActions())
		}
	})
}

func (r *Root) permissions() {
	if runtime.GOOS == "darwin" {
		r.permission.Open = true
		r.permission.DontAsk = r.settings.MacOSFDADismissed
		r.checkPermission()
		return
	}
	if runtime.GOOS == "windows" {
		r.ask(r.tr("请求管理员权限", "Request administrator access"), r.tr("将重新启动并保留原始用户上下文。", "Relaunch preserving the original user context."), func() { go platform.RelaunchAsAdminIfNeeded() })
		return
	}
	r.unavailable()
}

func (r *Root) reveal(path string) {
	if path != "" {
		go mygo.Shell.ShowItemInFolder(path)
	}
}
func (r *Root) open(path string) {
	if path != "" {
		go func() { err := mygo.Shell.OpenPath(path); r.publish(func() { r.failure(err) }) }()
	}
}
func (r *Root) copy(path string) {
	mygo.Clipboard.WriteText(path)
	if path == "" {
		return
	}
	if r.active == view.Declutter {
		r.message(fmt.Sprintf("已复制路径: %s", path), fmt.Sprintf("Copied path: %s", path))
		return
	}
	r.message("路径已复制", "Path copied")
}

func confirmScope(r *Root, paths []string) string {
	if len(paths) == 0 {
		return ""
	}
	shown := paths
	if len(shown) > 12 {
		shown = paths[:12]
	}
	list := strings.Join(shown, "\n")
	if len(paths) > len(shown) {
		return "\n\n" + r.tr(
			fmt.Sprintf("范围（共 %d 项，列出前若干）：\n%s", len(paths), list),
			fmt.Sprintf("Scope (%d items, first shown):\n%s", len(paths), list),
		)
	}
	return "\n\n" + r.tr("范围：\n"+list, "Scope:\n"+list)
}

func disposalDetail(r *Root, recycle bool) string {
	if recycle {
		return r.tr("删除方式：移入回收站，可在回收站恢复。", "Deletion method: move to the Recycle Bin; items can be restored from there.")
	}
	return r.tr("删除方式：永久删除，不进入回收站，删除后无法恢复。请确认没有重要数据。", "Deletion method: permanently delete; files do not go to the Recycle Bin and cannot be recovered. Make sure nothing important is selected.")
}

func targetDetails(paths []string) string { return strings.Join(paths, "\n") }
func resultMessage(ok uint, failed int, skipped uint) string {
	return fmt.Sprintf("OK: %d · Failed: %d · Skipped: %d", ok, failed, skipped)
}
