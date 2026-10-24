package app

import (
	"context"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func (r *Root) scanDeclutter() {
	if r.busy() {
		return
	}
	home, ok := platform.UserHome()
	if !ok {
		r.message("无法确定用户内容目录。", "User content roots are unavailable.")
		return
	}
	// Match the Rust content search scope; overlapping known folders would
	// duplicate candidates and change duplicate/photo grouping.
	roots := []string{home}
	query := core.NewFSIndexEngine(r.tree)
	r.declutter.Scanning = true
	r.message("正在利用索引与多线程深度扫描大文件、重复文件与相似图片...", "Scanning for large files, duplicates and similar photos (indexed)...")
	r.start(false, nil, func(ctx context.Context) func() {
		started := time.Now()
		rules, err := core.CurrentRules()
		if err != nil {
			return func() { r.declutter.Scanning = false; r.failure(err) }
		}
		downloadDir, _ := platform.DownloadsDir()
		downloads := core.ScanDownloads(ctx, rules, downloadDir, query)
		large := core.ScanLargeOldFiles(ctx, rules, roots, rules.Number("engine", "large_min_size", 50000000), query)
		duplicates := core.ScanDuplicateFiles(ctx, rules, roots, query)
		photos := core.ScanSimilarPhotos(ctx, rules, roots, query)
		authority := map[string]core.ArbitraryTarget{}
		add := func(path string) {
			if _, ok := authority[path]; !ok {
				authority[path] = core.CaptureArbitraryTarget(path)
			}
		}
		for _, i := range downloads {
			add(i.Path)
		}
		for _, i := range large {
			add(i.Path)
		}
		for _, g := range duplicates {
			for _, i := range g.Files {
				add(i.Path)
			}
		}
		for _, g := range photos {
			for _, i := range g.Photos {
				add(i.Path)
			}
		}
		thumbnails := loadDeclutterThumbnails(ctx, photos)
		cancelled := ctx.Err()
		elapsed := time.Since(started).Seconds()
		return func() {
			r.declutter.Scanning = false
			if cancelled != nil {
				r.failure(cancelled)
				return
			}
			r.declutter.Scanned = true
			r.declutter.Downloads = downloads
			r.declutter.LargeFiles = large
			r.declutter.Duplicates = duplicates
			r.declutter.Photos = photos
			r.declutter.Thumbnails = thumbnails
			r.declutter.ScanElapsedSecs = &elapsed
			r.declutterAuthority = authority
			r.filterLarge()
		}
	})
}

func (r *Root) filterLarge() {
	r.declutter.FilteredLargeIndices = nil
	for i, f := range r.declutter.LargeFiles {
		if f.Size >= r.declutter.MinSizeFilter && (r.declutter.KindFilter == nil || f.IconType == *r.declutter.KindFilter) {
			r.declutter.FilteredLargeIndices = append(r.declutter.FilteredLargeIndices, i)
		}
	}
}

func (r *Root) declutterActions() view.DeclutterPageActions {
	return view.DeclutterPageActions{SetTab: func(t view.DeclutterTab) { r.declutter.Tab = t }, Scan: r.scanDeclutter,
		CycleSizeFilter: func() {
			values := []uint64{0, 100000000, 500000000, 1000000000}
			next := uint64(0)
			for i, v := range values {
				if v == r.declutter.MinSizeFilter {
					next = values[(i+1)%len(values)]
				}
			}
			r.declutter.MinSizeFilter = next
			r.filterLarge()
		}, CycleKindFilter: func() {
			if r.declutter.KindFilter == nil {
				n := 0
				r.declutter.KindFilter = &n
			} else {
				n := *r.declutter.KindFilter + 1
				if n > 3 {
					r.declutter.KindFilter = nil
				} else {
					r.declutter.KindFilter = &n
				}
			}
			r.filterLarge()
		}, ClearFilters: func() { r.declutter.MinSizeFilter = 0; r.declutter.KindFilter = nil; r.filterLarge() },
		ToggleDownload: func(i int) {
			if !r.busy() && i >= 0 && i < len(r.declutter.Downloads) {
				r.declutter.Downloads[i].Selected = !r.declutter.Downloads[i].Selected
			}
		}, ToggleLargeFile: func(i int) {
			if !r.busy() && i >= 0 && i < len(r.declutter.LargeFiles) {
				r.declutter.LargeFiles[i].Selected = !r.declutter.LargeFiles[i].Selected
			}
		},
		ToggleDuplicate: func(g, i int) {
			if !r.busy() && g >= 0 && g < len(r.declutter.Duplicates) && i >= 0 && i < len(r.declutter.Duplicates[g].Files) {
				r.declutter.Duplicates[g].Files[i].Selected = !r.declutter.Duplicates[g].Files[i].Selected
			}
		}, TogglePhoto: func(g, i int) {
			if !r.busy() && g >= 0 && g < len(r.declutter.Photos) && i >= 0 && i < len(r.declutter.Photos[g].Photos) {
				r.declutter.Photos[g].Photos[i].Selected = !r.declutter.Photos[g].Photos[i].Selected
			}
		},
		TogglePhotoGroup: func(i int) { r.declutter.ExpandedPhotoGroups[i] = !r.declutter.ExpandedPhotoGroups[i] }, KeepNewestDuplicates: func() { r.keepDuplicates(true) }, KeepOldestDuplicates: func() { r.keepDuplicates(false) }, SmartSelectPhotos: r.keepBestPhotos,
		KeepBestPhotos: func(g int) { r.selectPhotos(g, true) }, KeepAllPhotos: func(g int) { r.selectPhotos(g, false) },
		ClearSelection: r.clearDeclutter, RequestRemoveSelected: r.requestDeclutterClean, Reveal: r.reveal, Open: r.open, CopyPath: r.copy,
	}
}

func (r *Root) keepDuplicates(newest bool) {
	if r.busy() {
		return
	}
	for g := range r.declutter.Duplicates {
		files := r.declutter.Duplicates[g].Files
		best := 0
		for i := range files {
			if newest && files[i].ModifiedAtSecs > files[best].ModifiedAtSecs || !newest && files[i].ModifiedAtSecs < files[best].ModifiedAtSecs {
				best = i
			}
		}
		for i := range files {
			files[i].Selected = i != best
		}
	}
}
func (r *Root) selectPhotos(g int, best bool) {
	if r.busy() || g < 0 || g >= len(r.declutter.Photos) {
		return
	}
	for i := range r.declutter.Photos[g].Photos {
		p := &r.declutter.Photos[g].Photos[i]
		p.Selected = best && !p.IsBestShot
	}
}
func (r *Root) keepBestPhotos() {
	for g := range r.declutter.Photos {
		r.selectPhotos(g, true)
	}
}

func (r *Root) declutterPaths(tab view.DeclutterTab) []string {
	var paths []string
	add := func(path string, on bool) {
		if on {
			paths = append(paths, path)
		}
	}
	switch tab {
	case view.DeclutterDownloads:
		for _, i := range r.declutter.Downloads {
			add(i.Path, i.Selected)
		}
	case view.DeclutterLargeFiles:
		for _, i := range r.declutter.LargeFiles {
			add(i.Path, i.Selected)
		}
	case view.DeclutterDuplicates:
		for _, g := range r.declutter.Duplicates {
			for _, i := range g.Files {
				add(i.Path, i.Selected)
			}
		}
	case view.DeclutterSimilarPhotos:
		for _, g := range r.declutter.Photos {
			for _, i := range g.Photos {
				add(i.Path, i.Selected)
			}
		}
	}
	return paths
}

func (r *Root) clearDeclutter(tab view.DeclutterTab) {
	if r.busy() {
		return
	}
	switch tab {
	case view.DeclutterDownloads:
		for i := range r.declutter.Downloads {
			r.declutter.Downloads[i].Selected = false
		}
	case view.DeclutterLargeFiles:
		for i := range r.declutter.LargeFiles {
			r.declutter.LargeFiles[i].Selected = false
		}
	case view.DeclutterDuplicates:
		for g := range r.declutter.Duplicates {
			for i := range r.declutter.Duplicates[g].Files {
				r.declutter.Duplicates[g].Files[i].Selected = false
			}
		}
	case view.DeclutterSimilarPhotos:
		for g := range r.declutter.Photos {
			r.selectPhotos(g, false)
		}
	}
}

func (r *Root) requestDeclutterClean(tab view.DeclutterTab) {
	if r.busy() || !r.declutter.Scanned {
		return
	}
	paths := r.declutterPaths(tab)
	var targets []core.ArbitraryTarget
	for _, path := range paths {
		t, ok := r.declutterAuthority[path]
		if !ok || t.Identity == nil {
			r.message("缺少扫描时的目标身份，必须重新扫描。", "Scan-time target identity is missing; rescan required.")
			return
		}
		targets = append(targets, t)
	}
	// Frozen authority is never recreated at acceptance, even if a file was replaced.
	r.requestArbitraryClean("declutter", targets, func() { r.declutter.Scanned = false; r.scanDeclutter() })
}
