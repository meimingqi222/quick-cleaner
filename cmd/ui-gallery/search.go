package main

import (
	"github.com/meimingqi222/quick-cleaner/internal/core"
	appui "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func searchFixture(shell appui.Dashboard) (*appui.SearchPage, appui.SearchPageActions) {
	s := &appui.SearchPage{Language: shell.Language, Ready: shell.Scanned, Indexing: shell.Scanning, GroupByKind: true}
	var snapshot []core.TreeSnapshotEntry
	if shell.HasJunk {
		snapshot = []core.TreeSnapshotEntry{
			{Path: "/example/Projects", IsDir: true},
			{Path: "/example/Downloads", IsDir: true},
			{Path: "/example/Documents", IsDir: true},
			{Path: "/example/Pictures", IsDir: true},
			{Path: "/example/Library", IsDir: true},
			{Path: "/example/Projects/main.go", Size: 6400},
			{Path: "/example/Projects/data.sqlite3", Size: 32 << 20},
			{Path: "/example/Downloads/installer.exe", Size: 180 << 20},
			{Path: "/example/Downloads/archive.zip", Size: 2 << 30},
			{Path: "/example/Documents/设计说明.pdf", Size: 12 << 20},
			{Path: "/example/Pictures/holiday.jpg", Size: 8 << 20},
			{Path: "/example/Library/opaque", Size: 800},
		}
	}
	index := core.SizeTreeFromSnapshot(core.VolumeFromMountPoint("/example"), snapshot)
	refresh := func() {
		s.Results = index.Search(s.Query, 500)
		appui.SortSearchResults(s.Results, s.GroupByKind, s.SortColumn, s.Ascending)
	}
	if shell.HasJunk {
		refresh()
	}
	return s, appui.SearchPageActions{
		SetQuery:          func(query string) { s.Query = query; refresh() },
		ToggleGroupByKind: func() { s.GroupByKind = !s.GroupByKind; refresh() },
		SortBy: func(column appui.SearchSortColumn) {
			if s.SortColumn == column {
				s.Ascending = !s.Ascending
			} else {
				s.SortColumn, s.Ascending = column, column != appui.SearchSortSize
			}
			refresh()
		},
	}
}
