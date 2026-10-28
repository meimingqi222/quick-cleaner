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

func TestVersionLayoutsMatchUnchangedRustBaseline(t *testing.T) {
	snapshot, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	type row struct {
		Layout, Platform, Path string
		Category               CategoryID
		Operation              Operation
		Disposal               Disposal
		Recommended            bool
	}
	data, err := os.ReadFile("../../rules/fixtures/version-layout-baseline.json")
	if err != nil {
		t.Fatal(err)
	}
	var expected, actual []row
	if err := json.Unmarshal(data, &expected); err != nil {
		t.Fatal(err)
	}
	for _, layout := range snapshot.Definition("development").VersionLayouts {
		root := t.TempDir()
		base := filepath.Join(root, layout.Path)
		versions := filepath.Join(base, layout.Versions)
		for _, name := range []string{"9.9.9", "8.8.8"} {
			if err := os.MkdirAll(filepath.Join(versions, name), 0700); err != nil {
				t.Fatal(err)
			}
		}
		if layout.Current != nil {
			pointer := filepath.Join(base, *layout.Current)
			if err := os.MkdirAll(filepath.Dir(pointer), 0700); err != nil {
				t.Fatal(err)
			}
			if err := os.Symlink(filepath.Join(versions, "8.8.8"), pointer); err != nil {
				t.Skipf("link fixture unavailable: %v", err)
			}
		}
		if layout.Lock != nil {
			path := filepath.Join(base, layout.Lock.Path)
			if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(path, []byte("lock"), 0600); err != nil {
				t.Fatal(err)
			}
		}
		if d := layout.Downloads; d != nil {
			path := filepath.Join(base, d.Path)
			if err := os.MkdirAll(path, 0700); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(path, "installer.tar.gz"), []byte("pkg"), 0600); err != nil {
				t.Fatal(err)
			}
		}
		bundle := clonePlanValue(snapshot.Bundle)
		s := &RuleSnapshot{Bundle: bundle}
		s.Definition("development").VersionLayouts = []VersionLayout{layout}
		osName := "macos"
		if layout.Platform == "windows" {
			osName = "windows"
		}
		for _, target := range RuleVersionTargets(s, map[string]string{string(layout.Root): root}, osName) {
			path, err := filepath.Rel(root, target.Path)
			if err != nil {
				t.Fatal(err)
			}
			actual = append(actual, row{layout.ID, string(layout.Platform), filepath.ToSlash(path), target.Category, target.Operation, target.Disposal, target.Recommended})
		}
	}
	sort.Slice(actual, func(i, j int) bool {
		if actual[i].Layout == actual[j].Layout {
			return actual[i].Path < actual[j].Path
		}
		return actual[i].Layout < actual[j].Layout
	})
	// The source fixture groups platforms; this test exercises both in one run.
	sort.Slice(expected, func(i, j int) bool {
		if expected[i].Layout == expected[j].Layout {
			return expected[i].Path < expected[j].Path
		}
		return expected[i].Layout < expected[j].Layout
	})
	if !reflect.DeepEqual(actual, expected) {
		a, _ := json.MarshalIndent(actual, "", "  ")
		t.Fatalf("Rust baseline mismatch; actual:\n%s", a)
	}
}

func TestVersionPlanRevalidatesCurrentPointerAndUpdateLock(t *testing.T) {
	root := t.TempDir()
	snapshot, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	s := &RuleSnapshot{Bundle: clonePlanValue(snapshot.Bundle)}
	var layout VersionLayout
	for _, l := range s.Definition("development").VersionLayouts {
		if l.ID == "codex_cli" {
			layout = l
		}
	}
	layout.Recommended = true
	s.Definition("development").VersionLayouts = []VersionLayout{layout}
	base := filepath.Join(root, layout.Path)
	versions := filepath.Join(base, layout.Versions)
	for _, name := range []string{"old", "current-version", "next"} {
		if err := os.MkdirAll(filepath.Join(versions, name), 0700); err != nil {
			t.Fatal(err)
		}
	}
	pointer := filepath.Join(base, *layout.Current)
	if err := os.Symlink(filepath.Join(versions, "current-version"), pointer); err != nil {
		t.Skipf("link fixture unavailable: %v", err)
	}
	scan := func() []ScanTarget { return RuleVersionTargets(s, map[string]string{"home": root}, "macos") }
	targets := scan()
	if len(targets) != 2 {
		t.Fatalf("targets = %+v", targets)
	}
	var target ScanTarget
	for _, it := range targets {
		if filepath.Base(it.Path) == "old" {
			target = it
		}
		if !it.Recommended {
			t.Fatal("idle old version not recommended")
		}
	}
	identity, ok := CaptureIdentity(target.Path)
	if !ok {
		t.Fatal("fixture identity unavailable")
	}
	plan := NewCleanupPlan(target.Rule, []PlannedTarget{{Path: target.Path, Operation: target.Operation, Disposal: target.Disposal, Identity: &identity}})
	if err := plan.Validate(); err != nil {
		t.Fatal(err)
	}
	if err := os.Remove(pointer); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(filepath.Join(versions, "old"), pointer); err != nil {
		t.Fatal(err)
	}
	if plan.Validate() == nil {
		t.Fatal("current pointer change authorized old plan")
	}
	for _, it := range scan() {
		if filepath.Base(it.Path) == "old" {
			t.Fatal("current version selected")
		}
	}
	lock := filepath.Join(base, layout.Lock.Path)
	if err := os.WriteFile(lock, []byte("updating"), 0600); err != nil {
		t.Fatal(err)
	}
	for _, it := range scan() {
		if it.Recommended || it.Rule.Revalidate() == nil {
			t.Fatal("active update authorized cleanup")
		}
	}
	stale := time.Now().Add(-time.Duration(layout.Lock.MaximumAgeSeconds+60) * time.Second)
	if err := os.Chtimes(lock, stale, stale); err != nil {
		t.Fatal(err)
	}
	for _, it := range scan() {
		if !it.Recommended || it.Rule.Revalidate() != nil {
			t.Fatal("stale lock blocked idle layout")
		}
	}
	if err := os.Remove(pointer); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(pointer, []byte("not a link"), 0600); err != nil {
		t.Fatal(err)
	}
	for _, it := range scan() {
		if it.Recommended || it.Rule.Revalidate() == nil {
			t.Fatal("unknown pointer authorized cleanup")
		}
	}
}
