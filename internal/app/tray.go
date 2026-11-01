package app

import (
	"context"
	"log"
	"path/filepath"
	"runtime"
	"slices"
	"strings"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/assets"
	"github.com/meimingqi222/quick-cleaner/internal/core"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

// trayLeftover is one detected uninstall waiting for review, stored on Root
// so the alert panel, the tray popover and the residual review share the
// frozen evidence. pending marks a detection whose residual scan is still
// running — the alert reports it immediately instead of waiting for the walk.
// selected tracks the in-card picklist; cleaning marks an in-flight cleanup.
type trayLeftover struct {
	id       string
	pending  bool
	cleaning bool
	selected map[int]bool
	icon     *ui.Bitmap
	app      core.InstalledApp
	result   core.ResidualScanResult
}

func (r *Root) trayMenu() *mygo.Menu {
	return mygo.NewMenu([]*mygo.MenuItem{
		{Label: "QuickCleaner", Click: func(*mygo.MenuItem, *mygo.Window) { r.showMain() }},
		{Label: r.tr("概览", "Overview"), Click: func(*mygo.MenuItem, *mygo.Window) { r.togglePopover() }},
		{Label: r.tr("扫描垃圾", "Scan junk"), Click: func(*mygo.MenuItem, *mygo.Window) { r.routeOverview(view.OverviewAction{Kind: view.OverviewScanJunk}) }},
		{Role: mygo.RoleQuit},
	})
}

func (r *Root) startTray() {
	opts := mygo.TrayOptions{ID: "quick-cleaner", Icon: assets.IconPNG, ToolTip: "QuickCleaner"}
	if runtime.GOOS == "linux" {
		opts.Menu = r.trayMenu()
	}
	tray, err := mygo.NewTray(opts)
	if err != nil {
		r.failure(err)
		return
	}
	r.tray = tray
	tray.OnClick(r.togglePopover)
	tray.OnRightClick(func() { r.trayMenu().Popup(nil) })
}

func (r *Root) showMain() {
	if r.popover != nil {
		r.popover.Close()
	}
	if r.window != nil && !r.window.IsDestroyed() {
		mygo.App.Show()
		if r.window.IsMinimized() {
			r.window.Restore()
		}
		r.window.Show()
		r.window.Focus()
	}
}

func (r *Root) togglePopover() {
	if r.closed || r.tray == nil {
		return
	}
	if r.popover != nil {
		r.popover.Close()
		return
	}
	controller := view.NewTrayOverviewController()
	ctx, cancel := context.WithCancel(r.ctx)
	r.trayController = controller
	r.trayCancel = cancel
	controller.SetLeftovers(r.trayLeftoverInfos())
	snapshot := view.TrayPopover{Language: r.settings.Language}
	var window *mygo.Window
	window = mygo.NewFlyout(mygo.FlyoutOptions{Tray: r.tray, Placement: mygo.PlacementBottom, Width: 360, Height: 560, Focusable: true, Shadow: true, Content: ui.View(func(c *ui.Context) {
		snapshot.Language = r.settings.Language
		snapshot.Render(c, view.TrayPopoverActions{Dismiss: func() { window.Close() }, Emit: func(a view.OverviewAction) {
			if a.DismissesPopover() {
				window.Close()
			}
			controller.Handle(a, r.routeOverview)
		}})
	})})
	r.popover = window
	window.OnClosed(func() {
		cancel()
		controller.Close()
		if r.popover == window {
			r.popover = nil
			r.trayController = nil
			r.trayCancel = nil
		}
	})
	go func() {
		for {
			select {
			case <-ctx.Done():
				return
			case <-controller.Changes():
				state := controller.Snapshot()
				r.publish(func() {
					if r.popover != window || ctx.Err() != nil {
						return
					}
					snapshot.Snapshot = state
					window.Invalidate()
				})
			}
		}
	}()
	controller.Start()
}

func (r *Root) trayLeftoverInfos() []view.TrayLeftover {
	out := make([]view.TrayLeftover, 0, len(r.trashLeftovers))
	for _, l := range r.trashLeftovers {
		var bytes uint64
		entries := make([]view.TrayLeftoverItem, 0, len(l.result.Items))
		for i, item := range l.result.Items {
			bytes += item.Size()
			manual := item.Kind.Type == core.ResidualSystemExtension
			name := filepath.Base(item.Kind.Path)
			label := item.DisplayLabelLang(r.settings.Language)
			if item.Kind.Path == "" || manual {
				name, label = label, ""
			}
			entries = append(entries, view.TrayLeftoverItem{
				Name:     name,
				Path:     item.Kind.Path,
				Label:    item.DisplayLabelLang(r.settings.Language),
				Size:     core.FmtSize(item.Size()),
				Bytes:    item.Size(),
				Selected: l.selected[i] && !manual,
				Manual:   manual,
				Confirm:  item.Confidence == core.Possible || item.RequiresConfirmation,
			})
		}
		out = append(out, view.TrayLeftover{ID: l.id, Name: l.app.Name, Items: len(l.result.Items), Size: core.FmtSize(bytes), Scanning: l.pending, Cleaning: l.cleaning, Icon: l.icon, Entries: entries})
	}
	return out
}

// markTrashSeen atomically claims an uninstall notification across the UI and watcher.
func (r *Root) markTrashSeen(id string) bool {
	r.trashSeenMu.Lock()
	defer r.trashSeenMu.Unlock()
	id = strings.ToLower(id)
	if r.trashSeen[id] {
		return false
	}
	r.trashSeen[id] = true
	return true
}

func (r *Root) dropLeftoverRow(id string) {
	for i, l := range r.trashLeftovers {
		if l.id == id {
			r.trashLeftovers = append(r.trashLeftovers[:i], r.trashLeftovers[i+1:]...)
			return
		}
	}
}

// dismissLeftoverAlert closes and clears the floating alert panel safely.
func (r *Root) dismissLeftoverAlert() {
	if r.leftoverAlert != nil {
		w := r.leftoverAlert
		r.leftoverAlert = nil
		w.Close()
	}
}

func (r *Root) refreshTrayLeftovers() {
	infos := r.trayLeftoverInfos()
	if r.trayController != nil {
		r.trayController.SetLeftovers(infos)
	}
	if r.leftoverAlert != nil {
		r.leftoverAlertSnapshot.Leftovers = infos
		// The card grows and shrinks with the rows so the footer stays with
		// the content instead of a fixed-height window clipping it.
		r.leftoverAlert.SetContentSize(leftoverAlertWidth, leftoverAlertHeight(infos))
		r.leftoverAlert.Invalidate()
		// A review that emptied the list makes the alert pointless chrome.
		if len(infos) == 0 {
			r.dismissLeftoverAlert()
		}
	}
}

const (
	leftoverAlertWidth     = 420
	leftoverAlertMinH      = 180
	leftoverAlertMaxH      = 660
	leftoverAlertChrome    = 60 // top draggable header + window borders
	leftoverAlertAppHead   = 96 // icon row (44) + selection summary bar (36) + gaps
	leftoverAlertGroup     = 28 // group heading line
	leftoverAlertItemRow   = 44 // name + path row
	leftoverAlertManualRow = 28 // manual awareness row
	leftoverAlertFooter    = 64 // bottom action bar (Skip + Clean)
	leftoverAlertTipRow    = 36 // privilege explanation row
	leftoverAlertPending   = 56 // scanning placeholder height
)

// leftoverAlertHeight sizes the window to its cards; the scroll view inside
// caps the growth.
func leftoverAlertHeight(infos []view.TrayLeftover) int {
	h := leftoverAlertChrome
	for _, l := range infos {
		if l.Scanning || len(l.Entries) == 0 {
			h += leftoverAlertPending
			continue
		}
		groups := 0
		rows, manual := 0, 0
		hadCertain, hadConfirm := false, false
		for _, it := range l.Entries {
			if it.Manual {
				manual++
				continue
			}
			rows++
			if it.Confirm {
				hadConfirm = true
			} else {
				hadCertain = true
			}
		}
		if hadCertain {
			groups++
		}
		if hadConfirm {
			groups++
		}
		extra := 0
		if hadConfirm {
			extra += leftoverAlertTipRow
		}
		h += leftoverAlertAppHead + groups*leftoverAlertGroup + rows*leftoverAlertItemRow + manual*leftoverAlertManualRow + leftoverAlertFooter + extra
	}
	return min(max(h, leftoverAlertMinH), leftoverAlertMaxH)
}

// surfaceLeftoverAlert shows the floating detection panel — a window of its
// own like CleanMyMac's leftover card, not the tray flyout. It opens once per
// detection; closing it is the user's dismissal, and the list it draws lives
// on Root so rows also reach the tray popover.
func (r *Root) surfaceLeftoverAlert() {
	if r.closed || r.leftoverAlert != nil {
		return
	}
	if r.tray == nil && r.window == nil {
		return
	}
	// Suppress floating alert if the main window's residual review modal is already open
	// or the main window is currently active and visible in the foreground.
	if r.apps.ResidualsOpen {
		log.Printf("残留检测：主程序已打开残留复核对话框，抑制外部悬浮小窗")
		return
	}
	if r.window != nil && !r.windowHidden && !r.window.IsDestroyed() {
		log.Printf("残留检测：主窗口在前台活跃，抑制外部悬浮小窗")
		return
	}
	log.Printf("残留检测：弹出残留提示面板")
	infos := r.trayLeftoverInfos()
	snapshot := view.LeftoverAlert{Language: r.settings.Language, Leftovers: infos}
	r.leftoverAlertSnapshot = snapshot
	// Top-right of the menu-bar display, where CleanMyMac puts its card.
	// X/Y both zero would center the window, so compute them only with a
	// real work area; height follows the cards and refresh resizes as they
	// change.
	opts := mygo.WindowOptions{
		Title: "QuickCleaner", Width: leftoverAlertWidth, Height: leftoverAlertHeight(infos), UseContentSize: true,
		Panel:             true,
		Frameless:         true,
		Transparent:       true,
		DisableResize:     true,
		DisableMinimize:   true,
		DisableMaximize:   true,
		DisableFullScreen: true,
	}
	if area := mygo.Screen.PrimaryDisplay().WorkArea; area.Width > 0 {
		opts.X = area.X + area.Width - leftoverAlertWidth - 12
		opts.Y = area.Y + 10
	}
	var window *mygo.Window
	opts.Content = ui.View(func(c *ui.Context) {
		r.leftoverAlertSnapshot.Language = r.settings.Language
		r.leftoverAlertSnapshot.Render(c, view.LeftoverAlertActions{
			Review:      func(id string) { r.routeOverview(view.OverviewAction{Kind: view.OverviewReviewLeftovers, Path: id}) },
			Dismiss:     func() { window.Close() },
			Clean:       r.requestLeftoverClean,
			Toggle:      r.toggleLeftoverItem,
			SelectGroup: func(id string, confirm bool) { r.selectLeftoverGroup(id, confirm) },
			SelectAll:   r.selectAllLeftovers,
			SelectSafe:  r.selectSafeLeftovers,
		})
	})
	window = mygo.NewWindow(opts)
	r.leftoverAlert = window
	window.OnClosed(func() {
		if r.leftoverAlert == window {
			r.leftoverAlert = nil
		}
	})
}

// toggleLeftoverItem flips one picklist row in the alert card. The picklist
// shares the review panel's rule: manual items (system extensions) never join
// a clean, and a row being cleaned is frozen.
func (r *Root) toggleLeftoverItem(id string, index int) {
	for i := range r.trashLeftovers {
		l := &r.trashLeftovers[i]
		if l.id != id || l.cleaning || index < 0 || index >= len(l.result.Items) {
			continue
		}
		if l.result.Items[index].Kind.Type == core.ResidualSystemExtension {
			return
		}
		if l.selected == nil {
			l.selected = map[int]bool{}
		}
		l.selected[index] = !l.selected[index]
		r.refreshTrayLeftovers()
		return
	}
}

// selectLeftoverGroup picks every selectable item in one confidence group —
// the certain picks or the needs-confirmation ones, never both, and never a
// manual item. It mirrors the review panel's Select All per section.
func (r *Root) selectLeftoverGroup(id string, confirm bool) {
	for i := range r.trashLeftovers {
		l := &r.trashLeftovers[i]
		if l.id != id || l.cleaning {
			continue
		}
		if l.selected == nil {
			l.selected = map[int]bool{}
		}
		for j, item := range l.result.Items {
			if item.Kind.Type == core.ResidualSystemExtension {
				continue
			}
			if (item.Confidence == core.Possible || item.RequiresConfirmation) == confirm {
				l.selected[j] = true
			}
		}
		r.refreshTrayLeftovers()
		return
	}
}

// selectAllLeftovers picks every selectable item (both safe and needs-confirmation).
func (r *Root) selectAllLeftovers(id string) {
	for i := range r.trashLeftovers {
		l := &r.trashLeftovers[i]
		if l.id != id || l.cleaning {
			continue
		}
		if l.selected == nil {
			l.selected = map[int]bool{}
		}
		for j, item := range l.result.Items {
			if item.Kind.Type == core.ResidualSystemExtension {
				continue
			}
			l.selected[j] = true
		}
		r.refreshTrayLeftovers()
		return
	}
}

// selectSafeLeftovers only selects routine/safe items and deselects confirmation-required items.
func (r *Root) selectSafeLeftovers(id string) {
	for i := range r.trashLeftovers {
		l := &r.trashLeftovers[i]
		if l.id != id || l.cleaning {
			continue
		}
		if l.selected == nil {
			l.selected = map[int]bool{}
		}
		for j, item := range l.result.Items {
			if item.Kind.Type == core.ResidualSystemExtension {
				continue
			}
			confirm := item.Confidence == core.Possible || item.RequiresConfirmation
			l.selected[j] = !confirm
		}
		r.refreshTrayLeftovers()
		return
	}
}

// requestLeftoverClean runs the same gated clean the review panel performs —
// frozen items, live authority recheck, then a verification pass — but the
// leftover row stands in for the dialog: it shows progress and drops itself
// on success instead of reopening.
func (r *Root) requestLeftoverClean(id string) {
	if r.busy() {
		return
	}
	idx := -1
	for i := range r.trashLeftovers {
		if r.trashLeftovers[i].id == id {
			idx = i
			break
		}
	}
	if idx < 0 || r.trashLeftovers[idx].pending || r.trashLeftovers[idx].cleaning {
		return
	}
	leftover := &r.trashLeftovers[idx]
	var items, unselected []core.ResidualItem
	var labels []string
	var totalBytes uint64
	for i, item := range leftover.result.Items {
		if item.Kind.Type == core.ResidualSystemExtension || !leftover.selected[i] {
			unselected = append(unselected, item)
			continue
		}
		items = append(items, item)
		labels = append(labels, item.DisplayLabelLang(r.settings.Language))
		totalBytes += item.Size()
	}
	if len(items) == 0 {
		return
	}
	app, scope := leftover.app, leftover.result.Scope
	leftover.cleaning = true
	r.refreshTrayLeftovers()

	p := core.NewCleanProgress(uint64(len(items)), totalBytes)
	r.start(true, p, func(ctx context.Context) func() {
		allowed := false
		if scope == core.ResidualScopeOrphanLeftovers {
			allowed = orphanAbsent(ctx, items)
		} else {
			allowed = r.residualAuthority(ctx, app, items)
		}
		if !allowed || ctx.Err() != nil || p.Cancelled() {
			return func() {
				r.clearLeftoverCleaning(id)
				if ctx.Err() == nil && !p.Cancelled() {
					r.apps.Notice = core.NewText("残留清理授权无法验证；未清理任何残留。", "Residual cleanup authority could not be verified; no residuals were cleaned.")
					r.record("clean_residuals", labels, 0, uint(len(items)), 0, 0)
				}
			}
		}
		report := cleanResiduals(ctx, slices.Clone(items), p)
		// Verification is limited to the candidates just cleaned, never a
		// fresh discovery.
		remaining, err := verifyResiduals(ctx, items)
		return func() {
			r.finishCleanup("clean_residuals", labels, report, p)
			if p.Cancelled() {
				r.clearLeftoverCleaning(id)
				return
			}
			if err != nil {
				r.clearLeftoverCleaning(id)
				r.apps.Notice = core.SameText(err.Error())
				return
			}
			// Only verified cleaned candidates leave the frozen inventory; unselected
			// and manual entries retain their evidence for a later review.
			for i := range r.trashLeftovers {
				l := &r.trashLeftovers[i]
				if l.id != id {
					continue
				}
				l.cleaning = false
				l.result.Items = append(unselected, remaining...)
				l.result.TotalFileSize = 0
				for _, item := range l.result.Items {
					l.result.TotalFileSize += item.Size()
				}
				l.selected = map[int]bool{}
				if len(l.result.Items) == 0 {
					r.dropLeftoverRow(id)
				}
				break
			}
			r.refreshTrayLeftovers()
		}
	})
}

func (r *Root) clearLeftoverCleaning(id string) {
	for i := range r.trashLeftovers {
		if r.trashLeftovers[i].id == id {
			r.trashLeftovers[i].cleaning = false
		}
	}
	r.refreshTrayLeftovers()
}

func (r *Root) routeOverview(a view.OverviewAction) {
	switch a.Kind {
	case view.OverviewQuit:
		mygo.App.Quit()
	case view.OverviewOpenMain:
		r.showMain()
	case view.OverviewScanJunk:
		r.showMain()
		r.Navigate(view.Junk)
		r.scanJunk()
	case view.OverviewOpenView:
		r.showMain()
		r.Navigate(a.View)
		// Sidebar starts Junk, Apps, Disk and Search on their first quiet visit
		// through Navigate. Declutter has no visit hook; gate it on its own flag
		// so a menu pick reuses the finished scan instead of starting over.
		if a.View == view.Declutter && !r.declutter.Scanned {
			r.scanDeclutter()
		}
	case view.OverviewReviewLeftovers:
		r.showMain()
		for i, l := range r.trashLeftovers {
			if l.id != a.Path || l.pending {
				continue
			}
			// Open the review before Navigate: a first visit queues the
			// apps scan, which would make busy() skip the panel entirely.
			if !r.busy() && !r.apps.ResidualsOpen {
				r.trashLeftovers = append(r.trashLeftovers[:i], r.trashLeftovers[i+1:]...)
				r.refreshTrayLeftovers()
				r.showResiduals(l.result, &l.app)
			}
			r.Navigate(view.Apps)
			break
		}
		r.changed()
	case view.OverviewTopMemory:
		r.showMain()
		r.status.Sort = view.StatusSort{Key: view.StatusSortMemory}
		r.Navigate(view.Status)
	case view.OverviewAnalyzeDisk:
		r.showMain()
		r.active = view.Disk
		matched := false
		for _, v := range r.disk.Volumes {
			if v.ID.Display() == a.Path {
				matched = true
				r.diskActions().SwitchVolume(v.ID)
				break
			}
		}
		if !matched {
			r.scanDisk()
			if !r.disk.Scanning {
				r.message("所选卷已不可用。", "The selected volume is no longer available.")
			}
		}
		r.changed()
	}
}
