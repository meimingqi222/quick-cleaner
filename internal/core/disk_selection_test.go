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
