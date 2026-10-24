package core

import (
	"context"
	"os"
	"path/filepath"
	"slices"
	"testing"
)

func TestOrphanResidualScanKeepsLiveFamiliesAndRejectsFilenameNoise(t *testing.T) {
	home := t.TempDir()
	snapshot, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	for _, relative := range []string{
		"HTTPStorages/com.gone.product.enterprise/localstorage", "Logs/com.gone.Widget/data",
		"Saved Application State/org.cindori.Sensei.savedState/data", "Preferences/com.gone.application.plist",
		"Preferences/ByHost/com.foo.Bar.5E9D1F0A-1B2C-3D4E-5F60-123456789ABC.plist",
		"WebKit/cenmrev.V2RayX/data", "Caches/warp.log.old.0", "Caches/main.kts.compiled.cache",
		"Caches/.com.raycast.macos.backups", "HTTPStorages/com.apple.akd/data",
		"Caches/systemgroup.com.apple.notes/data", "Preferences/com.alive.Vendor.plist",
		"HTTPStorages/com.alive.Vendor.helper/data", "LaunchAgents/com.docker.vmnetd.plist",
		"Caches/com.google/data", "Caches/cn.trae.app.helper.gpu/data",
		"Containers/com.gone.private/database", "Group Containers/com.gone.private/database",
	} {
		sourceFixtureFile(t, filepath.Join(home, "Library", filepath.FromSlash(relative)), "fixture")
	}
	installed := lowerBundleIDs(map[string]struct{}{"com.alive.Vendor": {}, "com.docker.docker": {}, "com.google.Chrome": {}, "cn.trae.app": {}})
	result, err := scanOrphanResidualsIn(context.Background(), home, installed, snapshot)
	if err != nil {
		t.Fatal(err)
	}
	var names []string
	for _, item := range result.Items {
		names = append(names, filepath.Base(item.Kind.Path))
		if item.Identity == nil || item.Rule == nil || item.Confidence != Possible || !item.RequiresConfirmation {
			t.Fatalf("missing evidence/manual gate: %+v", item)
		}
		if _, ok := item.Owner(); !ok {
			t.Fatal("missing exact owner")
		}
	}
	want := []string{"com.gone.product.enterprise", "com.gone.Widget", "org.cindori.Sensei.savedState", "com.gone.application.plist", "com.foo.Bar.5E9D1F0A-1B2C-3D4E-5F60-123456789ABC.plist", "cenmrev.V2RayX"}
	slices.Sort(names)
	slices.Sort(want)
	if !slices.Equal(names, want) {
		t.Fatalf("names=%q want=%q", names, want)
	}
	if result.Scope != ResidualScopeOrphanLeftovers || len(result.DefaultSelection()) != 0 || result.TotalFileSize == 0 {
		t.Fatalf("scan semantics=%+v", result)
	}
	if !slices.IsSortedFunc(result.Items, func(a, b ResidualItem) int {
		if a.Size() > b.Size() {
			return -1
		}
		if a.Size() < b.Size() {
			return 1
		}
		return 0
	}) {
		t.Fatal("not descending by allocated size")
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := scanOrphanResidualsIn(ctx, home, installed, snapshot); err == nil {
		t.Fatal("cancelled scan accepted")
	}
	if _, err := os.Stat(filepath.Join(home, "Library", "Containers", "com.gone.private", "database")); err != nil {
		t.Fatal("scan mutated data", err)
	}
}

func TestOrphanClassificationRequiresExactRemovedOwnerAndFreshWholeBatchGate(t *testing.T) {
	item := func(id string, size uint64) ResidualItem {
		return PossibleResidual(ResidualKind{Type: ResidualFile, Path: filepath.Join(t.TempDir(), id), Bytes: size}, ResidualCacheDir).ForBundle(id)
	}
	result := ResidualScanResult{Items: []ResidualItem{item("org.webkit.playwright", 3), item("com.mitchellh.ghostty", 5), item("com.mitchellh.other", 7), item("com.hackemist.sdimagecache", 11)}}
	classifyOrphanEvidence(&result, map[string]struct{}{"org.webkit.playwright": {}}, map[string]struct{}{"com.mitchellh.ghostty": {}})
	if len(result.Items) != 3 || result.TotalFileSize != 23 || result.Items[0].Confidence != Certain || result.Items[1].Confidence != Possible || len(result.DefaultSelection()) != 0 {
		t.Fatalf("classification=%+v", result)
	}
	if !orphanOwnersGone(result.Items, map[string]struct{}{}) {
		t.Fatal("known absent owners refused")
	}
	if orphanOwnersGone(result.Items, map[string]struct{}{"com.mitchellh.ghostty": {}}) {
		t.Fatal("reinstalled owner did not block entire batch")
	}
	unknown := result.Items[0]
	unknown.OwnerBundleID = nil
	if orphanOwnersGone([]ResidualItem{result.Items[1], unknown}, nil) || orphanOwnersGone(nil, nil) {
		t.Fatal("unknown or empty selection accepted")
	}
	classifyOrphanEvidence(&result, map[string]struct{}{"com.mitchellh.ghostty": {}}, map[string]struct{}{"com.mitchellh.ghostty": {}})
	if len(result.Items) != 1 || result.Items[0].OwnerBundleID == nil || *result.Items[0].OwnerBundleID != "com.hackemist.sdimagecache" {
		t.Fatal("live vendor did not suppress siblings despite trashed copy", result.Items)
	}
}
