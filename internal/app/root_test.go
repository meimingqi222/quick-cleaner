package app

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func testRoot(t *testing.T) (*Root, chan func()) {
	t.Helper()
	queue := make(chan func(), 32)
	r := NewRoot(core.DefaultSettings(), func(fn func()) { queue <- fn }, func() {})
	r.save = func(core.Settings) {}
	r.record = func(string, []string, uint, uint, uint, uint64) {}
	r.runningOwnerBusy = func([]core.ScanTarget) map[string]core.Busy { return map[string]core.Busy{} }
	r.findRunningApps = func([]core.ScanItem) []core.RunningApp { return nil }
	r.closeApps = func([]core.RunningApp, func() bool) []core.Text {
		t.Error("test closed real applications")
		return nil
	}
	t.Cleanup(r.Close)
	return r, queue
}
func dispatchNext(t *testing.T, q chan func()) {
	t.Helper()
	select {
	case fn := <-q:
		fn()
	case <-time.After(5 * time.Second):
		t.Fatal("worker did not publish")
	}
}

func TestExcludeDropsDescendantsAndDemotesParents(t *testing.T) {
	r, _ := testRoot(t)
	parent := t.TempDir()
	child := parent + "/child"
	if err := os.Mkdir(child, 0o755); err != nil {
		t.Fatal(err)
	}
	other := t.TempDir()
	r.junk.Categories = []core.CategorySummary{{Category: core.UserCache, TotalSize: 30, Items: []core.ScanItem{
		{Path: parent, Size: 10, Recommended: true},
		{Path: child, Size: 20, Recommended: true},
		{Path: other, Size: 5, Recommended: true},
	}}}
	r.junk.Selected = map[string]bool{parent: true, child: true, other: true}
	if n := r.dropExcludedJunk(child); n != 1 || len(r.junk.Categories[0].Items) != 2 {
		t.Fatalf("removed=%d items=%d", n, len(r.junk.Categories[0].Items))
	}
	if r.junk.Categories[0].Items[0].Recommended || r.junk.Selected[parent] || r.junk.Categories[0].TotalSize != 15 {
		t.Fatalf("parent not demoted: %+v selected=%v", r.junk.Categories[0], r.junk.Selected)
	}
	if !r.junk.Categories[0].Items[1].Recommended || !r.junk.Selected[other] {
		t.Fatal("unrelated item changed")
	}
	if !pathUnder(child, parent) || pathUnder(parent, child) || pathUnder(other, parent) {
		t.Fatal("path containment inverted")
	}
}

func TestMainThreadPublicationAndSerialWork(t *testing.T) {
	r, q := testRoot(t)
	finished := make(chan struct{})
	if !r.start(false, nil, func(context.Context) func() { close(finished); return func() { r.dashboard.AppCount = 42 } }) {
		t.Fatal("work refused")
	}
	<-finished
	if r.dashboard.AppCount != 0 {
		t.Fatal("worker mutated presentation state before dispatch")
	}
	if r.start(false, nil, func(context.Context) func() { t.Error("overlapping work launched"); return nil }) {
		t.Fatal("overlapping work accepted")
	}
	dispatchNext(t, q)
	if r.dashboard.AppCount != 42 || r.job != nil {
		t.Fatal("completion not published or work not released")
	}
}

func TestCancellationAndClosedRootRejectLatePublication(t *testing.T) {
	r, q := testRoot(t)
	stopped := make(chan struct{})
	p := core.NewCleanProgress(0, 0)
	r.start(true, p, func(ctx context.Context) func() {
		<-ctx.Done()
		close(stopped)
		return func() { t.Error("closed root accepted stale completion") }
	})
	r.Cancel()
	<-stopped
	if !p.Cancelled() {
		t.Fatal("cleaner cancellation was not propagated")
	}
	r.Close()
	dispatchNext(t, q)
}

