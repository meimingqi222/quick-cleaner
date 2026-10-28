package main

import (
	"github.com/meimingqi222/quick-cleaner/internal/core"
	appui "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func junkFixture(shell appui.Dashboard, details bool) *appui.JunkPage {
	s := &appui.JunkPage{
		Language: shell.Language, Scanned: shell.Scanned, Scanning: shell.Scanning, Cleaning: shell.Cleaning,
		Selected: map[string]bool{}, Expanded: map[core.CategoryID]bool{core.UserCache: true},
		RecommendedSelected: true,
	}
	for _, id := range []core.CategoryID{core.UserCache, core.BrowserCache, core.DevBuild} {
		s.Categories = append(s.Categories, core.CategorySummary{Category: id})
	}
	if shell.HasJunk {
		cache := core.ScanItem{Path: "/Users/fixture/Library/Caches/example", Label: core.NewText("示例应用缓存", "Example application cache"), Size: 250_000_000, FileCount: 1234, Recommended: true}
		browser := core.ScanItem{Path: "/Users/fixture/Library/Caches/browser", Label: core.NewText("浏览器 · Cache", "Browser · Cache"), Size: 500_000_000, FileCount: 5678, Recommended: true}
		build := core.ScanItem{Path: "/Users/fixture/code/example/target", Label: core.NewText("Rust · example", "Rust · example"), Size: 500_000_000, FileCount: 2345}
		for i, item := range []core.ScanItem{cache, browser, build} {
			item.Category = s.Categories[i].Category
			s.Categories[i].Items = []core.ScanItem{item}
			s.Categories[i].TotalSize = item.Size
			s.Selected[item.Path] = item.Recommended
		}
		s.Categories[2].Partial = true
	}
	if details {
		s.ShowExclusions, s.ShowFailedDetails = true, true
		s.Exclusions = []string{"/Users/fixture/Projects/keep"}
		s.FailedDetails = []string{core.NewText("系统占用 · 24.0 MB (42 个文件) · /Users/fixture/Library/Caches/busy", "In use · 24.0 MB (42 files) · /Users/fixture/Library/Caches/busy").Get(s.Language)}
	}
	return s
}
