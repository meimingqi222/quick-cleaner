package ui

import (
	"fmt"
	"image"
	"image/color"
	"image/png"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func declutterFixture() DeclutterPage {
	secs := 1.2
	s := DeclutterPage{Language: English, Scanned: true, ScanElapsedSecs: &secs, ExpandedPhotoGroups: map[int]bool{}, FilteredLargeIndices: []int{1}, Platform: "windows",
		Downloads:  []core.DownloadItem{{Filename: "installer.dmg", Path: "/unreadable/installer.dmg", Size: 40_000_000, Selected: true, Kind: core.NewText("安装包", "Installer"), DownloadedAtStr: core.NewText("昨天", "Yesterday")}},
		LargeFiles: []core.LargeFileItem{{Filename: "hidden.iso", Path: "/unreadable/hidden.iso", Size: 900_000_000, Selected: true}, {Filename: "movie.mp4", Path: "/unreadable/movie.mp4", PathDisplay: "/unreadable/movie.mp4", Size: 200_000_000, Kind: core.NewText("视频影音", "Videos"), LastAccessedStr: core.NewText("1 年前", "1 year ago"), Selected: true}},
		Duplicates: []core.DuplicateGroup{{Filename: "report.pdf", SizePerCopy: 1_000_000, Files: []core.DuplicateFileItem{{Path: "/unreadable/original.pdf", PathDisplay: "/unreadable/original.pdf", IsOriginal: true, ModifiedAtStr: "2025-01-01"}, {Path: "/unreadable/copy.pdf", PathDisplay: "/unreadable/copy.pdf", Selected: true, ModifiedAtStr: "2026-01-01"}}}},
		Photos:     []core.PhotoGroup{{IndexStr: "01", Title: core.NewText("旅行连拍", "Travel burst")}},
	}
	for i := range 7 {
		s.Photos[0].Photos = append(s.Photos[0].Photos, core.PhotoItem{Filename: fmt.Sprintf("photo-%d.jpg", i), Path: fmt.Sprintf("/unreadable/photo-%d.jpg", i), Dimensions: [2]uint32{2400, 1600}, Size: 2_000_000, IsBestShot: i == 0, Selected: i != 0, DateStr: "2026-10-01", BGGradientSeed: 0x67829a})
	}
	im := image.NewRGBA(image.Rect(0, 0, 40, 30))
	for y := range 30 {
		for x := range 40 {
			im.Set(x, y, color.RGBA{uint8(x * 6), uint8(y * 8), 120, 255})
		}
	}
	s.Thumbnails = map[string]*ui.Bitmap{s.Photos[0].Photos[0].Path: ui.NewBitmap(im)}
	return s
}

func declutterTester(s *DeclutterPage, a DeclutterPageActions) *ui.Tester {
	return ui.NewTester(func(c *ui.Context) {
		ui.Column(c).Fill().Background(background).TextColor(text).Children(func() { s.RenderContent(c, a) })
	}, 1100, 850)
}

func TestDeclutterNavigationScanAndReadOnlySnapshots(t *testing.T) {
	s := declutterFixture()
	scans := 0
	a := DeclutterPageActions{SetTab: func(tab DeclutterTab) { s.Tab = tab }, Scan: func() { scans++ }}
	tt := declutterTester(&s, a)
	if !tt.HasText("Scan took 1.2s") || !tt.HasText("Downloads Folder") {
		t.Fatal(tt.Texts())
	}
	if r, ok := tt.Find("Large & Old Files"); !ok || r.W < 300 || r.X+r.W > 1100 {
		t.Fatalf("fixed-width large-file bento card collapsed or overflowed: %+v", r)
	}
	if err := tt.Click("Start Smart Scan"); err != nil {
		t.Fatal(err)
	}
	if scans != 1 {
		t.Fatal(scans)
	}
	if err := tt.Click("Downloads"); err != nil {
		t.Fatal(err)
	}
	if s.Tab != DeclutterDownloads || !tt.HasText("Review Downloads") {
		t.Fatal(tt.Texts())
	}
	for _, tab := range []DeclutterTab{DeclutterSimilarPhotos, DeclutterDuplicates, DeclutterLargeFiles} {
		if err := tt.Click(s.title(tab)); err != nil {
			t.Fatal(err)
		}
		if s.Tab != tab {
			t.Fatal(tab)
		}
	}
	if err := tt.Click("Back to Overview"); err != nil {
		t.Fatal(err)
	}
	s.Scanning = true
	tt.Frame()
	if err := tt.Click("Scanning Drive..."); err != nil {
		t.Fatal(err)
	}
	if scans != 1 {
		t.Fatal("busy scan dispatched")
	}
	// Every nil callback must be safe even with populated results.
	for tab := DeclutterOverview; tab <= DeclutterDownloads; tab++ {
		s.Tab = tab
		declutterTester(&s, DeclutterPageActions{}).Frame()
	}
}

func TestDeclutterSelectionRequestsContextAndBusy(t *testing.T) {
	for _, tab := range []DeclutterTab{DeclutterDownloads, DeclutterLargeFiles, DeclutterDuplicates, DeclutterSimilarPhotos} {
		s := declutterFixture()
		s.Tab = tab
		before := fmt.Sprintf("%#v %#v %#v %#v", s.Downloads, s.LargeFiles, s.Duplicates, s.Photos)
		var events []string
		a := DeclutterPageActions{ToggleDownload: func(i int) { events = append(events, fmt.Sprintf("down:%d", i)) }, ToggleLargeFile: func(i int) { events = append(events, fmt.Sprintf("large:%d", i)) }, ToggleDuplicate: func(g, i int) { events = append(events, fmt.Sprintf("dup:%d:%d", g, i)) }, TogglePhoto: func(g, i int) { events = append(events, fmt.Sprintf("photo:%d:%d", g, i)) }, RequestRemoveSelected: func(got DeclutterTab) { events = append(events, fmt.Sprint(got)) }, Reveal: func(p string) { events = append(events, "reveal:"+p) }, Open: func(p string) { events = append(events, "open:"+p) }, CopyPath: func(p string) { events = append(events, "copy:"+p) }}
		tt := declutterTester(&s, a)
		label, want, path := "installer.dmg", "down:0", "/unreadable/installer.dmg"
		switch tab {
		case DeclutterLargeFiles:
			label, want, path = "movie.mp4", "large:1", "/unreadable/movie.mp4"
			if tt.HasText("hidden.iso") {
				t.Fatal("filter ignored")
			}
			if !tt.HasText("1 items selected • " + core.FmtSize(200_000_000) + " total") {
				t.Fatal("large-file summary must reflect filtered selection", tt.Texts())
			}
		case DeclutterDuplicates:
			label, want, path = "/unreadable/original.pdf", "dup:0:0", "/unreadable/original.pdf"
		case DeclutterSimilarPhotos:
			label, want, path = "photo-1.jpg", "photo:0:1", "/unreadable/photo-1.jpg"
		}
		if err := tt.Click(label); err != nil {
			t.Fatal(err)
		}
		if !reflect.DeepEqual(events, []string{want}) {
			t.Fatal(events)
		}
		if err := tt.Click("Remove Selected ›"); err != nil {
			t.Fatal(err)
		}
		if events[len(events)-1] != fmt.Sprint(tab) {
			t.Fatal(events)
		}
		if err := tt.RightClick(label); err != nil {
			t.Fatal(err)
		}
		for _, menu := range []string{"Reveal in File Explorer", "Open with System App", "Copy Full Path"} {
			if err := tt.ChooseMenuItem(menu); err != nil {
				t.Fatal(err)
			}
			if events[len(events)-1][strings.Index(events[len(events)-1], ":")+1:] != path {
				t.Fatal(events)
			}
			if err := tt.RightClick(label); err != nil {
				t.Fatal(err)
			}
		}
		tt.CloseMenu()
		for _, busy := range []string{"scan", "clean"} {
			s.Scanning = busy == "scan"
			s.Cleaning = busy == "clean"
			tt.Frame()
			n := len(events)
			_ = tt.Click(label)
			_ = tt.Click("Remove Selected ›")
			if len(events) != n {
				t.Fatal("busy selection/removal dispatched", events)
			}
		}
		if before != fmt.Sprintf("%#v %#v %#v %#v", s.Downloads, s.LargeFiles, s.Duplicates, s.Photos) {
			t.Fatal("render mutated snapshot")
		}
		if tab != DeclutterLargeFiles && tt.HasText("Cancel") {
			t.Fatal("clear selection leaked into another tab")
		}
	}
}

func TestDeclutterPoliciesAreEventsAndPhotoExpansion(t *testing.T) {
	s := declutterFixture()
	s.Tab = DeclutterSimilarPhotos
	var events []string
	a := DeclutterPageActions{SmartSelectPhotos: func() { events = append(events, "smart") }, KeepBestPhotos: func(i int) { events = append(events, fmt.Sprintf("best:%d", i)) }, KeepAllPhotos: func(i int) { events = append(events, fmt.Sprintf("keep:%d", i)) }, TogglePhotoGroup: func(i int) { s.ExpandedPhotoGroups[i] = !s.ExpandedPhotoGroups[i] }, TogglePhoto: func(int, int) {}, Open: func(p string) { events = append(events, "open:"+p) }, KeepNewestDuplicates: func() { events = append(events, "newest") }, KeepOldestDuplicates: func() { events = append(events, "oldest") }, CycleSizeFilter: func() { events = append(events, "size") }, CycleKindFilter: func() { events = append(events, "kind") }, ClearFilters: func() { events = append(events, "filters") }, ClearSelection: func(tab DeclutterTab) { events = append(events, fmt.Sprintf("clear:%d", tab)) }}
	tt := declutterTester(&s, a)
	if tt.HasText("photo-5.jpg") {
		t.Fatal("collapsed group rendered >4 redundant photos")
	}
	if _, ok := tt.Find("Select photo-0.jpg"); ok {
		t.Fatal("hero acquired selection checkbox")
	}
	for _, label := range []string{"✨ Smart Select All", "★ Keep Best Only", "Keep All", "Preview photo-0.jpg", "+ 2 more..."} {
		if err := tt.Click(label); err != nil {
			t.Fatal(err)
		}
	}
	if !tt.HasText("photo-6.jpg") || !tt.HasText("▲ Collapse") {
		t.Fatal(tt.Texts())
	}
	if err := tt.Click("▲ Collapse"); err != nil {
		t.Fatal(err)
	}
	s.Tab = DeclutterDuplicates
	tt.Frame()
	for _, label := range []string{"Keep Newest", "Keep Oldest"} {
		if err := tt.Click(label); err != nil {
			t.Fatal(err)
		}
	}
	s.Tab = DeclutterLargeFiles
	tt.Frame()
	for _, label := range []string{"Size: All ▾", "Kind: All Types ▾", "Clear Filters", "Cancel"} {
		if err := tt.Click(label); err != nil {
			t.Fatal(err)
		}
	}
	if !reflect.DeepEqual(events, []string{"smart", "best:0", "keep:0", "open:/unreadable/photo-0.jpg", "newest", "oldest", "size", "kind", "filters", "clear:3"}) {
		t.Fatal(events)
	}
}

func TestDeclutterEmptyBilingualAndRender(t *testing.T) {
	for _, lang := range []Language{Chinese, English} {
		for tab := DeclutterOverview; tab <= DeclutterDownloads; tab++ {
			s := DeclutterPage{Language: lang, Tab: tab}
			requests := 0
			tt := declutterTester(&s, DeclutterPageActions{RequestRemoveSelected: func(DeclutterTab) { requests++ }})
			if tab != DeclutterOverview && !tt.HasText(s.tr(fmt.Sprintf("empty%d", tab))) {
				t.Fatal(tt.Texts())
			}
			if tab != DeclutterOverview {
				if err := tt.Click(s.tr("remove")); err != nil {
					t.Fatal(err)
				}
				if requests != 0 {
					t.Fatal("empty selection requested deletion")
				}
			}
		}
		title, body, detail := DeclutterConfirmationCopy(lang, 3)
		if title == "" || !strings.Contains(body, "3") || detail == "" {
			t.Fatal(title, body, detail)
		}
	}
	for tab := DeclutterOverview; tab <= DeclutterDownloads; tab++ {
		s := declutterFixture()
		s.Tab = tab
		tt := declutterTester(&s, DeclutterPageActions{SetTab: func(DeclutterTab) {}, Open: func(string) {}})
		if tt.Image().Bounds().Dx() != 1100 {
			t.Fatal("missing raster frame")
		}
		if dir := os.Getenv("QUICKCLEANER_DECLUTTER_RENDER_DIR"); dir != "" {
			f, err := os.Create(filepath.Join(dir, fmt.Sprintf("declutter-%d.png", tab)))
			if err != nil {
				t.Fatal(err)
			}
			err = png.Encode(f, tt.Image())
			closeErr := f.Close()
			if err != nil || closeErr != nil {
				t.Fatal(err, closeErr)
			}
		}
	}
}

func TestDeclutterDisplayCapsDoNotTruncateSelectedSummary(t *testing.T) {
	s := DeclutterPage{Language: English, Tab: DeclutterDownloads}
	for i := range 101 {
		s.Downloads = append(s.Downloads, core.DownloadItem{Filename: fmt.Sprintf("download-%d", i), Selected: true, Size: 1000})
	}
	tt := declutterTester(&s, DeclutterPageActions{})
	if tt.HasText("download-100") || !tt.HasText("101 items selected • "+core.FmtSize(101_000)+" total") {
		t.Fatal(tt.Texts())
	}
	s.Tab = DeclutterDuplicates
	for i := range 51 {
		s.Duplicates = append(s.Duplicates, core.DuplicateGroup{Filename: fmt.Sprintf("group-%d", i), SizePerCopy: 1000, Files: []core.DuplicateFileItem{{Selected: true}}})
	}
	tt.Frame()
	if tt.HasText("group-50") || !tt.HasText("51 items selected • "+core.FmtSize(51_000)+" total") {
		t.Fatal(tt.Texts())
	}
	s.Tab = DeclutterSimilarPhotos
	for i := range 31 {
		s.Photos = append(s.Photos, core.PhotoGroup{Title: core.NewText("组", fmt.Sprintf("photos-%d", i)), Photos: []core.PhotoItem{{IsBestShot: true}, {Filename: "copy", Selected: true, Size: 1000}}})
	}
	tt.Frame()
	if tt.HasText("photos-30") || !tt.HasText("31 items selected • "+core.FmtSize(31_000)+" total") {
		t.Fatal(tt.Texts())
	}
}

// Rust hid everything past the first page; every entry must stay reachable
// for review.
func TestDeclutterListsShowMoreInsteadOfHidingTheRest(t *testing.T) {
	s := DeclutterPage{Language: English, Tab: DeclutterDownloads}
	for i := range 101 {
		s.Downloads = append(s.Downloads, core.DownloadItem{Filename: fmt.Sprintf("download-%d", i), Size: 1000})
	}
	tt := declutterTester(&s, DeclutterPageActions{ShowMore: s.ShowMore})
	if tt.HasText("download-100") || !tt.HasText("Show more (1 remaining)") {
		t.Fatal("first page", tt.Texts())
	}
	// The button follows the first page, below the fold.
	tt.Wheel(550, 425, 0, 100000)
	tt.Advance(2 * time.Second)
	tt.Frame()
	if err := tt.Click("Show more (1 remaining)"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if !tt.HasText("download-100") || tt.HasText("Show more (1 remaining)") {
		t.Fatal("after show more", tt.Texts())
	}

	s = DeclutterPage{Language: English, Tab: DeclutterDuplicates}
	for i := range 51 {
		s.Duplicates = append(s.Duplicates, core.DuplicateGroup{Filename: fmt.Sprintf("group-%d", i), SizePerCopy: 1000, Files: []core.DuplicateFileItem{{Path: fmt.Sprintf("/a/%d", i), IsOriginal: true}, {Path: fmt.Sprintf("/b/%d", i)}}})
	}
	tt = declutterTester(&s, DeclutterPageActions{ShowMore: s.ShowMore})
	if tt.HasText("group-50") || !tt.HasText("Show more (1 groups remaining)") {
		t.Fatal("duplicate first page", tt.Texts())
	}
	// The button follows the first page, below the fold.
	tt.Wheel(550, 425, 0, 100000)
	tt.Advance(2 * time.Second)
	tt.Frame()
	if err := tt.Click("Show more (1 groups remaining)"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if !tt.HasText("group-50") {
		t.Fatal("the 51st duplicate group stayed hidden")
	}
}