func TestConfirmationIsExplicitStableAndSingleUse(t *testing.T) {
	r, _ := testRoot(t)
	count := 0
	r.ask("Remove?", "frozen target", func() { count++ })
	id := r.confirmation.ID
	if count != 0 || !r.confirming {
		t.Fatal("request executed before explicit acceptance")
	}
	r.resolveConfirmation("stale", true)
	if count != 0 {
		t.Fatal("stale acceptance executed")
	}
	r.resolveConfirmation(id, false)
	if count != 0 || r.confirming {
		t.Fatal("cancel executed action or left modal active")
	}
	r.ask("Remove?", "new target", func() { count++ })
	next := r.confirmation.ID
	r.resolveConfirmation(id, true)
	if count != 0 {
		t.Fatal("old modal accepted new request")
	}
	r.resolveConfirmation(next, true)
	r.resolveConfirmation(next, true)
	if count != 1 {
		t.Fatalf("action executed %d times", count)
	}
}

func TestCleanupCancellationDoesNotDeleteAndReplacementIsRetained(t *testing.T) {
	r, q := testRoot(t)
	dir := t.TempDir()
	path := filepath.Join(dir, "selected.txt")
	if err := os.WriteFile(path, []byte("original"), 0600); err != nil {
		t.Fatal(err)
	}
	frozen := core.CaptureArbitraryTarget(path)
	if frozen.Identity == nil {
		t.Fatal("fixture identity missing")
	}
	r.requestArbitraryClean("test", []core.ArbitraryTarget{frozen}, 0, "", func(core.CleanReport) {})
	if _, err := os.Stat(path); err != nil {
		t.Fatal("request mutated path before confirmation", err)
	}
	r.resolveConfirmation(r.confirmation.ID, false)
	if data, err := os.ReadFile(path); err != nil || string(data) != "original" {
		t.Fatal("cancel changed target")
	}
	if err := os.Rename(path, path+".saved"); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte("replacement"), 0600); err != nil {
		t.Fatal(err)
	}
	var successes uint
	r.record = func(_ string, _ []string, ok, _, _ uint, _ uint64) { successes = ok }
	r.requestArbitraryClean("test", []core.ArbitraryTarget{frozen}, 0, "", func(core.CleanReport) {})
	r.resolveConfirmation(r.confirmation.ID, true)
	dispatchNext(t, q)
	if data, err := os.ReadFile(path); err != nil || string(data) != "replacement" {
		t.Fatal("replaced target was removed", err)
	}
	if successes != 0 {
		t.Fatal("unverified target reported as successfully removed")
	}
}

type fixtureDiskOwner struct {
	tree *core.SizeTree
	err  error
}

func (f fixtureDiskOwner) Load(context.Context, core.VolumeID) (*core.ScanResult, error) {
	if f.err != nil {
		return nil, f.err
	}
	return &core.ScanResult{Tree: f.tree}, nil
}
func (fixtureDiskOwner) Close() {}

func TestDiskErrorsAreNotEmptySuccessfulIndexes(t *testing.T) {
	r, q := testRoot(t)
	r.disk.Volume = core.VolumeFromMountPoint(t.TempDir())
	r.diskOwner = fixtureDiskOwner{err: errors.New("inventory denied")}
	r.scanDisk()
	dispatchNext(t, q)
	if r.disk.Loaded || r.search.Ready || r.disk.Error != "inventory denied" {
		t.Fatal("inventory error became a successful empty index")
	}
}

func TestDiskProtectionCoverageAndInheritedExclusion(t *testing.T) {
	r, _ := testRoot(t)
	dir := t.TempDir()
	for _, name := range []string{"keep", "remove"} {
		if err := os.Mkdir(filepath.Join(dir, name), 0700); err != nil {
			t.Fatal(err)
		}
	}
	r.tree = core.SizeTreeFromSnapshot(core.VolumeFromMountPoint(dir), []core.TreeSnapshotEntry{{Path: dir, IsDir: true}, {Path: filepath.Join(dir, "keep"), IsDir: true}, {Path: filepath.Join(dir, "remove"), IsDir: true}})
	r.disk.Loaded = true
	r.rebuildDisk()
	r.diskSelected.Toggle(dir, 100)
	r.diskSelected.Toggle(filepath.Join(dir, "keep"), 40)
	paths := r.diskSelected.ResolveTargets()
	if len(paths) != 1 || paths[0] != filepath.Join(dir, "remove") {
		t.Fatalf("exclusion not respected: %v", paths)
	}
	r.tree.SetBlockedSubtrees([]string{filepath.Join(dir, "keep")})
	r.rebuildDisk()
	for _, row := range r.disk.Rows {
		if row.Path == filepath.Join(dir, "keep") && (row.CanDelete || row.CanSelect) {
			t.Fatal("unknown index coverage authorized deletion")
		}
	}
	r.requestDiskDelete([]string{dir})
	if r.confirming {
		t.Fatal("index root became a deletion target")
	}
}

