package ui

import (
	"fmt"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func devEnvFixture() DevEnvPage {
	site, version, count := "/example/python/site-packages", "1.2", 2
	return DevEnvPage{Language: English, ActiveID: "python", Selected: map[string]bool{}, ChangedLabels: map[string]string{"python": "2 days ago"},
		TotalLogical: 900 << 20, TotalReclaimable: 150 << 20,
		Assets: []core.DevAssetItem{
			{ID: "base", Kind: core.DevAssetKind{Type: core.DevCondaEnv, Name: "base", IsBase: true}, Path: "/example/conda", Source: core.AssetTool},
			{ID: "python", Kind: core.DevAssetKind{Type: core.DevPythonInterpreter, Name: "Python 3.13", Scope: core.SiteUser, SitePackages: &site, PackageCount: &count}, Path: "/example/python", Source: core.AssetTool},
			{ID: "node", Kind: core.DevAssetKind{Type: core.DevNodePackage, Name: "npm-example", Manager: "npm", Version: &version, BinShims: []string{"example-cli"}}, Path: "/example/npm", Source: core.AssetTool, Size: core.AssetStorageSize{ExclusiveReclaimableBytes: 8 << 20}},
			{ID: "bun", Kind: core.DevAssetKind{Type: core.DevNodePackage, Name: "bun-example", Manager: "bun"}, Path: "/example/bun", Source: core.AssetTool},
			{ID: "tool", Kind: core.DevAssetKind{Type: core.DevPythonTool, Name: "ruff", ToolKind: core.PythonToolUv, Executables: []string{"ruff"}}, Path: "/example/uv", Source: core.AssetTool},
		},
		Packages: map[string][]core.DevAssetItem{"python": {
			{ID: "greek", Kind: core.DevAssetKind{Type: core.DevPipPackage, Name: "ΟΣ", Version: &version}, Path: "/example/python/site-packages/greek.dist-info", Source: core.AssetTool},
			{ID: "pkg", Kind: core.DevAssetKind{Type: core.DevPipPackage, Name: "requests"}, Path: "/example/python/site-packages/requests.dist-info", Source: core.AssetTool},
		}},
	}
}

func devEnvTester(s *DevEnvPage, a DevEnvPageActions) *ui.Tester {
	return ui.NewTester(func(c *ui.Context) {
		AppsPage{Language: s.Language, Tab: AppsPageDevEnvironments}.RenderContent(c, AppsPageActions{RenderDevEnvironments: func(c *ui.Context) { s.RenderContent(c, a) }})
	}, 1090, 800)
}

func TestDevEnvMasterDetailAndSelectionIsolation(t *testing.T) {
	s := devEnvFixture()
	before := append([]core.DevAssetItem(nil), s.Assets...)
	var requested []string
	a := DevEnvPageActions{
		SelectAsset:    func(id string) { s.ActiveID = id },
		ToggleSelected: func(id string) { s.Selected[id] = !s.Selected[id] },
		RequestRemove:  func(id string) { requested = append(requested, id) },
	}
	tt := devEnvTester(&s, a)
	for _, label := range []string{"Base Protected", "Display only", "2 days ago", "2 of 2 packages", filepath.Join("site-packages", "requests.dist-info")} {
		if !tt.HasText(label) {
			t.Fatalf("missing %q: %v", label, tt.Texts())
		}
	}
	if _, ok := tt.Find("Select base"); ok {
		t.Fatal("base environment acquired a destructive selection")
	}
	if _, ok := tt.Find("Select bun-example"); ok {
		t.Fatal("bun display-only entry acquired selection")
	}
	if err := tt.Click("Select npm-example"); err != nil {
		t.Fatal(err)
	}
	if !s.Selected["node"] || s.ActiveID != "python" {
		t.Fatal("checkbox click also changed active asset")
	}
	if err := tt.Click("Remove"); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(requested, []string{"greek"}) {
		t.Fatal(requested)
	}
	if err := tt.Click("npm-example"); err != nil {
		t.Fatal(err)
	}
	if s.ActiveID != "node" || !tt.HasText("Associated Executables") || !tt.HasText("example-cli") {
		t.Fatal("master selection did not replace detail")
	}
	if err := tt.Click("Remove"); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(requested, []string{"greek", "node"}) {
		t.Fatal(requested)
	}
	if !reflect.DeepEqual(before, s.Assets) {
		t.Fatal("render mutated native inventory")
	}
}

func TestDevEnvSearchFiltersAndPackageLoadStates(t *testing.T) {
	s := devEnvFixture()
	a := DevEnvPageActions{SetSearch: func(q string) { s.Search = q }, SetFilter: func(f DevEnvFilter) { s.Filter = f }}
	tt := devEnvTester(&s, a)
	if err := tt.Click("Search envs or packages..."); err != nil {
		t.Fatal(err)
	}
	tt.Type("ΟΣ")
	if s.Search != "ΟΣ" || !tt.HasText("Python 3.13") || !tt.HasText("1 of 2 packages") || tt.HasText("npm-example") || tt.HasText("requests") {
		t.Fatalf("Unicode package search: %v", tt.Texts())
	}
	s.Search = ""
	tt.Frame()
	if err := tt.Click("Node (2)"); err != nil {
		t.Fatal(err)
	}
	if s.Filter != DevEnvNode || !tt.HasText("npm-example") || tt.HasText("Python & Conda Environments") {
		t.Fatal(tt.Texts())
	}
	// Filtering does not silently change the selected detail, just as in Rust.
	if !tt.HasText("2 of 2 packages") {
		t.Fatal("filter discarded active detail")
	}
	s.LoadingSite = "python"
	tt.Frame()
	if !tt.HasText("Loading packages…") || tt.HasText("requests") {
		t.Fatal("stale package list shown during refresh")
	}
	s.LoadingSite = ""
	s.Packages["python"] = nil
	tt.Frame()
	if !tt.HasText("No identifiable installed packages") {
		t.Fatal(tt.Texts())
	}
	delete(s.Packages, "python")
	tt.Frame()
	if !tt.HasText("Loading packages…") {
		t.Fatal("unloaded inventory reported empty")
	}
}

func TestDevEnvBusyAndBatchRequests(t *testing.T) {
	for _, l := range []Language{Chinese, English} {
		s := devEnvFixture()
		s.Language = l
		s.Selected = map[string]bool{"node": true, "pkg": true, "stale-id": true}
		// Duplicate package projections must not double-count selection.
		s.Packages["duplicate"] = s.Packages["python"]
		toggles, batches, selections := 0, 0, 0
		a := DevEnvPageActions{ToggleSelected: func(string) { toggles++ }, RequestRemoveSelected: func() { batches++ }, SelectAll: func() { selections++ }}
		tt := devEnvTester(&s, a)
		tcopy := devEnvStrings(l)
		if !tt.HasText(fmt.Sprintf(tcopy.selection, 2, "8.4 MB")) {
			t.Fatalf("selection summary: %v", tt.Texts())
		}
		for _, label := range []string{fmt.Sprintf(tcopy.batchRemove, 2), tcopy.all} {
			if err := tt.Click(label); err != nil {
				t.Fatal(err)
			}
		}
		if batches != 1 || selections != 1 {
			t.Fatal("enabled requests not dispatched")
		}
		s.Removing = true
		tt.Frame()
		for _, label := range []string{fmt.Sprintf(tcopy.batchRemove, 2), tcopy.all, fmt.Sprintf(tcopy.selectionLabel, "npm-example")} {
			if err := tt.Click(label); err != nil {
				t.Fatal(err)
			}
		}
		if batches != 1 || selections != 1 || toggles != 0 {
			t.Fatal("busy actions dispatched")
		}
		s.Removing = false
		s.Selected = map[string]bool{"stale-id": true}
		tt.Frame()
		if err := tt.Click(fmt.Sprintf(tcopy.batchRemove, 0)); err != nil {
			t.Fatal(err)
		}
		if batches != 1 {
			t.Fatal("stale selection dispatched removal")
		}
		s.Scanning = true
		tt.Frame()
		if tt.HasText("npm-example") || tt.HasText(tcopy.selected) {
			t.Fatal("scan displays actionable stale results")
		}
	}
}
