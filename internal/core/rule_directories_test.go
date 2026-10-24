package core

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"testing"

	"github.com/pelletier/go-toml/v2"
)

func TestDirectoryLayoutsMatchUnchangedRustBaseline(t *testing.T) {
	root := t.TempDir()
	home, parent := filepath.Join(root, "home"), filepath.Join(root, "temp")
	mkdir := func(path string) {
		t.Helper()
		if err := os.MkdirAll(path, 0700); err != nil {
			t.Fatal(err)
		}
	}
	write := func(path string) {
		t.Helper()
		mkdir(filepath.Dir(path))
		if err := os.WriteFile(path, []byte("isolated"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	for _, dir := range []string{"Notion", "DiagnosticReports", "OneDrive", "com.apple.CloudTelemetry", "Telemetry"} {
		mkdir(filepath.Join(home, "Library/Logs", dir))
	}
	for _, file := range []string{"plain.log", "data.bin", "Telemetry/state.db", "Telemetry/state.db-wal"} {
		write(filepath.Join(home, "Library/Logs", file))
	}
	for _, dir := range []string{"C/com.apple.dns", "C/com.apple.quicklook", "C/unrelated", "T/com.apple.networkd", "T/com.apple.quicklook.worker"} {
		mkdir(filepath.Join(parent, dir))
	}
	write(filepath.Join(parent, "C/com.apple.dns-file"))
	for _, path := range []string{"Desktop/.DS_Store", "Desktop/project/.DS_Store", "Desktop/project/deep/.DS_Store"} {
		write(filepath.Join(home, path))
	}
	mkdir(filepath.Join(home, "Documents/.DS_Store"))
	for _, owner := range []string{"group.example", "TEAM.1Password"} {
		for _, path := range []string{"Library/Caches", "Library/tmp", "Library/Logs"} {
			mkdir(filepath.Join(home, "Library/Group Containers", owner, path))
		}
	}
	write(filepath.Join(home, "Library/Application Support/Firefox/Profiles/abc.default-release/cache2/data"))
	mkdir(filepath.Join(home, "Library/Application Support/Firefox/Profiles/xyz.profile-no-cache"))
	snapshot, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	s := &RuleSnapshot{Bundle: RuleBundle{Schema: RuleSchema, Sequence: 1, Rules: []RuleDefinition{clonePlanValue(*snapshot.Definition("macos"))}}}
	type row struct {
		Path        string
		Category    CategoryID
		Operation   Operation
		Disposal    Disposal
		Recommended bool
	}
	var actual, expected []row
	for _, target := range RuleDirectoryTargets(s, map[string]string{"home": home, "user_temp": filepath.Join(parent, "user-temp")}, "macos") {
		path, err := filepath.Rel(root, target.Path)
		if err != nil {
			t.Fatal(err)
		}
		actual = append(actual, row{filepath.ToSlash(path), target.Category, target.Operation, target.Disposal, target.Recommended})
	}
	sort.Slice(actual, func(i, j int) bool { return actual[i].Path < actual[j].Path })
	data, err := os.ReadFile("../../rules/fixtures/directory-layout-baseline.json")
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(data, &expected); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(actual, expected) {
		data, _ := json.MarshalIndent(actual, "", "  ")
		t.Fatalf("Rust directory baseline mismatch:\n%s", data)
	}
}

func TestDirectoryManifestAndChromiumLeafSelection(t *testing.T) {
	root := t.TempDir()
	write := func(path, value string) {
		t.Helper()
		path = filepath.Join(root, path)
		if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(value), 0600); err != nil {
			t.Fatal(err)
		}
	}
	for _, dir := range []string{"retired", "current", "not-bool"} {
		write("vendor/extensions/"+dir+"/sentinel", "x")
	}
	write("vendor/extensions/.obsolete", `{"retired":true,"current":false,"not-bool":"true","../escape":true}`)
	for name, project := range map[string]string{"missing": filepath.Join(root, "missing project"), "existing": filepath.Join(root, "vendor"), "encoded": filepath.Join(root, "missing%20project"), "outside": filepath.Join(filepath.Dir(root), "not-here")} {
		data, _ := json.Marshal(map[string]string{"folder": "file://" + project})
		write("editors/projects/"+name+"/workspace.json", string(data))
	}
	snapshot, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile("../../rules/fixtures/manifest-layout-extra.toml")
	if err != nil {
		t.Fatal(err)
	}
	var fixture RuleDefinition
	if err := toml.Unmarshal(data, &fixture); err != nil {
		t.Fatal(err)
	}
	s := &RuleSnapshot{Bundle: RuleBundle{Schema: RuleSchema, Sequence: 1, Rules: []RuleDefinition{fixture, clonePlanValue(*snapshot.Definition("chromium"))}}}
	s.Bundle.Rules[0].Directories = append(s.Bundle.Rules[0].Directories, DirectoryRule{ID: "leaves", Root: "home", Paths: []string{"browser"}, Select: Selection{Kind: "signature_leaves", Signature: "chromium"}, Zh: "浏览器 · {trail}", En: "Browser · {trail}", Category: "BrowserCache", Operation: "contents", Disposal: Permanent, Recommended: true})
	for _, path := range []string{"Cache/data", "GPUCache/data", "Default/Cache/data", "Default/blob_storage/data", "Default/IndexedDB/data", "Default/Local Storage/data", "Crashpad/completed/report", "Crashpad/pending/report"} {
		write("browser/"+path, "data")
	}
	got := map[string]ScanTarget{}
	for _, target := range RuleDirectoryTargets(s, map[string]string{"home": root}, "macos") {
		path, _ := filepath.Rel(root, target.Path)
		got[filepath.ToSlash(path)] = target
	}
	expected := map[string]bool{"vendor/extensions/retired": true, "editors/projects/missing": false, "browser/Cache": true, "browser/GPUCache": true, "browser/Default/Cache": true, "browser/Default/blob_storage": false, "browser/Crashpad/completed": true}
	if len(got) != len(expected) {
		t.Fatalf("targets = %v", got)
	}
	for path, recommended := range expected {
		item, exists := got[path]
		if !exists || item.Recommended != recommended {
			t.Fatalf("target %s = %+v", path, item)
		}
	}
	if label := got["browser/Default/Cache"].Label.Get(English); label != "Browser · Default · Cache" {
		t.Fatalf("leaf label = %s", label)
	}
}

func TestDirectoryInventoryBudgetNeverAuthorizesPartialChildren(t *testing.T) {
	root := t.TempDir()
	dir := filepath.Join(root, "cache")
	if err := os.Mkdir(dir, 0700); err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 4097; i++ {
		if err := os.WriteFile(filepath.Join(dir, fmt.Sprintf("entry-%04d", i)), nil, 0600); err != nil {
			t.Fatal(err)
		}
	}
	s := &RuleSnapshot{Bundle: RuleBundle{Schema: RuleSchema, Sequence: 1, Rules: []RuleDefinition{{ID: "fixture", Version: 1, Platform: "all", Directories: []DirectoryRule{{ID: "children", Root: "home", Paths: []string{"cache"}, Select: Selection{Kind: "children"}, Category: "UserCache", Operation: "file", Disposal: Permanent, Recommended: true}}}}}}
	if targets := RuleDirectoryTargets(s, map[string]string{"home": root}, "macos"); len(targets) != 0 {
		t.Fatalf("partial inventory authorized %d children", len(targets))
	}
}

func TestCacheCatalogMatchesUnchangedRustBaseline(t *testing.T) {
	root := t.TempDir()
	for _, name := range []string{"uv", "pypoetry", "opencode", "some-unknown-tool", "chrome-devtools-mcp/chrome-profile/Default/Cookies", "empty-leaf-only/Default/GPUCache"} {
		if err := os.MkdirAll(filepath.Join(root, ".cache", name), 0700); err != nil {
			t.Fatal(err)
		}
	}
	for _, name := range []string{"Cache", "GPUCache"} {
		path := filepath.Join(root, ".cache/chrome-devtools-mcp/chrome-profile/Default", name)
		if err := os.MkdirAll(path, 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(path, "data"), []byte("x"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(filepath.Join(root, ".cache/loose.txt"), []byte("file"), 0600); err != nil {
		t.Fatal(err)
	}
	s, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	fixture := &RuleSnapshot{Bundle: RuleBundle{Rules: []RuleDefinition{*s.Definition("cache"), *s.Definition("chromium")}}}
	type row struct {
		Relative, Label string
		Category        CategoryID
		Recommended     bool
		Operation       string
		Disposal        string
	}
	var actual, expected []row
	for _, target := range RuleTargets(fixture, map[string]string{"home": root}, "macos") {
		rel, err := filepath.Rel(root, target.Path)
		if err != nil {
			t.Fatal(err)
		}
		operation := map[OperationKind]string{OpContents: "Contents", OpTree: "Tree", OpFile: "File"}[target.Operation.Kind]
		disposal := map[Disposal]string{Permanent: "Permanent", RecycleBin: "RecycleBin"}[target.Disposal]
		actual = append(actual, row{filepath.ToSlash(rel), target.Label.Get(Chinese), target.Category, target.Recommended, operation, disposal})
	}
	sort.Slice(actual, func(i, j int) bool { return actual[i].Relative < actual[j].Relative })
	data, err := os.ReadFile("../../rules/fixtures/cache-catalog-baseline.json")
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(data, &expected); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(actual, expected) {
		t.Fatalf("Rust cache catalog mismatch:\ngot  %+v\nwant %+v", actual, expected)
	}
}