// Splitting selected folders around exclusions walks the filesystem; doing it
// on the click froze the window before the confirmation could appear.
func TestRebuildDiskLargestFilesOmitsProtected(t *testing.T) {
	r, _ := testRoot(t)
	dir := t.TempDir()
	movie := filepath.Join(dir, "movie.mp4")
	page := filepath.Join(dir, "pagefile.sys")
	r.tree = core.SizeTreeFromSnapshot(core.VolumeFromMountPoint(dir), []core.TreeSnapshotEntry{
		{Path: dir, IsDir: true},
		{Path: movie, Size: 100},
		{Path: page, Size: 9999},
	})
	r.disk.Loaded = true
	r.disk.Tab = view.DiskFiles
	r.rebuildDisk()
	if len(r.disk.Rows) != 1 || r.disk.Rows[0].Path != movie {
		t.Fatalf("largest-files rows=%+v, want only %s", r.disk.Rows, movie)
	}
}

func TestSelectedDiskDeleteResolvesTargetsOnWorkerBeforeConfirming(t *testing.T) {
	r, q := testRoot(t)
	dir := t.TempDir()
	target := filepath.Join(dir, "target")
	if err := os.Mkdir(target, 0700); err != nil {
		t.Fatal(err)
	}
	r.tree = core.SizeTreeFromSnapshot(core.VolumeFromMountPoint(dir), []core.TreeSnapshotEntry{{Path: dir, IsDir: true}, {Path: target, IsDir: true}})
	r.disk.Loaded = true
	r.rebuildDisk()
	r.diskSelected.Toggle(target, 10)
	r.rebuildDisk()
	r.requestSelectedDiskDelete()
	if r.confirming {
		t.Fatal("confirmation appeared before targets were resolved")
	}
	dispatchNext(t, q)
	if !r.confirming {
		t.Fatal("resolved targets never reached confirmation")
	}
}

// A select-all in a churning folder (Temp) always includes rows whose files
// vanished after the scan. One vanished row used to fail identity capture and
// refuse the whole batch; it has nothing to delete, so only the rows still on
// disk need a frozen identity.
func TestSelectedDiskDeleteSkipsRowsAlreadyGoneFromDisk(t *testing.T) {
	for _, refused := range []bool{false, true} {
		t.Run(fmt.Sprintf("refused=%v", refused), func(t *testing.T) {
			r, q := testRoot(t)
			dir := t.TempDir()
			kept, gone := filepath.Join(dir, "kept.tmp"), filepath.Join(dir, "gone.tmp")
			for _, p := range []string{kept, gone} {
				if err := os.WriteFile(p, []byte("x"), 0600); err != nil {
					t.Fatal(err)
				}
			}
			const keptSize, goneSize = 4 << 10, 1 << 20
			r.tree = core.SizeTreeFromSnapshot(core.VolumeFromMountPoint(dir), []core.TreeSnapshotEntry{{Path: dir, IsDir: true}, {Path: kept, Size: keptSize}, {Path: gone, Size: goneSize}})
			r.disk.Loaded = true
			r.rebuildDisk()
			r.diskSelected.SetMany([]string{kept, gone}, []uint64{keptSize, goneSize}, true)
			r.rebuildDisk()
			if err := os.Remove(gone); err != nil {
				t.Fatal(err)
			}
			r.requestSelectedDiskDelete()
			dispatchNext(t, q)
			if !r.confirming {
				t.Fatalf("a vanished row refused the batch: %q", r.dashboard.StatusText)
			}
			// The estimate counts only what will be deleted, not the vanished row.
			if detail := r.confirmation.Detail.Get(r.settings.Language); !strings.Contains(detail, core.FmtSize(keptSize)) || strings.Contains(detail, core.FmtSize(keptSize+goneSize)) {
				t.Fatalf("confirmation size counts vanished rows: %s", detail)
			}
			// This test owns the app flow, not host-wide occupancy permissions.
			// Linux CI cannot inspect every /proc PID and correctly fails closed.
			called := false
			r.cleanArbitrary = func(items []core.ArbitraryTarget, disposal core.Disposal, p *core.CleanProgress) core.CleanReport {
				called = true
				if len(items) != 1 || items[0].Path != kept || items[0].Identity == nil || !items[0].Identity.Recheck(kept) || disposal != core.Permanent {
					t.Errorf("unexpected authorized cleanup: items=%+v disposal=%s", items, disposal)
					return core.CleanReport{}
				}
				var report core.CleanReport
				if refused {
					report.Record(kept, core.CleanFailed)
				} else {
					report.Record(kept, core.CleanPath(kept, p))
				}
				return report
			}
			r.resolveConfirmation(r.confirmation.ID, true)
			for r.job != nil {
				dispatchNext(t, q)
			}
			if !called {
				t.Fatal("confirmed targets never reached the cleaner")
			}
			if _, err := os.Lstat(kept); refused {
				if err != nil {
					t.Fatalf("refused file did not survive: %v", err)
				}
			} else if !os.IsNotExist(err) {
				t.Fatalf("surviving selected row was not deleted: %v", err)
			}
			if _, ok := r.tree.FindNodeByPath(kept); ok != refused {
				t.Fatalf("surviving row in index = %v, want %v", ok, refused)
			}
			if _, ok := r.tree.FindNodeByPath(gone); ok {
				t.Fatal("already vanished row stayed in the index")
			}
		})
	}
}

