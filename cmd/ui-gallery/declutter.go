package main

import (
	"fmt"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	appui "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func declutterFixture(shell appui.Dashboard) (*appui.DeclutterPage, appui.DeclutterPageActions) {
	s := &appui.DeclutterPage{Language: shell.Language, Scanning: shell.Scanning, Scanned: shell.Scanned, Cleaning: shell.Cleaning, ExpandedPhotoGroups: map[int]bool{}, MinSizeFilter: 100_000_000}
	if shell.HasJunk {
		secs := 1.2
		s.ScanElapsedSecs = &secs
		s.Downloads = []core.DownloadItem{
			{Filename: "installer.dmg", Path: "/example/Downloads/installer.dmg", Size: 180 << 20, Kind: core.NewText("安装包", "Installer"), DownloadedAtStr: core.FormatAgeText(20)},
			{Filename: "archive.zip", Path: "/example/Downloads/archive.zip", Size: 500 << 20, Kind: core.NewText("压缩包", "Archive"), DownloadedAtStr: core.FormatAgeText(90)},
		}
		s.LargeFiles = []core.LargeFileItem{
			{Filename: "movie.mp4", Path: "/example/Videos/movie.mp4", PathDisplay: "/example/Videos/movie.mp4", Size: 2 << 30, IconType: 2, Kind: core.NewText("视频影音", "Videos"), LastAccessedStr: core.FormatAgeText(365)},
			{Filename: "backup.iso", Path: "/example/Backups/backup.iso", PathDisplay: "/example/Backups/backup.iso", Size: 700 << 20, IconType: 3, Kind: core.NewText("压缩镜像", "Archives"), LastAccessedStr: core.FormatAgeText(180)},
		}
		s.Duplicates = []core.DuplicateGroup{{ID: "example", Filename: "report.pdf", SizePerCopy: 12 << 20, Category: core.NewText("文档", "Document"), Files: []core.DuplicateFileItem{
			{Path: "/example/Documents/report.pdf", PathDisplay: "/example/Documents/report.pdf", IsOriginal: true, ModifiedAtSecs: 1, ModifiedAtStr: "2025-10-01"},
			{Path: "/example/Downloads/report.pdf", PathDisplay: "/example/Downloads/report.pdf", Selected: true, ModifiedAtSecs: 2, ModifiedAtStr: "2026-10-01"},
		}}}
		s.Photos = []core.PhotoGroup{{IndexStr: "01", Title: core.NewText("旅行连拍", "Travel burst")}}
		for i := range 7 {
			s.Photos[0].Photos = append(s.Photos[0].Photos, core.PhotoItem{ID: fmt.Sprint(i), Filename: fmt.Sprintf("holiday-%d.jpg", i+1), Path: fmt.Sprintf("/example/Pictures/holiday-%d.jpg", i+1), Dimensions: [2]uint32{2400, 1600}, Size: 2 << 20, IsBestShot: i == 0, Selected: i > 0, DateStr: "2026-10-01", BGGradientSeed: 0x67829a})
		}
	}
	refresh := func() {
		s.FilteredLargeIndices = nil
		for i, item := range s.LargeFiles {
			if item.Size >= s.MinSizeFilter && (s.KindFilter == nil || item.IconType == *s.KindFilter) {
				s.FilteredLargeIndices = append(s.FilteredLargeIndices, i)
			}
		}
	}
	refresh()
	return s, appui.DeclutterPageActions{
		SetTab:           func(tab appui.DeclutterTab) { s.Tab = tab },
		ToggleDownload:   func(i int) { s.Downloads[i].Selected = !s.Downloads[i].Selected },
		ToggleLargeFile:  func(i int) { s.LargeFiles[i].Selected = !s.LargeFiles[i].Selected },
		ToggleDuplicate:  func(g, i int) { s.Duplicates[g].Files[i].Selected = !s.Duplicates[g].Files[i].Selected },
		TogglePhoto:      func(g, i int) { s.Photos[g].Photos[i].Selected = !s.Photos[g].Photos[i].Selected },
		TogglePhotoGroup: func(g int) { s.ExpandedPhotoGroups[g] = !s.ExpandedPhotoGroups[g] },
		KeepBestPhotos: func(g int) {
			for i := range s.Photos[g].Photos {
				s.Photos[g].Photos[i].Selected = !s.Photos[g].Photos[i].IsBestShot
			}
		},
		KeepAllPhotos: func(g int) {
			for i := range s.Photos[g].Photos {
				s.Photos[g].Photos[i].Selected = false
			}
		},
		ClearSelection: func(tab appui.DeclutterTab) {
			if tab == appui.DeclutterLargeFiles {
				for i := range s.LargeFiles {
					s.LargeFiles[i].Selected = false
				}
			}
		},
		CycleSizeFilter: func() {
			switch s.MinSizeFilter {
			case 100_000_000:
				s.MinSizeFilter = 500_000_000
			case 500_000_000:
				s.MinSizeFilter = 1_000_000_000
			case 1_000_000_000:
				s.MinSizeFilter = 0
			default:
				s.MinSizeFilter = 100_000_000
			}
			refresh()
		},
		CycleKindFilter: func() {
			if s.KindFilter == nil {
				v := 0
				s.KindFilter = &v
			} else if *s.KindFilter == 3 {
				s.KindFilter = nil
			} else {
				*s.KindFilter++
			}
			refresh()
		},
		ClearFilters: func() { s.MinSizeFilter = 0; s.KindFilter = nil; refresh() },
	}
}
