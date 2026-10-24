package app

import (
	"context"
	"os"
	"path/filepath"
	"runtime"
	"testing"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func TestCancelAfterReadCompletesRejectsQueuedSnapshot(t *testing.T) {
	r, q := testRoot(t)
	r.junk.Discovering = true
	r.start(false, nil, func(context.Context) func() { return func() { t.Error("canceled snapshot published") } })
	var publication func()
	select {
	case publication = <-q:
	case <-time.After(5 * time.Second):
		t.Fatal("missing completion")
	}
	r.Cancel()
	publication()
	if r.job != nil || r.junk.Discovering {
		t.Fatal("canceled read left product busy")
	}
}

type volumesDiskOwner struct {
	calls []string
	trees map[string]*core.SizeTree
}

func (f *volumesDiskOwner) Close() {}
func (f *volumesDiskOwner) Load(_ context.Context, v core.VolumeID) (*core.ScanResult, error) {
	f.calls = append(f.calls, v.Mount)
	return &core.ScanResult{Tree: f.trees[v.Mount]}, nil
}

func TestSearchIndexesMissingVolumesWithoutChangingDiskNavigation(t *testing.T) {
	r, q := testRoot(t)
	owner := &volumesDiskOwner{trees: map[string]*core.SizeTree{}}
	for i := 0; i < 2; i++ {
		dir := t.TempDir()
		v := core.VolumeFromMountPoint(dir)
		r.disk.Volumes = append(r.disk.Volumes, view.DiskPageVolume{ID: v})
		owner.trees[v.Mount] = core.SizeTreeFromSnapshot(v, []core.TreeSnapshotEntry{{Path: dir, IsDir: true}, {Path: filepath.Join(dir, "needle.txt"), Size: 1}})
	}
	r.diskOwner = owner
	r.disk.Volume = r.disk.Volumes[0].ID
	r.tree = owner.trees[r.disk.Volume.Mount]
	r.searchTrees[r.disk.Volume.Mount] = r.tree
	diskTree := r.tree
	r.search.Query = "needle"
	r.scanSearch()
	dispatchNext(t, q)
	dispatchNext(t, q)
	expectedCalls := 1
	if runtime.GOOS == "darwin" {
		expectedCalls = 2
	}
	if len(owner.calls) != expectedCalls || len(r.search.Results) != 2 || !r.search.Ready || r.tree != diskTree {
		t.Fatalf("multi-volume index/navigation mismatch: calls=%v hits=%v", owner.calls, r.search.Results)
	}
}

func TestResidualAbsenceIsCheckedOnlyAfterExplicitConsent(t *testing.T) {
	r, q := testRoot(t)
	path := filepath.Join(t.TempDir(), "retain")
	if err := os.WriteFile(path, []byte("live app data"), 0600); err != nil {
		t.Fatal(err)
	}
	item := core.CertainResidual(core.ResidualKind{Type: core.ResidualFile, Path: path}, core.ResidualCacheDir)
	app := core.InstalledApp{ID: "example", Name: "Example"}
	unselected := core.CertainResidual(core.ResidualKind{Type: core.ResidualFile, Path: path + "-unselected"}, core.ResidualCacheDir)
	r.showResiduals(core.ResidualScanResult{AppName: app.Name, Items: []core.ResidualItem{item, unselected}}, &app)
	r.appsActions().SetResidualSelected("1", false)
	probes := 0
	r.residualAuthority = func(_ context.Context, reviewed core.InstalledApp, items []core.ResidualItem) bool {
		probes++
		if reviewed.ID != app.ID || len(items) != 1 || items[0].Kind.Path != path {
			t.Error("authority gate did not receive exactly the confirmed app and selected targets")
		}
		return false
	}
	r.requestResidualClean()
	if probes != 0 || !r.confirming {
		t.Fatal("absence/disposal occurred without consent")
	}
	r.resolveConfirmation(r.confirmation.ID, false)
	if probes != 0 {
		t.Fatal("cancellation probed or removed")
	}
	r.requestResidualClean()
	r.resolveConfirmation(r.confirmation.ID, true)
	dispatchNext(t, q)
	if probes != 1 || r.apps.Cleaning || r.apps.Notice.Get(core.English) == "" {
		t.Fatal("unknown absence did not refuse batch")
	}
	if data, err := os.ReadFile(path); err != nil || string(data) != "live app data" {
		t.Fatal("live/unknown-owner data removed", err)
	}
	if r.residual.Items[0].Identity != item.Identity {
		t.Fatal("review replaced frozen authority")
	}
}

func TestVenvLoadsPackagesButInstallationInterpreterDoesNot(t *testing.T) {
	r, q := testRoot(t)
	dir := t.TempDir()
	site := filepath.Join(dir, "Lib", "site-packages")
	metadata := filepath.Join(site, "example-1.dist-info", "METADATA")
	python := filepath.Join(dir, "bin", "python")
	if runtime.GOOS == "windows" {
		python = filepath.Join(dir, "Scripts", "python.exe")
	}
	for _, path := range []string{metadata, python} {
		if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte("Name: example\nVersion: 1\n"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	r.dev.Assets = []core.DevAssetItem{{ID: "venv", Path: dir, Kind: core.DevAssetKind{Type: core.DevVirtualEnv}}, {ID: "install", Kind: core.DevAssetKind{Type: core.DevPythonInterpreter, Scope: core.SiteInstall, SitePackages: &site, Python: python}}}
	r.selectDev("install")
	if r.job != nil {
		t.Fatal("installation interpreter acquired removable package list")
	}
	r.selectDev("venv")
	dispatchNext(t, q)
	packages := r.dev.Packages["venv"]
	if len(packages) != 1 || packages[0].Kind.Name != "example" || r.dev.LoadingSite != "" {
		t.Fatal("venv master/detail package load failed", packages)
	}
}

func TestCacheNormalAndForceRequireDistinctExplicitConfirmations(t *testing.T) {
	r, _ := testRoot(t)
	r.confirmCacheRelease(core.CacheReleasePlan{}, false)
	first := r.confirmation.ID
	if r.confirmation.Kind != view.ConfirmReleaseCache || r.confirmation.Force || r.job != nil {
		t.Fatal("normal cache consent incorrect")
	}
	r.resolveConfirmation(first, false)
	r.confirmCacheRelease(core.CacheReleasePlan{}, true)
	if !r.confirmation.Force || r.confirmation.ID == first || r.job != nil {
		t.Fatal("force escalation reused or bypassed consent")
	}
	r.resolveConfirmation(first, true)
	if !r.confirming || r.job != nil {
		t.Fatal("stale normal consent approved force")
	}
	r.resolveConfirmation(r.confirmation.ID, false)
}

func TestResidualUserDataRequiresOptInNotPermanentDisabling(t *testing.T) {
	r, _ := testRoot(t)
	item := core.CertainResidual(core.ResidualKind{Type: core.ResidualDirectory, Path: filepath.Join(t.TempDir(), "user-data")}, core.ResidualContainerDir)
	item.RequiresConfirmation = true
	extension := core.CertainResidual(core.ResidualKind{Type: core.ResidualSystemExtension, BundleID: "com.example.driver"}, core.ResidualExtensionSource)
	extension.RequiresConfirmation = true
	r.showResiduals(core.ResidualScanResult{Items: []core.ResidualItem{item, extension}}, nil)
	a := r.appsActions()
	if r.apps.Residuals[0].Selected {
		t.Fatal("user data preselected")
	}
	a.SetResidualSelected("0", true)
	if !r.apps.Residuals[0].Selected {
		t.Fatal("explicit user-data selection was incorrectly disabled")
	}
	a.SelectRecommended()
	if r.apps.Residuals[0].Selected {
		t.Fatal("recommended selection ignored explicit-confirmation policy")
	}
	a.SelectAllResiduals()
	if !r.apps.Residuals[0].Selected || r.apps.Residuals[1].Selected {
		t.Fatal("select-all conflated user data with manual system-extension removal")
	}
	a.ClearResidualSelection()
	if r.apps.Residuals[0].Selected {
		t.Fatal("clear retained user-data selection")
	}
}
