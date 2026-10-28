package core

import (
	"context"
	"fmt"
	"log"
	"os"
	"runtime"
	"sort"
	"strings"
	"sync"
	"time"
)

type ScanItem struct {
	Plans                         []*CleanupPlan
	Operation                     Operation
	Disposal                      Disposal
	Rule                          RuleRef
	Path                          string
	Label                         Text
	Size, FileCount, LastModified uint64
	Category                      CategoryID
	Recommended                   bool
	Busy                          *Busy
	Identity                      *TargetIdentity
}

func (item ScanItem) FreezePlan() ScanItem {
	if len(item.Plans) == 0 {
		plan := NewCleanupPlan(item.Rule, []PlannedTarget{{Path: item.Path, Operation: item.Operation, Identity: item.Identity, Disposal: item.Disposal}})
		item.Rule = plan.rule
		item.Plans = []*CleanupPlan{plan}
	}
	return item
}

type CategorySummary struct {
	Category  CategoryID
	TotalSize uint64
	Items     []ScanItem
	Partial   bool
}

// Indexed and live scans freeze the same identity independently of weighing.
// A failed identity probe is not authorization to clean a filesystem target.
func ScanFixed(ctx context.Context, targets []ScanTarget, query FileIndexQuery) []CategorySummary {
	started := time.Now()
	if query == nil {
		query = NewFSIndexEngine(nil)
	}
	items := make([]*ScanItem, len(targets))
	elapsed := make([]time.Duration, len(targets))
	work := make(chan int)
	var workers sync.WaitGroup
	for range min(len(targets), max(1, runtime.GOMAXPROCS(0))) {
		workers.Go(func() {
			for i := range work {
				if ctx.Err() != nil {
					continue
				}
				targetStarted := time.Now()
				t := targets[i]
				if IsWhitelisted(t.Path) {
					elapsed[i] = time.Since(targetStarted)
					continue
				}
				item := ScanItem{Rule: t.Rule, Operation: t.Operation.ForScannedPath(t.Path), Disposal: t.Disposal, Path: t.Path, Label: t.Label, Category: t.Category, Recommended: t.Recommended && !HasWhitelistEntryUnder(t.Path)}
				if IsVirtualPath(t.Path) {
					if t.SizeHint != nil {
						item.Size = *t.SizeHint
					}
				} else {
					md, err := os.Lstat(t.Path)
					if err != nil || ruleEvidenceLink(md) {
						elapsed[i] = time.Since(targetStarted)
						continue
					}
					identity, ok := CaptureIdentity(t.Path)
					if !ok {
						elapsed[i] = time.Since(targetStarted)
						continue
					}
					measurement, ok := query.MeasurePath(ctx, t.Path)
					if !ok {
						elapsed[i] = time.Since(targetStarted)
						continue
					}
					item.Identity = &identity
					item.Size, item.FileCount, item.LastModified = measurement.Size, measurement.Files, measurement.NewestMtime
				}
				frozen := item.FreezePlan()
				items[i] = &frozen
				elapsed[i] = time.Since(targetStarted)
			}
		})
	}
	for i := range targets {
		if ctx.Err() != nil {
			break
		}
		work <- i
	}
	close(work)
	workers.Wait()
	type timedScanItem struct {
		item    ScanItem
		elapsed time.Duration
	}
	var results []ScanItem
	timed := make([]timedScanItem, 0, len(items))
	var total uint64
	for i, item := range items {
		if item == nil {
			continue
		}
		results = append(results, *item)
		timed = append(timed, timedScanItem{*item, elapsed[i]})
		total = saturatingAdd(total, item.Size)
	}
	sort.SliceStable(timed, func(i, j int) bool { return timed[i].elapsed > timed[j].elapsed })
	if len(timed) > 5 {
		timed = timed[:5]
	}
	slow := make([]string, len(timed))
	for i, item := range timed {
		slow[i] = fmt.Sprintf("%s %s %s", item.elapsed, FmtSize(item.item.Size), item.item.Path)
	}
	log.Printf("阶段一 scan_fixed 完成：%s，%d/%d 个目标命中，合计 %s；最慢 5 个：%s", time.Since(started), len(results), len(targets), FmtSize(total), strings.Join(slow, " | "))
	return AggregateScanItems(results)
}

func AggregateScanItems(items []ScanItem) []CategorySummary {
	out := make([]CategorySummary, 0, len(AllCategories()))
	for _, category := range AllCategories() {
		summary := CategorySummary{Category: category}
		for _, item := range items {
			if item.Category == category {
				summary.Items = append(summary.Items, item)
			}
		}
		sort.SliceStable(summary.Items, func(i, j int) bool { return summary.Items[i].Size > summary.Items[j].Size })
		for _, item := range summary.Items {
			summary.TotalSize = saturatingAdd(summary.TotalSize, item.Size)
		}
		out = append(out, summary)
	}
	return out
}

// DropVanishedDiscovered removes discovered paths that no longer exist. It
// stats every item, so it runs on the discovery worker, not the main thread:
// phase two can take long enough for the user to delete some of them.
func DropVanishedDiscovered(items []ScanItem) []ScanItem {
	kept := items[:0:0]
	for _, item := range items {
		if _, err := os.Stat(item.Path); err == nil {
			kept = append(kept, item)
		}
	}
	return kept
}

// MergeDiscovered runs on the main thread and touches no filesystem: pass
// items through DropVanishedDiscovered first. Whitelist and overlap checks
// stay here because exclusions and fixed targets can change meanwhile.
func MergeDiscovered(cats []CategorySummary, items []ScanItem, partial bool) {
	var fixed []string
	for _, cat := range cats {
		for _, item := range cat.Items {
			fixed = append(fixed, Norm(item.Path))
		}
	}
	byCategory := map[CategoryID][]ScanItem{}
	for _, item := range items {
		if IsWhitelisted(item.Path) {
			continue
		}
		norm := Norm(item.Path)
		overlap := false
		for _, path := range fixed {
			if atOrUnder(path, norm) || atOrUnder(norm, path) {
				overlap = true
				break
			}
		}
		if overlap {
			continue
		}
		item.Recommended = item.Recommended && !HasWhitelistEntryUnder(item.Path)
		byCategory[item.Category] = append(byCategory[item.Category], item)
	}
	for i := range cats {
		cat := &cats[i]
		cat.Partial = cat.Partial || partial && cat.Category.IsDiscovered()
		cat.Items = append(cat.Items, byCategory[cat.Category]...)
		if cat.Category.IsDiscovered() {
			sort.SliceStable(cat.Items, func(i, j int) bool { return cat.Items[i].Size > cat.Items[j].Size })
		}
		cat.TotalSize = 0
		for _, item := range cat.Items {
			cat.TotalSize = saturatingAdd(cat.TotalSize, item.Size)
		}
	}
}

// Failed targets remain visible and retryable; successful attempts disappear
// without another expensive discovery scan.
func ApplyCleanResult(cats []CategorySummary, attempted, failed []string) []string {
	attempts, failures := map[string]bool{}, map[string]bool{}
	for _, path := range attempted {
		attempts[path] = true
	}
	for _, path := range failed {
		failures[path] = true
	}
	var cleared []string
	for i := range cats {
		cat := &cats[i]
		kept := cat.Items[:0]
		cat.TotalSize = 0
		for _, item := range cat.Items {
			if attempts[item.Path] && !failures[item.Path] {
				cleared = append(cleared, item.Path)
				continue
			}
			kept = append(kept, item)
			cat.TotalSize = saturatingAdd(cat.TotalSize, item.Size)
		}
		cat.Items = kept
	}
	return cleared
}
