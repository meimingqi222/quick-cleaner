package core

import (
	"context"
	"path/filepath"
	"testing"
)

func TestJunkIndexRetainsUnixRootVolume(t *testing.T) {
	if filepath.Separator != '/' {
		t.Skip("Unix root mount")
	}
	mount := VolumeFromMountPoint("/")
	home := t.TempDir()
	modules := filepath.Join(home, "code", "app", "node_modules")
	snapshot := []TreeSnapshotEntry{{Path: filepath.Join(modules, "pkg.js"), Size: 4242}}
	for p := modules; p != "/"; p = filepath.Dir(p) {
		snapshot = append(snapshot, TreeSnapshotEntry{Path: p, IsDir: true})
	}
	tree := SizeTreeFromSnapshot(mount, snapshot)
	if !tree.CoversSubtree(modules) {
		t.Fatal("fixture does not cover the target")
	}

	targets := []ScanTarget{{Path: modules, Category: DevBuild, Operation: Operation{Kind: OpTree}}}
	plan := BeginJunkIndex(context.Background(), false, home, []VolumeID{mount}, []*SizeTree{tree}, targets, nil)
	if plan.Query == nil || len(plan.trees) != 1 {
		t.Fatal("root-volume index was discarded")
	}
	// No live fixture exists: this measurement must come from the held index.
	got, ok := plan.Query.MeasurePath(context.Background(), modules)
	if !ok || got.Size != 4242 {
		t.Fatalf("root-volume measurement fell back to disk: %+v %v", got, ok)
	}
	tree.SetBlockedSubtrees([]string{modules})
	if _, ok := plan.Query.MeasurePath(context.Background(), modules); ok {
		t.Fatal("root-volume query trusted an incomplete subtree")
	}
	tree.SetBlockedSubtrees(nil)
	if v, ok := volumeContaining(modules, []VolumeID{mount}); !ok || v.Mount != "/" {
		t.Fatal("root volume does not contain an absolute target")
	}
	if _, ok := volumeContaining("relative/path", []VolumeID{mount}); ok {
		t.Fatal("root volume accepted a relative target")
	}
	subMount := VolumeFromMountPoint(home)
	subTree := SizeTreeFromSnapshot(subMount, []TreeSnapshotEntry{{Path: filepath.Join(home, "code"), IsDir: true}, {Path: filepath.Join(home, "code", "app"), IsDir: true}, {Path: modules, IsDir: true}, {Path: filepath.Join(modules, "pkg.js"), Size: 99}})
	plan = BeginJunkIndex(context.Background(), false, home, []VolumeID{mount, subMount}, []*SizeTree{tree, subTree}, targets, nil)
	got, ok = plan.Query.MeasurePath(context.Background(), modules)
	if !ok || got.Size != 99 {
		t.Fatalf("more specific mount lost priority: %+v %v", got, ok)
	}
}