func TestSearchDebounceLatestGenerationAndClear(t *testing.T) {
	r, q := testRoot(t)
	dir := t.TempDir()
	r.tree = core.SizeTreeFromSnapshot(core.VolumeFromMountPoint(dir), []core.TreeSnapshotEntry{{Path: dir, IsDir: true}, {Path: filepath.Join(dir, "alpha.txt"), Size: 2}, {Path: filepath.Join(dir, "beta.txt"), Size: 1}})
	r.setSearchQuery("alpha")
	r.setSearchQuery("beta")
	dispatchNext(t, q)
	if len(r.search.Results) != 1 || r.search.Results[0].Name != "beta.txt" {
		t.Fatalf("stale search published: %v", r.search.Results)
	}
	r.setSearchQuery("")
	if r.search.Searching || len(r.search.Results) != 0 {
		t.Fatal("clear retained results or searching state")
	}
}

func TestDeclutterSelectionUsesFullSnapshotNotFilteredRows(t *testing.T) {
	r, _ := testRoot(t)
	r.declutter.LargeFiles = []core.LargeFileItem{{Path: "visible", Selected: true}, {Path: "filtered-out", Selected: true}}
	r.declutter.FilteredLargeIndices = []int{0}
	if len(r.declutterPaths(view.DeclutterLargeFiles)) != 2 {
		t.Fatal("deletion request silently dropped hidden selected item")
	}
	r.declutter.Scanned = true
	r.requestDeclutterClean(view.DeclutterLargeFiles)
	if r.confirming {
		t.Fatal("missing scan identity acquired deletion authority")
	}
}

func TestDeveloperLayoutAndProtectedAssetsHaveNoSelectionAuthority(t *testing.T) {
	r, _ := testRoot(t)
	r.dev.Assets = []core.DevAssetItem{{ID: "layout", Source: core.AssetLayout, Kind: core.DevAssetKind{Type: core.DevNodePackage, Manager: "npm"}}, {ID: "base", Source: core.AssetTool, Kind: core.DevAssetKind{Type: core.DevCondaEnv, IsBase: true}}}
	a := r.devActions()
	a.SelectAll()
	a.ToggleSelected("layout")
	a.ToggleSelected("base")
	if len(r.dev.Selected) != 0 {
		t.Fatal("display-only/protected assets became selected")
	}
	a.RequestRemoveSelected()
	if r.confirming {
		t.Fatal("display-only asset removal became confirmable")
	}
}

