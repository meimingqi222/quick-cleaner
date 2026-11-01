package app

import (
	"strings"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/core"
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
		RemainingLive: []core.RemainingLive{{Path: "/cache/busy", Bytes: 300}, {Path: "/cache/unknown", Bytes: 500}, {Path: "/cache/denied", Bytes: 75, FileCount: 3}},
	}
	r.applyJunkCleanResult([]string{"/cache/busy", "/cache/unknown", "/cache/denied"}, report)
	r.junkSummary()
	items := r.junk.Categories[0].Items
	if len(items) != 1 || items[0].Path != "/cache/denied" {
		t.Fatalf("expected active-skip targets collapsed and real error retained, got %+v", items)
	}
	if items[0].Busy != nil {
		t.Error("real access error reclassified as occupancy")
	}
	if items[0].Size != 75 || items[0].FileCount != 3 {
		t.Errorf("remaining live size not refreshed on error item: %+v", items[0])
	}
	if r.junk.Selected["/cache/denied"] {
		t.Error("failed target remained selected after cleanup")
	}
	if r.dashboard.Cleanable != core.FmtSize(75) {
		t.Errorf("remaining estimate = %s", r.dashboard.Cleanable)
	}
}

// Invalid operations record unverified failures without granting deletion.
func skippedProgress(paths ...string) *core.CleanProgress {
	p := core.NewCleanProgress(0, 0)
	var targets []core.CleanTarget
	for _, path := range paths {
		targets = append(targets, core.CleanTarget{Path: path})
	}
	core.CleanTargets(targets, p)
	return p
}

func TestJunkOverlappingTargetsDoNotHideHistoryFailures(t *testing.T) {
	r, _ := testRoot(t)
	var skipped, failed uint
	r.record = func(_ string, _ []string, _, s, f uint, _ uint64) { skipped, failed = s, f }
	report := core.CleanReport{
		Failed: []core.CleanFailure{{Path: "/cache/sub/locked"}, {Path: "/cache/denied"}},
		FailInfo: []core.TargetFailInfo{
			{Path: "/cache", Reason: core.FailAccessDenied, FailedLeaves: 2, SkippedLeaves: 1},
			{Path: "/cache/sub", Reason: core.FailInUse, FailedLeaves: 1, SkippedLeaves: 1},
		},
	}
	r.finishCleanup("clean", nil, report, skippedProgress("/cache/sub/locked"))
	if skipped != 1 || failed != 1 {
		t.Fatalf("overlapping targets changed history: skipped=%d failed=%d; want 1/1", skipped, failed)
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
	r.finishCleanup("clean", nil, report, skippedProgress("/cache/busy1", "/cache/busy2"))
	if failed != 1 || skipped != 2 {
		t.Errorf("history failed=%d skipped=%d; want 1, 2", failed, skipped)
	}
	if len(r.junk.FailedDetails) != 1 || r.junk.ShowFailedDetails || !strings.Contains(r.junk.FailedDetails[0].Text, "access denied") {
		t.Errorf("mixed target lost its collapsed failure: show=%v failed=%v", r.junk.ShowFailedDetails, r.junk.FailedDetails)
	}
	if r.junk.CleanBanner == nil || !r.junk.CleanBanner.SkippedActive {
		t.Errorf("expected CleanBanner with SkippedActive, got %+v", r.junk.CleanBanner)
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
	r.finishCleanup("clean", nil, report, skippedProgress("/fixture/cache", "/fixture/system", "/fixture/huge"))

	if len(r.junk.FailedDetails) != 1 || r.junk.ShowFailedDetails {
		t.Fatalf("failed=%d show=%v", len(r.junk.FailedDetails), r.junk.ShowFailedDetails)
	}
	if text := r.junk.FailedDetails[0].Text; !strings.Contains(text, "access denied") || !strings.Contains(text, "/fixture/denied") {
		t.Fatalf("real failure lost its alarm copy: %q", text)
	}
	if r.junk.CleanBanner == nil || !r.junk.CleanBanner.SkippedActive {
		t.Fatalf("clean banner missing active-skip note: %+v", r.junk.CleanBanner)
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
	r.finishCleanup("clean", nil, report, core.NewCleanProgress(0, 0))
	if len(r.junk.FailedDetails) != 0 {
		t.Fatalf("in-use target classified as failure: failed=%d", len(r.junk.FailedDetails))
	}
	if r.junk.CleanBanner == nil || !r.junk.CleanBanner.SkippedActive {
		t.Fatalf("clean banner missing skipped-active note: %+v", r.junk.CleanBanner)
	}
	if !strings.Contains(r.dashboard.StatusText, "in use; skipped") || strings.Contains(r.dashboard.StatusText, "partially complete") {
		t.Fatalf("in-use-only status = %q", r.dashboard.StatusText)
	}
}

func TestJunkPartialLeafFailureCollapsesInUseTailAndKeepsRealError(t *testing.T) {
	r, _ := testRoot(t)
	inUseTail, denied, done := "/fixture/cache", "/fixture/denied", "/fixture/done"
	r.junk.Categories = []core.CategorySummary{{Category: core.UserCache, Items: []core.ScanItem{
		{Path: inUseTail, Size: 100, FileCount: 10},
		{Path: denied, Size: 80, FileCount: 4},
		{Path: done, Size: 20, FileCount: 2},
	}}}
	r.junk.Selected = map[string]bool{inUseTail: true, denied: true, done: true}
	report := core.CleanReport{
		FailInfo: []core.TargetFailInfo{
			{Path: inUseTail, Reason: core.FailInUse, FailedLeaves: 1, SkippedLeaves: 1},
			{Path: denied, Reason: core.FailAccessDenied, FailedLeaves: 1},
		},
		Failed: []core.CleanFailure{{Path: inUseTail + "/sub/locked.bin"}, {Path: denied + "/secret.dat"}},
		RemainingLive: []core.RemainingLive{
			{Path: inUseTail, Bytes: 9, FileCount: 1},
			{Path: denied, Bytes: 12, FileCount: 1},
		},
	}
	r.applyJunkCleanResult([]string{inUseTail, denied, done}, report)
	items := r.junk.Categories[0].Items
	if len(items) != 1 || items[0].Path != denied {
		t.Fatalf("expected only real error retained, got %+v", items)
	}
	if items[0].Size != 12 || items[0].FileCount != 1 {
		t.Fatalf("remaining size not refreshed: size=%d files=%d", items[0].Size, items[0].FileCount)
	}
	if len(r.junk.Selected) != 0 {
		t.Fatalf("attempted targets must be deselected after cleanup: %v", r.junk.Selected)
	}
}
