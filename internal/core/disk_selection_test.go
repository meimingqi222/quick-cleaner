package core

import (
	"os"
	"path/filepath"
	"reflect"
	"testing"
)

func TestDiskSelectionInheritanceAndAccounting(t *testing.T) {
	root := t.TempDir()
	a := filepath.Join(root, "a")
	b := filepath.Join(a, "b")
	c := filepath.Join(b, "c")
	s := NewDiskSelectionState()
	s.Toggle(b, 100)
	s.Toggle(a, 1000)
	if s.Len() != 1 || s.TotalSize() != 1000 || !s.IsSelected(c) {
		t.Fatal(s)
	}
	s.Toggle(b, 300)
	if s.IsSelected(c) || s.TotalSize() != 700 {
		t.Fatal(s)
	}
	s.Toggle(c, 50)
	if !s.IsSelected(c) || s.TotalSize() != 750 || s.Len() != 2 {
		t.Fatal(s)
	}
	s.Toggle(a, 1000)
	if s.IsSelected(a) || !s.IsSelected(c) || s.TotalSize() != 50 {
		t.Fatal(s)
	}
	s.Toggle(a, 1000)
	if !s.IsSelected(b) || s.TotalSize() != 1000 || s.Len() != 1 {
		t.Fatal(s)
	}
	s.Set(filepath.Join(root, "other"), 25, true)
	if s.TotalSize() != 1025 {
		t.Fatal(s)
	}
	s.Clear()
	if s.Len() != 0 || !s.IsEmpty() || s.TotalSize() != 0 || len(s.ResolveTargets()) != 0 {
		t.Fatal(s)
	}
}

func TestDiskSelectionSetManyMatchesIndividualSets(t *testing.T) {
	root := t.TempDir()
	parent := filepath.Join(root, "parent")
	paths := []string{
		filepath.Join(parent, "a"),
		filepath.Join(parent, "b"),
		filepath.Join(root, "c"),
	}
	sizes := []uint64{10, 20, 30}
	batch, single := NewDiskSelectionState(), NewDiskSelectionState()
	batch.SetMany(paths, sizes, true)
	for i, path := range paths {
		single.Set(path, sizes[i], true)
	}
	assertSameSelection(t, batch, single)

	batch.SetMany(paths[:2], sizes[:2], false)
	for i, path := range paths[:2] {
		single.Set(path, sizes[i], false)
	}
	assertSameSelection(t, batch, single)

	batch.Toggle(parent, 100)
	single.Toggle(parent, 100)
	batch.SetMany([]string{filepath.Join(parent, "a")}, []uint64{10}, false)
	single.Set(filepath.Join(parent, "a"), 10, false)
	assertSameSelection(t, batch, single)
}

// A selection under an excluded folder is an explicit re-inclusion, not a
// redundant child of the selected ancestor. A batch select elsewhere, or a
// batch select of a higher ancestor, must not drop it.
func TestDiskSelectionSetManyKeepsReinclusionUnderExclusion(t *testing.T) {
	root := t.TempDir()
	a := filepath.Join(root, "a")
	x := filepath.Join(a, "x")
	y := filepath.Join(x, "y")
	z := filepath.Join(y, "z")
	other := filepath.Join(root, "other")
	setup := func() *DiskSelectionState {
		s := NewDiskSelectionState()
		s.Toggle(x, 50)
		s.Toggle(y, 20)
		s.Toggle(z, 10)
		return s
	}

	batch, single := setup(), setup()
	batch.SetMany([]string{other}, []uint64{1}, true)
	single.Set(other, 1, true)
	assertSameSelection(t, batch, single)
	if !batch.IsSelected(z) {
		t.Fatal("unrelated batch select dropped a re-included path")
	}

	batch, single = setup(), setup()
	batch.SetMany([]string{a}, []uint64{100}, true)
	single.Set(a, 100, true)
	assertSameSelection(t, batch, single)
	if !batch.IsSelected(z) || batch.IsSelected(y) {
		t.Fatal("selecting an ancestor changed the nearest explicit choice below it")
	}
}

func assertSameSelection(t *testing.T, a, b *DiskSelectionState) {
	t.Helper()
	if !reflect.DeepEqual(a.SelectedRoots(), b.SelectedRoots()) || a.TotalSize() != b.TotalSize() || a.Len() != b.Len() {
		t.Fatalf("batch=%v/%d/%d individual=%v/%d/%d", a.SelectedRoots(), a.Len(), a.TotalSize(), b.SelectedRoots(), b.Len(), b.TotalSize())
	}
}

func TestDiskSelectionDeepResolveAndReinclusion(t *testing.T) {
	root := t.TempDir()
	keep := filepath.Join(root, "branch", "deep", "keep")
	remove := filepath.Join(root, "branch", "deep", "remove")
	sibling := filepath.Join(root, "sibling")
	for _, p := range []string{keep, remove, sibling} {
		if err := os.MkdirAll(p, 0700); err != nil {
			t.Fatal(err)
		}
	}
	s := NewDiskSelectionState()
	s.Toggle(root, 1000)
	s.Toggle(keep, 100)
	if got := s.ResolveTargets(); !reflect.DeepEqual(got, []string{remove, sibling}) {
		t.Fatal(got)
	}
	s.Toggle(keep, 100)
	if got := s.ResolveTargets(); !reflect.DeepEqual(got, []string{root}) {
		t.Fatal(got)
	}
	s.Toggle(filepath.Join(root, "branch"), 300)
	s.Toggle(keep, 100)
	if !s.IsSelected(keep) || s.IsSelected(remove) {
		t.Fatal(s)
	}
	if got := s.ResolveTargets(); !reflect.DeepEqual(got, []string{keep, sibling}) {
		t.Fatal(got)
	}
}

// Rows of one folder share a parent, so the disk lens resolves the parent once
// and asks each child only about its own entry. That shortcut must agree with
// the full ancestor walk for every node, across nested exclusions.
func TestDiskSelectionChildSelectedMatchesAncestorWalk(t *testing.T) {
	root := t.TempDir()
	a := filepath.Join(root, "a")
	x := filepath.Join(a, "x")
	y := filepath.Join(x, "y")
	z := filepath.Join(y, "z")
	s := NewDiskSelectionState()
	s.Toggle(a, 100)
	s.Toggle(x, 50)
	s.Toggle(z, 10)
	nodes := []string{root, a, x, y, z, filepath.Join(z, "leaf"), filepath.Join(y, "sibling"), filepath.Join(a, "other"), filepath.Join(root, "outside")}
	for _, node := range nodes {
		parent := filepath.Dir(node)
		if got, want := s.ChildSelected(node, s.IsSelected(parent)), s.IsSelected(node); got != want {
			t.Fatalf("%s: ChildSelected=%v IsSelected=%v", node, got, want)
		}
	}
}