func TestProductRendersExistingPagesAndConfirmation(t *testing.T) {
	r, _ := testRoot(t)
	for v := view.Overview; v <= view.Search; v++ {
		r.active = v
		if frame := ui.Render(r.Render, 1280, 880, 1); frame.Bounds().Dx() != 1280 {
			t.Fatalf("view %d did not render", v)
		}
	}
	r.ask("Confirm frozen operation", "No mutation before acceptance", func() { t.Error("render accepted confirmation") })
	ui.Render(r.Render, 1280, 880, 1)
}

func TestMonitoringHistoriesFollowSelectionAndNativeReadings(t *testing.T) {
	r, _ := testRoot(t)
	usage := float32(75)
	s := core.StatusSnapshot{Processes: []core.ProcInfo{{PID: 17, CPU: 11, MemBytes: 100}, {PID: 29, CPU: 32, MemBytes: 900}}, GPUs: []core.GpuReading{{ID: "idle"}, {ID: "busy", Utilization: &usage}}}
	r.updateStatus(s)
	r.statusActions().SelectProcess(29)
	for i := 0; i < 30; i++ {
		s.CPUUsage = float32(i)
		s.Processes[1].CPU = float32(100 + i)
		s.Processes[1].MemBytes = uint64(1000 + i)
		r.updateStatus(s)
	}
	if len(r.status.CPUHistory) != 24 || r.status.CPUHistory[0] != 6 || r.status.CPUHistory[23] != 29 {
		t.Fatal("CPU history did not retain the latest 24 samples", r.status.CPUHistory)
	}
	if r.status.SelectedGPU != "busy" || len(r.status.GPUHistory["busy"]) != 24 || len(r.status.GPUHistory["idle"]) != 0 {
		t.Fatal("unknown GPU usage became zero history, or busiest GPU was not selected")
	}
	if len(r.status.SelectedCPUHistory) != 24 || r.status.SelectedCPUHistory[0] != 106 || r.status.SelectedMemHistory[23] != 1029 {
		t.Fatal("selected-process history is missing or mixed with another process")
	}
	r.statusActions().SelectProcess(17)
	if len(r.status.SelectedCPUHistory) != 1 || r.status.SelectedCPUHistory[0] != 11 || r.status.SelectedMemHistory[0] != 100 {
		t.Fatal("switching processes retained old history")
	}
	r.statusActions().SelectProcess(17)
	if r.status.SelectedPID != nil || len(r.status.SelectedCPUHistory) != 0 || len(r.status.SelectedMemHistory) != 0 {
		t.Fatal("clicking the selected process did not close and clear its detail")
	}
	r.statusActions().SelectProcess(29)
	s.Processes = s.Processes[:1]
	s.GPUs = s.GPUs[:1]
	r.updateStatus(s)
	if r.status.SelectedPID != nil || len(r.status.SelectedCPUHistory) != 0 || r.status.SelectedGPU != "idle" {
		t.Fatal("vanished process/GPU retained stale selection")
	}
}

// 残留清理是用户勾选后主动发起的动作：选中即确认。Rust 也不再弹二次确认框，
// 额外一层确认只是拖慢这个已经明确的动作。
func TestResidualCleanStartsWithoutConfirmation(t *testing.T) {
	r, _ := testRoot(t)
	r.residual = core.ResidualScanResult{Items: []core.ResidualItem{
		core.CertainResidual(core.ResidualKind{Type: core.ResidualFile, Path: filepath.Join(t.TempDir(), "leftover"), Bytes: 4}, core.ResidualOther),
	}}
	r.apps.Residuals = []view.AppsPageResidual{{Selected: true}}
	r.requestResidualClean()
	if r.confirming {
		t.Fatal("residual clean still opens a confirmation dialog")
	}
	if !r.apps.Cleaning {
		t.Fatal("residual clean did not start")
	}
}

