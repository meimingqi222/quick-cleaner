package app

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

// Pruning the photo tab resets which groups are expanded, because indices
// shift. The reset left a nil map, and the next expand click panicked.
func TestPhotoGroupExpandWorksAfterPruneAndRescan(t *testing.T) {
	r, _ := testRoot(t)
	r.declutter.Photos = []core.PhotoGroup{{Photos: []core.PhotoItem{{Path: "a"}, {Path: "b"}}}}
	r.applyDeclutterPrune(view.DeclutterSimilarPhotos, nil, nil)
	func() {
		defer func() {
			if p := recover(); p != nil {
				t.Fatalf("expand after prune panicked: %v", p)
			}
		}()
		r.declutterActions().TogglePhotoGroup(0)
	}()
	if !r.declutter.ExpandedPhotoGroups[0] {
		t.Fatal("expand click had no effect")
	}
	// A rescan replaces the groups; old expanded indices must not carry over.
	r.declutter.ExpandedPhotoGroups[3] = true
	r.setDeclutterPhotos([]core.PhotoGroup{{Photos: []core.PhotoItem{{Path: "c"}, {Path: "d"}}}})
	if len(r.declutter.ExpandedPhotoGroups) != 0 {
		t.Fatalf("rescan kept stale expanded groups: %v", r.declutter.ExpandedPhotoGroups)
	}
}

func declutterFixture(t *testing.T, r *Root, names ...string) []string {
	t.Helper()
	dir := t.TempDir()
	r.declutterAuthority = map[string]core.ArbitraryTarget{}
	var paths []string
	for _, name := range names {
		p := filepath.Join(dir, name)
		if err := os.WriteFile(p, []byte("same content"), 0600); err != nil {
			t.Fatal(err)
		}
		r.declutterAuthority[p] = core.CaptureArbitraryTarget(p)
		paths = append(paths, p)
	}
	r.declutter.Scanned = true
	return paths
}

// Duplicates share identical content: selecting every copy of a group would
// move the only data to Trash. The clean is refused and names the file.
func TestDuplicateCleanRefusesGroupWithEveryCopySelected(t *testing.T) {
	r, _ := testRoot(t)
	paths := declutterFixture(t, r, "report.pdf", "report (1).pdf")
	r.declutter.Duplicates = []core.DuplicateGroup{{Filename: "report.pdf", Files: []core.DuplicateFileItem{{Path: paths[0], Selected: true}, {Path: paths[1], Selected: true}}}}
	r.requestDeclutterClean(view.DeclutterDuplicates)
	if r.confirming {
		t.Fatal("every copy of a duplicate group reached the delete confirmation")
	}
	if !strings.Contains(r.dashboard.StatusText, "report.pdf") {
		t.Fatalf("refusal does not name the group: %q", r.dashboard.StatusText)
	}
	r.declutter.Duplicates[0].Files[0].Selected = false
	r.requestDeclutterClean(view.DeclutterDuplicates)
	if !r.confirming {
		t.Fatalf("keeping one copy was still refused: %q", r.dashboard.StatusText)
	}
}

// Similar photos are not identical, so a whole group may be deliberate; the
// confirmation must say that no photo of the group will remain.
func TestPhotoCleanWarnsWhenWholeGroupSelected(t *testing.T) {
	r, _ := testRoot(t)
	paths := declutterFixture(t, r, "IMG_1.jpg", "IMG_2.jpg")
	r.declutter.Photos = []core.PhotoGroup{{Photos: []core.PhotoItem{{Path: paths[0], Selected: true}, {Path: paths[1], Selected: true}}}}
	r.requestDeclutterClean(view.DeclutterSimilarPhotos)
	if !r.confirming {
		t.Fatalf("whole photo group was refused instead of confirmed: %q", r.dashboard.StatusText)
	}
	warning := r.tr("整组", "entire group")
	if detail := r.confirmation.Detail.Get(r.settings.Language); !strings.Contains(detail, warning) {
		t.Fatalf("confirmation does not warn about the whole group: %s", detail)
	}
	r.resolveConfirmation(r.confirmation.ID, false)
	r.declutter.Photos[0].Photos[0].Selected = false
	r.requestDeclutterClean(view.DeclutterSimilarPhotos)
	if detail := r.confirmation.Detail.Get(r.settings.Language); strings.Contains(detail, warning) {
		t.Fatalf("warning shown although a photo is kept: %s", detail)
	}
}

