package main

import (
	"github.com/meimingqi222/quick-cleaner/internal/core"
	appui "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func diskFixture(shell appui.Dashboard, files, details bool) (*appui.DiskPage, appui.DiskPageActions) {
	v := core.VolumeFromMountPoint("/")
	space := &appui.DiskPageSpace{Total: 512 << 30, Free: 128 << 30}
	s := &appui.DiskPage{Language: shell.Language, Volume: v, Space: space, Volumes: []appui.DiskPageVolume{{ID: v, Space: space}},
		Loaded: shell.Scanned, Scanning: shell.Scanning, Cleaning: shell.Cleaning, ToRecycleBin: true}
	if files {
		s.Tab = appui.DiskFiles
	}
	if details {
		s.Partial, s.IndexStatus = true, "Coverage unavailable for /System/Private"
	}
	entries := []core.TreeIndexEntry{{IsDir: true, Used: true},
		{Parent: 0, Name: "Projects", IsDir: true, Used: true},
		{Parent: 1, Name: "main.go", Size: 8 << 20, Used: true},
		{Parent: 1, Name: "archive.zip", Size: 40 << 30, Used: true},
		{Parent: 0, Name: "System", IsDir: true, Used: true},
		{Parent: 4, Name: "system.bin", Size: 200 << 30, Used: true},
		{Parent: 0, Name: "movie.mp4", Size: 25 << 30, Used: true},
	}
	if !shell.HasJunk {
		entries = entries[:1]
	}
	tree := core.NewSizeTree(v, entries)
	current := uint32(0)
	selected := map[uint32]bool{}
	refresh := func() {
		s.AtRoot, s.CurrentPath, s.CurrentName = current == 0, tree.PathOf(current), tree.NameOf(current)
		s.CurrentSize, s.UniqueSize = tree.SizeOf(current), tree.SizeOf(0)
		s.Crumbs = []appui.DiskPageCrumb{{ID: 0, Name: "/", Path: "/"}}
		if current != 0 {
			s.Crumbs = append(s.Crumbs, appui.DiskPageCrumb{ID: current, Name: s.CurrentName, Path: s.CurrentPath})
		}
		nodes := tree.Children(current)
		if s.Tab == appui.DiskFiles {
			nodes = tree.LargestFiles(200)
		}
		s.Rows = nil
		for _, node := range nodes {
			protected := node.Name == "System" || node.Name == "system.bin"
			s.Rows = append(s.Rows, appui.DiskPageRow{Node: node, Path: tree.PathOf(node.Idx), Protected: protected, CanEnter: node.IsDir, CanSelect: !protected, Selected: selected[node.Idx]})
		}
		s.SelectedCount, s.SelectedSize = 0, 0
		for idx, on := range selected {
			if on {
				s.SelectedCount++
				s.SelectedSize += tree.SizeOf(idx)
			}
		}
		s.CanSelectView, s.HeaderSelected, s.HeaderMixed = len(s.Rows) > 0, false, false
		count, eligible := 0, 0
		for _, row := range s.Rows {
			if row.CanSelect {
				eligible++
				if row.Selected {
					count++
				}
			}
		}
		s.HeaderSelected, s.HeaderMixed = eligible > 0 && count == eligible, count > 0 && count < eligible
	}
	refresh()
	return s, appui.DiskPageActions{
		SetTab: func(tab appui.DiskTab) { s.Tab = tab; refresh() },
		Navigate: func(idx uint32) {
			if tree.IsDir(idx) {
				current = idx
				refresh()
			}
		},
		Up: func() {
			if parent, ok := tree.ParentOf(current); ok {
				current = parent
				refresh()
			}
		},
		ToggleSelected: func(row appui.DiskPageRow) { selected[row.Node.Idx] = !selected[row.Node.Idx]; refresh() },
		SetViewSelected: func(on bool) {
			for _, row := range s.Rows {
				if row.CanSelect {
					selected[row.Node.Idx] = on
				}
			}
			refresh()
		},
		ClearSelection: func() { clear(selected); refresh() }, SetRecycleBin: func(on bool) { s.ToRecycleBin = on },
		SetCrumbsExpanded: func(on bool) { s.CrumbsExpanded = on },
	}
}