// Rust 清理后不重开残留面板（residual.result = None）：删不掉的多半一直删不掉，
// 重开就变成「点一次弹一次」。状态栏/提示报失败计数，残留入口可再扫。
func TestResidualCleanDoesNotReopenTheReviewPanel(t *testing.T) {
	r, q := testRoot(t)
	gone := filepath.Join(t.TempDir(), "already-gone")
	item := core.CertainResidual(core.ResidualKind{Type: core.ResidualFile, Path: gone, Bytes: 1024}, core.ResidualCacheDir)
	app := core.InstalledApp{ID: "example", Name: "Example"}
	r.showResiduals(core.ResidualScanResult{AppName: app.Name, Items: []core.ResidualItem{item}}, &app)
	if !r.apps.ResidualsOpen {
		t.Fatal("fixture did not open the residual panel")
	}
	r.residualAuthority = func(context.Context, core.InstalledApp, []core.ResidualItem) bool { return true }
	r.requestResidualClean()
	if r.apps.ResidualsOpen {
		t.Fatal("review panel remained open while cleaning was in progress")
	}
	if r.job == nil || r.job.progress == nil || r.job.progress.TotalFiles.Load() != 1 || r.job.progress.TotalBytes.Load() != 1024 {
		t.Fatalf("expected progress initialized with 1 file and 1024 bytes, got %+v", r.job)
	}
	dispatchNext(t, q)
	if r.apps.ResidualsOpen {
		t.Fatal("cleaning reopened the residual review panel")
	}
}

// 授权未通过时，原样还原面板、选项与说明，以便用户查看失败原因。
func TestResidualCleanRestoresReviewOnRefusal(t *testing.T) {
	r, q := testRoot(t)
	gone := filepath.Join(t.TempDir(), "already-gone")
	item := core.CertainResidual(core.ResidualKind{Type: core.ResidualFile, Path: gone, Bytes: 2048}, core.ResidualCacheDir)
	app := core.InstalledApp{ID: "example", Name: "Example"}
	r.showResiduals(core.ResidualScanResult{AppName: app.Name, Items: []core.ResidualItem{item}}, &app)
	r.residualAuthority = func(context.Context, core.InstalledApp, []core.ResidualItem) bool { return false }
	r.requestResidualClean()
	if r.apps.ResidualsOpen {
		t.Fatal("review panel should close immediately on start")
	}
	dispatchNext(t, q)
	if !r.apps.ResidualsOpen {
		t.Fatal("refused cleanup did not restore the review panel")
	}
	if len(r.apps.Residuals) != 1 || !r.apps.Residuals[0].Selected {
		t.Fatal("refused cleanup did not restore residual selection")
	}
	if r.apps.Notice.Get(core.Chinese) == "" {
		t.Fatal("refused cleanup did not set notice")
	}
}

// 关闭残留面板要带走面板说明，但不能把真正的失败提示一起清掉——两者原本共用一个
// Notice 字段，这正是文案泄漏到页面正文的根因。
func TestClosingResidualReviewDropsOnlyItsOwnExplanation(t *testing.T) {
	r, _ := testRoot(t)
	app := core.InstalledApp{ID: "example", Name: "Example"}
	item := core.CertainResidual(core.ResidualKind{Type: core.ResidualFile, Path: "p"}, core.ResidualCacheDir)
	r.showResiduals(core.ResidualScanResult{AppName: app.Name, Items: []core.ResidualItem{item}}, &app)
	if r.apps.ResidualNotice.Get(core.Chinese) == "" {
		t.Fatal("panel has no explanation")
	}
	if !r.apps.ResidualsOpen {
		t.Fatal("panel did not open")
	}
	r.apps.Notice = core.NewText("授权失败", "authority failed")
	r.closeResiduals()
	if r.apps.ResidualNotice.Get(core.Chinese) != "" {
		t.Fatalf("explanation outlived the panel: %q", r.apps.ResidualNotice.Get(core.Chinese))
	}
	if r.apps.Notice.Get(core.Chinese) != "授权失败" {
		t.Fatal("closing the review cleared a real failure notice")
	}
	if r.apps.ResidualsOpen || len(r.apps.Residuals) != 0 {
		t.Fatal("review state survived the close")
	}
}

func TestNavigateToJunkTriggersFirstScan(t *testing.T) {
	r, _ := testRoot(t)
	if r.junk.Scanning || r.junk.Scanned {
		t.Fatal("fixture started with junk scanned or scanning")
	}
	r.Navigate(view.Junk)
	if !r.junk.Scanning {
		t.Fatal("navigating to junk did not trigger first scan")
	}
	r.Cancel()
}

