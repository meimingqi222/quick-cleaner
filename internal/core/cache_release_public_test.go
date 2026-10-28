package core_test

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func TestCacheReleasePublicEligibilityAndFailClosedAPI(t *testing.T) {
	root := t.TempDir()
	id, ok := core.CaptureIdentity(root)
	if !ok {
		t.Fatal("missing identity")
	}
	base := core.ScanItem{Path: root, Category: core.UserCache, Recommended: true, Operation: core.Operation{Kind: core.OpContents}, Disposal: core.Permanent, Identity: &id, Rule: core.EngineRuleRef()}.FreezePlan()
	if !core.CacheReleaseEligible(base) {
		t.Fatal("recommended cache rejected")
	}
	for _, category := range []core.CategoryID{core.UserCache, core.BrowserCache, core.Logs} {
		item := base
		item.Category = category
		if !core.CacheReleaseEligible(item) {
			t.Fatal(category)
		}
	}
	for _, change := range []func(*core.ScanItem){
		func(i *core.ScanItem) { i.Category = core.AiAgents }, func(i *core.ScanItem) { i.Recommended = false }, func(i *core.ScanItem) { i.Busy = &core.Busy{Unknown: true} }, func(i *core.ScanItem) { i.Operation = core.Operation{Kind: core.OpTree} }, func(i *core.ScanItem) { i.Disposal = core.RecycleBin }, func(i *core.ScanItem) { i.Identity = nil }, func(i *core.ScanItem) { i.Plans = nil },
	} {
		item := base
		change(&item)
		if core.CacheReleaseEligible(item) {
			t.Fatal("unsafe eligibility")
		}
		if _, err := core.PrepareCacheRelease(item); err == nil {
			t.Fatal("unsafe prepare")
		}
	}
	busy := base
	busy.Recommended = false
	busy.Busy = &core.Busy{RecommendedBeforeBusy: true, Open: true}
	if !core.CacheReleaseEligible(busy) {
		t.Fatal("busy lost original recommendation")
	}
	path := filepath.Join(root, "cache")
	if err := os.Mkdir(path, 0700); err != nil {
		t.Fatal(err)
	}
	id, ok = core.CaptureIdentity(path)
	if !ok {
		t.Fatal("missing identity")
	}
	base.Path = path
	base.Identity = &id
	base.Plans = nil
	base = base.FreezePlan()
	if err := os.Rename(path, path+"-old"); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(path, 0700); err != nil {
		t.Fatal(err)
	}
	if _, err := core.PrepareCacheRelease(base); err == nil {
		t.Fatal("changed root accepted")
	}
	if _, err := core.ExecuteCacheRelease(core.CacheReleasePlan{}, true, &core.CleanProgress{}); err == nil {
		t.Fatal("unprepared capability accepted")
	}
}
