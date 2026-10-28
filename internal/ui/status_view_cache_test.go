package ui

import (
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

// Hover and pointer moves rebuild the page; the process view must not be
// filtered, sorted and grouped again unless its inputs changed.
func TestProcessViewIsComputedOncePerInputs(t *testing.T) {
	s := statusFixture()
	first := s.filteredProcesses()
	if len(first) == 0 {
		t.Fatal("fixture has no processes")
	}
	if again := s.filteredProcesses(); &again[0] != &first[0] {
		t.Fatal("unchanged inputs filtered and sorted the processes again")
	}
	rows := s.cachedProcessRows(first)
	if again := s.cachedProcessRows(first); len(rows) == 0 || &again[0] != &rows[0] {
		t.Fatal("unchanged inputs grouped the processes again")
	}

	next := *s.Snapshot
	s.Snapshot = &next
	if got := s.filteredProcesses(); &got[0] == &first[0] {
		t.Fatal("a new sample reused the previous view")
	}
	s.Query = "node"
	for _, p := range s.filteredProcesses() {
		if !core.MatchProcQuery(p, "node") {
			t.Fatalf("a changed query kept %q", p.Name)
		}
	}
	s.Query = ""
	filtered := s.filteredProcesses()
	before := s.cachedProcessRows(filtered)
	if s.ExpandedGroups == nil {
		s.ExpandedGroups = map[string]bool{}
	}
	for _, row := range before {
		if row.count > 1 {
			s.ExpandedGroups[row.group] = true
		}
	}
	if after := s.cachedProcessRows(filtered); len(before) > 0 && &after[0] == &before[0] {
		t.Fatal("expanding a group reused the collapsed rows")
	}
}