// Cancelling a read-only job is the user's choice, not an error: the status
// must be localized and must not leak Go's "context canceled".
func TestCancelledReadOnlyJobShowsLocalizedStatus(t *testing.T) {
	r, q := testRoot(t)
	if !r.start(false, nil, func(ctx context.Context) func() {
		<-ctx.Done()
		return func() { t.Error("finish ran for a cancelled job") }
	}) {
		t.Fatal("job did not start")
	}
	r.Cancel()
	dispatchNext(t, q)
	if got, want := r.dashboard.StatusText, r.tr("已取消。", "Cancelled."); got != want {
		t.Fatalf("status = %q, want %q", got, want)
	}
}

// Declutter promises Trash in its confirmation and result ("moved to Trash,
// restorable"), as the Rust reference did with Disposal::RecycleBin. The Go
// port read the global delete-to-recycle-bin setting instead, which defaults
// to off, so every declutter clean deleted permanently under a Trash promise.
// The disk lens keeps following that setting.
func TestDeclutterCleanAlwaysUsesTrashWhileDiskLensFollowsSetting(t *testing.T) {
	r, q := testRoot(t)
	r.settings.DeleteToRecycleBin = false
	var got []core.Disposal
	r.cleanArbitrary = func(items []core.ArbitraryTarget, disposal core.Disposal, _ *core.CleanProgress) core.CleanReport {
		got = append(got, disposal)
		return core.CleanReport{}
	}
	paths := declutterFixture(t, r, "download.zip")
	r.declutter.Downloads = []core.DownloadItem{{Path: paths[0], Selected: true}}
	r.requestDeclutterClean(view.DeclutterDownloads)
	r.resolveConfirmation(r.confirmation.ID, true)
	for r.job != nil {
		dispatchNext(t, q)
	}
	r.requestArbitraryClean("disk", []core.ArbitraryTarget{core.CaptureArbitraryTarget(paths[0])}, 1, "", func(core.CleanReport) {})
	r.resolveConfirmation(r.confirmation.ID, true)
	for r.job != nil {
		dispatchNext(t, q)
	}
	if len(got) != 2 || got[0] != core.RecycleBin || got[1] != core.Permanent {
		t.Fatalf("disposals = %v, want [RecycleBin Permanent] (declutter, then disk lens with the setting off)", got)
	}
}

