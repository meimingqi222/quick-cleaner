package app

import (
	"strings"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

// Default selection is the rule's recommendation and Select All means all;
// occupancy is only a badge. The cleaner skips locked leaves at execution.
func TestJunkBusyItemsStayRecommendedAndSelectable(t *testing.T) {
	r, _ := testRoot(t)
	r.junk.Scanned = true
	r.junk.Categories = []core.CategorySummary{{Category: core.UserCache, TotalSize: 950, Items: []core.ScanItem{
		{Path: "/cache/clear", Size: 100, Recommended: true},
		{Path: "/cache/busy", Size: 300, Recommended: true},
		{Path: "/cache/unknown", Size: 500},
		{Path: "/cache/optional", Size: 50},
	}}}
	core.ApplyBusy(r.junk.Categories, map[string]core.Busy{"/cache/busy": {Open: true}, "/cache/unknown": {Unknown: true}})
	r.selectJunk(view.JunkRecommended)
	r.junkSummary()
	if !r.junk.Selected["/cache/clear"] || !r.junk.Selected["/cache/busy"] || r.junk.Selected["/cache/unknown"] || r.junk.Selected["/cache/optional"] {
		t.Errorf("recommended selection depends on occupancy: %v", r.junk.Selected)
	}
	if r.dashboard.Cleanable != core.FmtSize(950) {
		t.Errorf("occupied bytes dropped from the estimate: %s", r.dashboard.Cleanable)
	}
	r.selectJunk(view.JunkAll)
	for _, path := range []string{"/cache/clear", "/cache/busy", "/cache/unknown", "/cache/optional"} {
		if !r.junk.Selected[path] {
			t.Errorf("select all skipped %s: %v", path, r.junk.Selected)
		}
	}
	r.junkActions().ToggleItem("/cache/busy")
	if r.junk.Selected["/cache/busy"] {
		t.Error("busy item cannot be toggled manually")
	}
}

func TestJunkLateOccupancyKeepsBadgeOnlyAndRealErrors(t *testing.T) {
	r, _ := testRoot(t)
	r.junk.Scanned = true
	r.junk.Categories = []core.CategorySummary{{Category: core.UserCache, Items: []core.ScanItem{
		{Path: "/cache/busy", Size: 300, Recommended: true},
		{Path: "/cache/unknown", Size: 500, Recommended: true},
		{Path: "/cache/denied", Size: 100, Recommended: true},
	}}}
	r.selectJunk(view.JunkAll)
	report := core.CleanReport{
		Failed: []core.CleanFailure{{Path: "/cache/busy"}, {Path: "/cache/unknown"}, {Path: "/cache/denied"}},
		FailInfo: []core.TargetFailInfo{
			{Path: "/cache/busy", Reason: core.FailInUse, FailedLeaves: 1, SkippedLeaves: 1},
			{Path: "/cache/unknown", Reason: core.FailUnverified, FailedLeaves: 1, SkippedLeaves: 1},
			{Path: "/cache/denied", Reason: core.FailAccessDenied, FailedLeaves: 1},
		},
		RemainingLive: []core.RemainingLive{{Path: "/cache/busy", Bytes: 300}, {Path: "/cache/unknown", Bytes: 500}, {Path: "/cache/denied", Bytes: 100}},
	}
	r.applyJunkCleanResult([]string{"/cache/busy", "/cache/unknown", "/cache/denied"}, report)
	r.junkSummary()
	for _, item := range r.junk.Categories[0].Items {
		if item.Path == "/cache/denied" {
			if item.Busy != nil {
				t.Error("real access error reclassified as occupancy")
			}
			continue
		}
		if !item.CleanupAvailable() || !item.Recommended {
			t.Errorf("late occupancy changed eligibility: %+v", item)
		}
		if item.Busy == nil {
			t.Errorf("late occupancy badge missing on %s", item.Path)
		}
	}
	if r.dashboard.Cleanable != core.FmtSize(900) {
		t.Errorf("remaining estimate = %s", r.dashboard.Cleanable)
	}
}

func TestJunkMixedLeafFailuresKeepErrorAndExactHistory(t *testing.T) {
	r, _ := testRoot(t)
	r.settings.Language = core.English
	var skipped, failed uint
	r.record = func(_ string, _ []string, _, s, f uint, _ uint64) {
		skipped, failed = s, f
	}
	report := core.CleanReport{
		Failed:   []core.CleanFailure{{Path: "/cache/busy1"}, {Path: "/cache/busy2"}, {Path: "/cache/denied"}},
		FailInfo: []core.TargetFailInfo{{Path: "/cache", Reason: core.FailAccessDenied, FailedLeaves: 3, SkippedLeaves: 2}},
	}
	r.finishCleanup("clean", nil, report, core.NewCleanProgress(0, 0), nil)
	if failed != 1 || skipped != 2 {
		t.Errorf("history failed=%d skipped=%d; want 1, 2", failed, skipped)
	}
	if len(r.junk.FailedDetails) != 1 || len(r.junk.SkippedDetails) != 0 || !strings.Contains(r.junk.FailedDetails[0].Text, "access denied") {
		t.Errorf("mixed target lost its failure: failed=%v skipped=%v", r.junk.FailedDetails, r.junk.SkippedDetails)
	}
	if !strings.Contains(r.dashboard.StatusText, "partially complete") {
		t.Errorf("mixed target reported as clean: %q", r.dashboard.StatusText)
	}
}

func TestJunkDetailsSeparateSkippedInUseFromRealFailures(t *testing.T) {
	r, _ := testRoot(t)
	r.settings.Language = core.English
	var recordedSkipped, recordedFailed uint
	r.record = func(_ string, _ []string, _, skipped, failed uint, _ uint64) {
		recordedSkipped, recordedFailed = skipped, failed
	}
	r.junk.ReleaseEligible = map[string]bool{"/fixture/cache": true, "/fixture/system": true}
	report := core.CleanReport{
		FailInfo: []core.TargetFailInfo{
			{Path: "/fixture/cache", Reason: core.FailInUse, FailedLeaves: 1, SkippedLeaves: 1},
			{Path: "/fixture/system", Reason: core.FailInUse, FailedLeaves: 1, SkippedLeaves: 1},
			{Path: "/fixture/huge", Reason: core.FailUnverified, FailedLeaves: 1, SkippedLeaves: 1},
			{Path: "/fixture/denied", Reason: core.FailAccessDenied, FailedLeaves: 1},
		},
		Failed: []core.CleanFailure{
			{Path: "/fixture/cache"}, {Path: "/fixture/system"}, {Path: "/fixture/huge"}, {Path: "/fixture/denied"},
		},
		RemainingLive: []core.RemainingLive{
			{Path: "/fixture/cache", Bytes: 100, FileCount: 2},
			{Path: "/fixture/system", Bytes: 200, FileCount: 3},
			{Path: "/fixture/huge", Bytes: 300, FileCount: 4},
			{Path: "/fixture/denied", Bytes: 400, FileCount: 5},
		},
	}
	owners := map[string][]platform.Occupant{
		"/fixture/cache":  {{PID: 42, Name: "Edge"}},
		"/fixture/system": {{PID: 4, Name: "svchost.exe", System: true}},
	}
	r.finishCleanup("clean", nil, report, core.NewCleanProgress(0, 0), owners)

	if len(r.junk.SkippedDetails) != 3 || len(r.junk.FailedDetails) != 1 {
		t.Fatalf("skipped=%d failed=%d", len(r.junk.SkippedDetails), len(r.junk.FailedDetails))
	}
	if text := r.junk.SkippedDetails[0].Text; !strings.Contains(text, "Edge") || !strings.Contains(text, "/fixture/cache") {
		t.Fatalf("app attribution missing: %q", text)
	}
	if !r.junk.SkippedDetails[0].Release {
		t.Fatal("identified user app did not keep its retry entry")
	}
	if text := r.junk.SkippedDetails[1].Text; !strings.Contains(text, "System, another user, or a protected process") {
		t.Fatalf("system owner not explained: %q", text)
	}
	if r.junk.SkippedDetails[1].Release {
		t.Fatal("system owner acquired a termination retry")
	}
	if text := r.junk.SkippedDetails[2].Text; !strings.Contains(text, "cleanup authority unverified") {
		t.Fatalf("unverified target not explained: %q", text)
	}
	if text := r.junk.FailedDetails[0].Text; !strings.Contains(text, "access denied") || !strings.Contains(text, "/fixture/denied") {
		t.Fatalf("real failure lost its alarm copy: %q", text)
	}
	if !strings.Contains(r.dashboard.StatusText, "partially complete") ||
		!strings.Contains(r.dashboard.StatusText, "1 target(s) could not be cleaned") ||
		!strings.Contains(r.dashboard.StatusText, "3 target(s) in use or unverified; skipped") {
		t.Fatalf("mixed result reported as clean: %q", r.dashboard.StatusText)
	}
	if recordedFailed != 1 || recordedSkipped != 3 {
		t.Fatalf("history recorded failed=%d skipped=%d; want failed=1 skipped=3", recordedFailed, recordedSkipped)
	}
}

func TestJunkOnlyInUseTargetsReportSkippedNotFailed(t *testing.T) {
	r, _ := testRoot(t)
	r.settings.Language = core.English
	report := core.CleanReport{
		FailInfo:      []core.TargetFailInfo{{Path: "/fixture/busy", Reason: core.FailInUse, FailedLeaves: 1, SkippedLeaves: 1}},
		Failed:        []core.CleanFailure{{Path: "/fixture/busy"}},
		RemainingLive: []core.RemainingLive{{Path: "/fixture/busy", Bytes: 100, FileCount: 2}},
	}
	r.finishCleanup("clean", nil, report, core.NewCleanProgress(0, 0), nil)
	if len(r.junk.FailedDetails) != 0 || len(r.junk.SkippedDetails) != 1 {
		t.Fatalf("in-use target classified as failure: failed=%d skipped=%d", len(r.junk.FailedDetails), len(r.junk.SkippedDetails))
	}
	if !strings.Contains(r.dashboard.StatusText, "in use; skipped") || strings.Contains(r.dashboard.StatusText, "partially complete") {
		t.Fatalf("in-use-only status = %q", r.dashboard.StatusText)
	}
}
