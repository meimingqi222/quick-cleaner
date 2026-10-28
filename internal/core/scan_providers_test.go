package core

import (
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"testing"
	"time"
)

func TestFilesystemProvidersMatchUnchangedLibraryClaimsBaseline(t *testing.T) {
	root := t.TempDir()
	home := filepath.Join(root, "home")
	cache, roaming := filepath.Join(home, "Library/Caches"), filepath.Join(home, "Library/Application Support")
	for _, relative := range []string{"Google/Chrome", "Google/Other", "Microsoft Edge", "com.apple.Safari", "Firefox", "Homebrew", "pip", "claude-cli-nodejs/Cache", "claude-cli-nodejs/other", "Zed/logs", "Zed/extra", "UnknownVendor", "MyUpdater/pending", "com.apple.dt.Xcode/Downloads"} {
		if err := os.MkdirAll(filepath.Join(cache, relative), 0700); err != nil {
			t.Fatal(err)
		}
	}
	for _, relative := range []string{"Google/Chrome/blob", "MyUpdater/update.zip", "loose-file"} {
		if err := os.WriteFile(filepath.Join(cache, relative), []byte("x"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	for _, relative := range []string{"Google/Chrome/Default/Cache", "Google/Chrome/Default/GPUCache", "Google/Chrome/Default/Code Cache", "Google/OtherApp/Cache", "Google/OtherApp/GPUCache", "Arc/Cache", "Maka/Code Cache", "Maka/GPUCache", "Maka/DawnGraphiteCache", "Code/CachedExtensionVSIXs", "MobileSync/Backup/device-1", "Microsoft Edge/Default/GPUCache"} {
		if err := os.MkdirAll(filepath.Join(roaming, relative), 0700); err != nil {
			t.Fatal(err)
		}
	}
	s, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	type row struct {
		Path, Zh, En string
		Category     CategoryID
		Recommended  bool
		Operation    string
		Disposal     string
	}
	var actual, expected []row
	for _, target := range FilesystemScanTargets(s, map[string]string{"home": home, "local": cache, "cache": cache, "roaming": roaming}, "macos") {
		md, err := os.Lstat(target.Path)
		if err != nil || !md.IsDir() {
			continue
		}
		rel, err := filepath.Rel(root, target.Path)
		if err != nil {
			t.Fatal(err)
		}
		operation := map[OperationKind]string{OpContents: "Contents", OpTree: "Tree", OpFile: "File"}[target.Operation.Kind]
		disposal := map[Disposal]string{Permanent: "Permanent", RecycleBin: "RecycleBin"}[target.Disposal]
		actual = append(actual, row{filepath.ToSlash(rel), target.Label.Get(Chinese), target.Label.Get(English), target.Category, target.Recommended, operation, disposal})
	}
	sort.Slice(actual, func(i, j int) bool { return actual[i].Path < actual[j].Path })
	data, err := os.ReadFile("../../rules/fixtures/library-claims-baseline.json")
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(data, &expected); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(actual, expected) {
		a, _ := json.MarshalIndent(actual, "", "  ")
		t.Fatalf("Rust library claims mismatch:\n%s", a)
	}
}

func TestFilesystemProvidersPreserveMixedCachesAndWindowsPrivateState(t *testing.T) {
	snapshot, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	for _, osName := range []string{"macos", "windows"} {
		t.Run(osName, func(t *testing.T) {
			home := t.TempDir()
			local, roaming := filepath.Join(home, "Library/Caches"), filepath.Join(home, "roaming")
			write := func(root, relative string) string {
				t.Helper()
				path := filepath.Join(root, relative)
				if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
					t.Fatal(err)
				}
				if err := os.WriteFile(path, []byte("isolated"), 0600); err != nil {
					t.Fatal(err)
				}
				return path
			}
			old := write(local, "Fixture-updater/update.zip")
			past := time.Now().Add(-8 * 24 * time.Hour)
			if err := os.Chtimes(old, past, past); err != nil {
				t.Fatal(err)
			}
			write(local, "Fixture-updater/pending/payload")
			write(local, "Fixture-updater/update.log")
			write(local, "Fixture-updater/Cache.db")
			write(local, "JetBrains/LocalHistory/keep")
			write(local, "CloudKit/update.zip")
			write(local, "com.apple.fixture/update.zip")
			for _, relative := range []string{"UnknownApp/Cache/data", "UnknownApp/GPUCache/data", "UnknownApp/Default/Local Storage/data", "UnknownApp/Default/IndexedDB/data", "UnknownApp/Crashpad/completed/report", "UnknownApp/Crashpad/pending/report", "Google/Chrome/User Data/Default/Cache/data", "Google/Chrome/User Data/Default/blob_storage/data", "Google/Chrome/User Data/Local State"} {
				write(roaming, relative)
			}
			if osName == "windows" {
				write(local, "Google/Chrome/User Data/Default/Cache/data")
				write(local, "Google/Chrome/User Data/Default/blob_storage/data")
			}
			got := map[string]ScanTarget{}
			for _, target := range FilesystemScanTargets(snapshot, map[string]string{"home": home, "local": local, "roaming": roaming}, osName) {
				got[target.Path] = target
			}
			if target, ok := got[old]; !ok || target.Category != UpdaterPackages || !target.Recommended || target.Operation.Kind != OpTree || target.Label.Get(English) != "Fixture · update.zip" {
				t.Fatalf("old artifact: %+v, exists=%v", target, ok)
			}
			if target, ok := got[filepath.Join(local, "Fixture-updater/pending")]; !ok || target.Recommended || target.Category != UpdaterPackages {
				t.Fatalf("fresh artifact: %+v, exists=%v", target, ok)
			}
			for _, relative := range []string{"Fixture-updater/Cache.db", "Fixture-updater/update.log", "JetBrains"} {
				target, exists := got[filepath.Join(local, relative)]
				if exists != (osName == "macos") || target.Recommended {
					t.Fatalf("ambiguous/private cache %s: %+v, exists=%v", relative, target, exists)
				}
			}
			if _, exists := got[filepath.Join(local, "Fixture-updater")]; exists {
				t.Fatal("mixed cache parent double counted")
			}
			if osName == "macos" {
				if _, exists := got[filepath.Join(local, "CloudKit")]; exists {
					t.Fatal("sensitive cache surfaced")
				}
				if _, exists := got[filepath.Join(local, "com.apple.fixture/update.zip")]; exists {
					t.Fatal("Apple service probed as third-party updater")
				}
			}
			for _, relative := range []string{"UnknownApp/Cache", "UnknownApp/GPUCache", "UnknownApp/Crashpad/completed"} {
				if target, exists := got[filepath.Join(roaming, relative)]; !exists || target.Category != UserCache || !target.Recommended {
					t.Fatalf("signature cache %s: %+v, exists=%v", relative, target, exists)
				}
			}
			for _, relative := range []string{"UnknownApp", "UnknownApp/Default/Local Storage", "UnknownApp/Default/IndexedDB", "UnknownApp/Crashpad/pending"} {
				if _, exists := got[filepath.Join(roaming, relative)]; exists {
					t.Fatalf("session/profile target: %s", relative)
				}
			}
			if osName == "windows" {
				if target := got[filepath.Join(local, "Google/Chrome/User Data/Default/Cache")]; target.Category != BrowserCache || target.Label.Get(English) != "Chrome · Default · Cache" || !target.Recommended {
					t.Fatalf("declared browser: %+v", target)
				}
				if target, exists := got[filepath.Join(local, "Google/Chrome/User Data/Default/blob_storage")]; !exists || target.Recommended {
					t.Fatalf("shown browser leaf: %+v, exists=%v", target, exists)
				}
			}
		})
	}
}