// Removal deliberately skips a rescan, so the page totals must follow the
// removed rows. Scan totals count each unique file once and the reclaimable
// sum covers removable assets; a removed item takes away its exclusive bytes
// (shared hard-linked files stay on disk), and a removed package shrinks its
// parent environment and its package count.
func TestDevRemovalKeepsTotalsAndParentInStep(t *testing.T) {
	r, q := testRoot(t)
	var measured []string
	r.measureDevStorage = func(dirs []string) (core.AssetStorageSize, []core.AssetStorageSize) {
		measured = dirs
		// keep's files were hard-linked with the removed "a": now exclusive.
		return core.AssetStorageSize{LogicalBytes: 222}, []core.AssetStorageSize{{LogicalBytes: 50, ExclusiveReclaimableBytes: 60}, {LogicalBytes: 172, ExclusiveReclaimableBytes: 170}}
	}
	count := 3
	a := core.DevAssetItem{ID: "a", Kind: core.DevAssetKind{Type: core.DevCondaEnv, Name: "a"}, Source: core.AssetTool, Size: core.AssetStorageSize{LogicalBytes: 100, ExclusiveReclaimableBytes: 80}}
	keep := core.DevAssetItem{ID: "keep", Kind: core.DevAssetKind{Type: core.DevCondaEnv, Name: "keep"}, Source: core.AssetTool, Size: core.AssetStorageSize{LogicalBytes: 50, ExclusiveReclaimableBytes: 50}}
	env := core.DevAssetItem{ID: "env", Kind: core.DevAssetKind{Type: core.DevCondaEnv, Name: "env", PackageCount: &count}, Source: core.AssetTool, Size: core.AssetStorageSize{LogicalBytes: 200, ExclusiveReclaimableBytes: 150}}
	pkg := core.DevAssetItem{ID: "env/pkg", Kind: core.DevAssetKind{Type: core.DevPipPackage, Name: "pkg"}, Source: core.AssetTool, Size: core.AssetStorageSize{LogicalBytes: 10, ExclusiveReclaimableBytes: 8}}
	other := core.DevAssetItem{ID: "env/other", Kind: core.DevAssetKind{Type: core.DevPipPackage, Name: "other"}, Source: core.AssetTool, Size: core.AssetStorageSize{LogicalBytes: 5, ExclusiveReclaimableBytes: 5}}
	r.dev.Assets = []core.DevAssetItem{a, keep, env}
	r.dev.Packages = map[string][]core.DevAssetItem{"env": {pkg, other}}
	r.dev.TotalLogical, r.dev.TotalReclaimable = 350, 280

	r.dropRemovedDevAssets([]core.DevAssetItem{a, pkg}, []core.RemovalOutcome{{Status: core.AssetRemoved}, {Status: core.AssetRemoved}})

	if len(r.dev.Assets) != 2 || len(r.dev.Packages["env"]) != 1 {
		t.Fatalf("rows: assets=%d packages=%d", len(r.dev.Assets), len(r.dev.Packages["env"]))
	}
	if r.dev.TotalLogical != 350-80-8 {
		t.Fatalf("total logical = %d, want %d", r.dev.TotalLogical, 350-80-8)
	}
	gotEnv := r.dev.Assets[1]
	if gotEnv.Size.LogicalBytes != 190 || gotEnv.Size.ExclusiveReclaimableBytes != 142 || gotEnv.Kind.PackageCount == nil || *gotEnv.Kind.PackageCount != 2 {
		t.Fatalf("parent env not updated: size=%+v packages=%v", gotEnv.Size, gotEnv.Kind.PackageCount)
	}
	if count != 3 {
		t.Fatal("package count was changed through a pointer shared with the caller")
	}
	if r.dev.TotalReclaimable != 50+142 {
		t.Fatalf("total reclaimable = %d, want %d", r.dev.TotalReclaimable, 50+142)
	}

	// The immediate figures are estimates: hard links make the exact change
	// unknowable without re-reading the remaining assets, which happens in
	// the background and replaces them.
	dispatchNext(t, q)
	if len(measured) != 2 {
		t.Fatalf("re-measured %d asset dirs, want the 2 remaining", len(measured))
	}
	if r.dev.TotalLogical != 222 || r.dev.TotalReclaimable != 60+170 || r.dev.Assets[0].Size.ExclusiveReclaimableBytes != 60 {
		t.Fatalf("measured totals not applied: logical=%d reclaimable=%d keep=%+v", r.dev.TotalLogical, r.dev.TotalReclaimable, r.dev.Assets[0].Size)
	}

	// A scan or removal that starts meanwhile makes the measurement stale.
	r.measureDevStorage = func(dirs []string) (core.AssetStorageSize, []core.AssetStorageSize) {
		return core.AssetStorageSize{LogicalBytes: 999}, make([]core.AssetStorageSize, len(dirs))
	}
	r.remeasureDev()
	r.devGeneration++
	dispatchNext(t, q)
	if r.dev.TotalLogical != 222 || r.dev.TotalReclaimable != 230 {
		t.Fatal("stale measurement overwrote newer state")
	}
	r.dev.TotalLogical = 1
	r.remeasureDev()
	r.dev.Assets = r.dev.Assets[:1]
	dispatchNext(t, q)
	if r.dev.TotalLogical != 1 {
		t.Fatal("measurement for a different asset list was applied")
	}
}