// A row that exists but cannot be frozen still refuses the whole batch:
// skipping only applies to rows confirmed gone, never to unknown identity.
func TestCaptureDiskTargetsSeparatesGoneFromUnfreezable(t *testing.T) {
	dir := t.TempDir()
	kept, gone := filepath.Join(dir, "kept"), filepath.Join(dir, "gone")
	if err := os.WriteFile(kept, []byte("x"), 0600); err != nil {
		t.Fatal(err)
	}
	targets, vanished, failed := captureDiskTargets(context.Background(), []string{kept, gone})
	if failed != "" || len(targets) != 1 || targets[0].Path != kept || targets[0].Identity == nil || !slices.Equal(vanished, []string{gone}) {
		t.Fatalf("targets=%+v vanished=%v failed=%q", targets, vanished, failed)
	}
	unfreezable := func(path string) core.ArbitraryTarget { return core.ArbitraryTarget{Path: path} }
	targets, vanished, failed = captureDiskTargetsUsing(context.Background(), []string{gone, kept}, unfreezable)
	if failed != kept || targets != nil || vanished != nil {
		t.Fatalf("existing unfreezable path was not refused: targets=%+v vanished=%v failed=%q", targets, vanished, failed)
	}
}

// A checkbox click changes selection only. Rebuilding every row re-ran the
// protection, whitelist and coverage checks: about 50 ms per click on a
// 7400-entry folder. Selection actions refresh the existing rows instead, and
// the result must equal a full rebuild.
func TestDiskSelectionActionsRefreshRowsWithoutRebuilding(t *testing.T) {
	r, _ := testRoot(t)
	dir := t.TempDir()
	sub := filepath.Join(dir, "sub")
	entries := []core.TreeSnapshotEntry{{Path: dir, IsDir: true}, {Path: sub, IsDir: true}}
	for i := 0; i < 4; i++ {
		entries = append(entries, core.TreeSnapshotEntry{Path: filepath.Join(sub, fmt.Sprintf("f%d", i)), Size: uint64(i + 1)})
	}
	r.tree = core.SizeTreeFromSnapshot(core.VolumeFromMountPoint(dir), entries)
	r.disk.Loaded = true
	id, _ := r.tree.FindNodeByPath(sub)
	r.currentNode = id
	r.rebuildDisk()
	a := r.diskActions()
	sentinel := r.disk.Rows[3].Path
	steps := []func(){
		func() { a.ToggleSelected(r.disk.Rows[1]) },
		func() { a.SetViewSelected(true) },
		func() { a.ToggleSelected(r.disk.Rows[2]) },
		func() { a.SetViewSelected(false) },
		func() { a.ToggleSelected(r.disk.Rows[0]) },
		a.ClearSelection,
	}
	for i, step := range steps {
		r.disk.Rows[3].Protected = true // only a full rebuild would reset this
		step()
		if !r.disk.Rows[3].Protected || r.disk.Rows[3].Path != sentinel {
			t.Fatalf("step %d rebuilt every row for a selection change", i)
		}
		got := r.disk
		r.disk.Rows = slices.Clone(got.Rows)
		r.rebuildDisk()
		want := r.disk
		for j := range want.Rows {
			if got.Rows[j].Selected != want.Rows[j].Selected {
				t.Fatalf("step %d row %d selected=%v, full rebuild says %v", i, j, got.Rows[j].Selected, want.Rows[j].Selected)
			}
		}
		if got.SelectedCount != want.SelectedCount || got.SelectedSize != want.SelectedSize || got.HeaderSelected != want.HeaderSelected || got.HeaderMixed != want.HeaderMixed || got.CanRequestDelete != want.CanRequestDelete {
			t.Fatalf("step %d summary %+v, full rebuild %+v", i, []any{got.SelectedCount, got.SelectedSize, got.HeaderSelected, got.HeaderMixed, got.CanRequestDelete}, []any{want.SelectedCount, want.SelectedSize, want.HeaderSelected, want.HeaderMixed, want.CanRequestDelete})
		}
	}
}
