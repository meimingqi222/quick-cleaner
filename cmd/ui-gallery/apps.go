package main

import (
	"github.com/meimingqi222/quick-cleaner/internal/core"
	appui "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func appsFixture(shell appui.Dashboard, details bool) (*appui.AppsPage, appui.AppsPageActions) {
	s := &appui.AppsPage{Language: shell.Language, MacOS: shell.MacOS, Scanning: shell.Scanning,
		Sort: core.DefaultAppSortState(), TotalSize: "0 B", FilteredSize: "0 B"}
	var inventory []core.InstalledApp
	if shell.HasJunk {
		inventory = []core.InstalledApp{
			{ID: "editor", Name: "Example Editor", Publisher: "Example Software", EstimatedSize: 800 << 20, LastUsedRaw: core.NowUnixSecs()},
			{ID: "browser", Name: "Example Browser", Publisher: "Example Software", EstimatedSize: 400 << 20, LastUsedRaw: core.NowUnixSecs()},
			{ID: "portable", Name: "Portable Tool", Publisher: "Example Tools", EstimatedSize: 42 << 20, Discovery: &core.AppDiscovery{}},
		}
		for _, app := range inventory {
			s.Apps = append(s.Apps, appui.AppsPageApp{ID: app.ID, Name: app.Name, Publisher: app.Publisher,
				Version: "1.2.3", Size: core.FmtSize(app.EstimatedSize), InstalledDate: "2026-09-01", LastUsed: "2026-10-20",
				Discovered: app.Discovery != nil, CanUninstall: app.Discovery == nil, HasLocation: true})
		}
	}
	refresh := func() {
		s.View = core.FilterAndSortApps(inventory, s.Preset, s.Search, s.Sort)
		var total, filtered uint64
		for _, app := range inventory {
			total += app.EstimatedSize
		}
		for _, i := range s.View {
			filtered += inventory[i].EstimatedSize
		}
		s.TotalSize, s.FilteredSize = core.FmtSize(total), core.FmtSize(filtered)
	}
	refresh()
	if details {
		s.ResidualsOpen, s.ResidualAppName, s.ResidualSize, s.SelectedSize = true, "Example Editor", "24 MB", "0 B"
		s.Residuals = []appui.AppsPageResidual{
			{ID: "cache", Path: `C:\Users\Example\AppData\Local\ExampleEditor\Cache`, Size: "24 MB", Confidence: core.Certain, Kind: core.ResidualDirectory},
		}
	}
	return s, appui.AppsPageActions{
		SetSearch:      func(q string) { s.Search = q; refresh() },
		SetPreset:      func(p core.AppFilterPreset) { s.Preset = p; refresh() },
		SortBy:         func(col core.AppSortColumn) { s.Sort.Toggle(col); refresh() },
		CloseResiduals: func() { s.ResidualsOpen = false },
	}
}
