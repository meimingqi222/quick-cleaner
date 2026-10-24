package core

import (
	"os"
	"path/filepath"
	"sort"
	"strings"
)

func pathWithin(path, root string) bool {
	rel, err := filepath.Rel(root, path)
	return err == nil && rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator)) && !filepath.IsAbs(rel)
}

// DiskSelectionState gives the nearest explicit inclusion/exclusion precedence.
// Sizes are snapshots supplied by the caller; rendering never touches the disk.
type DiskSelectionState struct {
	selected map[string]uint64
	excluded map[string]uint64
}

func NewDiskSelectionState() *DiskSelectionState { return &DiskSelectionState{} }
func (s *DiskSelectionState) IsSelected(path string) bool {
	for p := filepath.Clean(path); ; p = filepath.Dir(p) {
		if _, ok := s.excluded[p]; ok {
			return false
		}
		if _, ok := s.selected[p]; ok {
			return true
		}
		if filepath.Dir(p) == p {
			return false
		}
	}
}
func (s *DiskSelectionState) hasSelectedAncestor(path string) bool {
	for p := filepath.Dir(path); p != path; path, p = p, filepath.Dir(p) {
		if _, ok := s.selected[p]; ok {
			return true
		}
	}
	return false
}
func (s *DiskSelectionState) Toggle(path string, size uint64) {
	path = filepath.Clean(path)
	if s.selected == nil {
		s.selected = make(map[string]uint64)
		s.excluded = make(map[string]uint64)
	}
	if s.IsSelected(path) {
		if _, ok := s.selected[path]; ok {
			delete(s.selected, path)
			for p := range s.excluded {
				if !s.hasSelectedAncestor(p) {
					delete(s.excluded, p)
				}
			}
		} else {
			s.excluded[path] = size
		}
	} else if _, ok := s.excluded[path]; ok {
		delete(s.excluded, path)
	} else {
		s.selected[path] = size
		for p, sz := range s.selected {
			if p == path || !pathWithin(p, path) {
				continue
			}
			delete(s.selected, p)
			if !s.IsSelected(p) {
				s.selected[p] = sz
			}
		}
	}
}
func (s *DiskSelectionState) Set(path string, size uint64, on bool) {
	if s.IsSelected(path) != on {
		s.Toggle(path, size)
	}
}
func (s *DiskSelectionState) Clear()        { clear(s.selected); clear(s.excluded) }
func (s *DiskSelectionState) Len() int      { return len(s.selected) }
func (s *DiskSelectionState) IsEmpty() bool { return s.Len() == 0 }
func (s *DiskSelectionState) TotalSize() uint64 {
	var picked, excluded uint64
	for _, n := range s.selected {
		picked = saturatingAdd(picked, n)
	}
	for _, n := range s.excluded {
		excluded = saturatingAdd(excluded, n)
	}
	return saturatingSub(picked, excluded)
}
func (s *DiskSelectionState) SelectedRoots() []string {
	out := make([]string, 0, s.Len())
	for p := range s.selected {
		out = append(out, p)
	}
	sort.Strings(out)
	return out
}

// ResolveTargets splits selected roots recursively around every exclusion.
// An unreadable branch is omitted rather than risking removal of an exclusion.
func (s *DiskSelectionState) ResolveTargets() []string {
	var out []string
	var expand func(string)
	expand = func(path string) {
		if _, ok := s.excluded[path]; ok {
			return
		}
		split := false
		for p := range s.excluded {
			if p != path && pathWithin(p, path) {
				split = true
				break
			}
		}
		if !split {
			out = append(out, path)
			return
		}
		info, err := os.Lstat(path)
		if err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
			return
		}
		entries, _ := os.ReadDir(path)
		for _, e := range entries {
			expand(filepath.Join(path, e.Name()))
		}
	}
	for _, p := range s.SelectedRoots() {
		expand(p)
	}
	sort.Strings(out)
	return out
}
