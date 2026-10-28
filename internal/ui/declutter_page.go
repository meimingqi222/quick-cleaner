package ui

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

type DeclutterTab uint8

const (
	DeclutterOverview DeclutterTab = iota
	DeclutterSimilarPhotos
	DeclutterDuplicates
	DeclutterLargeFiles
	DeclutterDownloads
)

// DeclutterPage is a read-only presentation snapshot. The controller supplies
// filtered indices in scan order (including an empty slice for no matches),
// decoded thumbnails, selection, and expansion. No path is opened by rendering.
type DeclutterPage struct {
	Language                    Language
	Tab                         DeclutterTab
	Scanning, Scanned, Cleaning bool
	ScanElapsedSecs             *float64
	Downloads                   []core.DownloadItem
	LargeFiles                  []core.LargeFileItem
	Duplicates                  []core.DuplicateGroup
	Photos                      []core.PhotoGroup
	FilteredLargeIndices        []int
	MinSizeFilter               uint64
	KindFilter                  *int
	ExpandedPhotoGroups         map[int]bool
	// Supply only source-eligible decoded previews (jpg/jpeg/png/webp/bmp/gif,
	// at most 8,000,000 bytes). Missing images retain the source color fallback.
	Thumbnails map[string]*ui.Bitmap
	// Platform controls reveal copy only: "windows", "macos", or generic.
	Platform string
	// Shown is how many rows or groups a tab lists; zero means its first
	// page. Rust capped the lists outright, which hid the rest from review.
	Shown map[DeclutterTab]int
}

// declutterPage is each tab's first page and the step of "show more".
var declutterPage = map[DeclutterTab]int{DeclutterDownloads: 100, DeclutterLargeFiles: 100, DeclutterDuplicates: 50, DeclutterSimilarPhotos: 30}

// limit is how many entries of tab the list shows now.
func (s DeclutterPage) limit(tab DeclutterTab) int {
	return max(s.Shown[tab], declutterPage[tab])
}

// ShowMore extends tab's list by one page. Controllers call it from the
// ShowMore action; it is presentation state, not a selection.
func (s *DeclutterPage) ShowMore(tab DeclutterTab) {
	if s.Shown == nil {
		s.Shown = map[DeclutterTab]int{}
	}
	s.Shown[tab] = s.limit(tab) + declutterPage[tab]
}

// showMore offers the next page when total exceeds what is listed.
func (s DeclutterPage) showMore(c *ui.Context, a DeclutterPageActions, tab DeclutterTab, total int, groups bool) {
	rest := total - s.limit(tab)
	if rest <= 0 {
		return
	}
	key := "showMore"
	if groups {
		key = "showMoreGroups"
	}
	ui.Row(c).FillWidth().Padding(8, 0).Justify(ui.Center).Children(func() {
		s.button(c, fmt.Sprintf("show-more-%d", tab), fmt.Sprintf(s.tr(key), rest), func() { a.ShowMore(tab) }, a.ShowMore != nil)
	})
}

// Callbacks dispatch events only. RequestRemoveSelected must open the shared
// confirmation gate, never delete directly. It receives the tab, not a truncated
// visible scope: the controller re-derives the full selection at acceptance.
// Group/item indices refer to the unchanged snapshot, not filtered row positions.
type DeclutterPageActions struct {
	SetTab                                                        func(DeclutterTab)
	Scan                                                          func()
	CycleSizeFilter, CycleKindFilter, ClearFilters                func()
	ToggleDownload, ToggleLargeFile                               func(int)
	ToggleDuplicate, TogglePhoto                                  func(group, item int)
	KeepNewestDuplicates, KeepOldestDuplicates, SmartSelectPhotos func()
	KeepBestPhotos, KeepAllPhotos, TogglePhotoGroup               func(int)
	ClearSelection, RequestRemoveSelected, ShowMore               func(DeclutterTab)
	Reveal, Open, CopyPath                                        func(string)
}

func (s DeclutterPage) busy() bool { return s.Scanning || s.Cleaning }

func (s DeclutterPage) button(c *ui.Context, key, label string, action func(), enabled bool) ui.Element {
	enabled = enabled && action != nil
	button := ui.ButtonBase(c.Key("declutter-"+key)).Padding(5, 10).Radius(6).Border(1, outlineVariant.Alpha(.6)).Disabled(!enabled).Cursor(ui.CursorPointer)
	fg := muted
	interactive(button, ui.Transparent, surfaceLow, surfaceHigh, enabled)
	if !enabled {
		fg = outline
	}
	button.TextColor(fg).Children(func() { ui.Text(c, label).FontSize(12).FixedLineHeight(16) })
	if button.Clicked() && enabled {
		action()
	}
	return button
}

func (s DeclutterPage) RenderContent(c *ui.Context, a DeclutterPageActions) {
	ui.Column(c.Key("declutter-page")).FillWidth().Grow(1).MinHeight(0).Gap(0).Children(func() {
		ui.Scroll(c.Key(fmt.Sprintf("declutter-scroll-%d", s.Tab))).Grow(1).FillWidth().MinHeight(0).Padding(20).Gap(16).Children(func() {
			if s.Tab == DeclutterOverview {
				s.overview(c, a)
				return
			}
			ui.Row(c).FillWidth().Gap(8).Children(func() {
				s.button(c, "back", s.tr("back"), func() { a.SetTab(DeclutterOverview) }, a.SetTab != nil)
				ui.Box(c).Grow(1)
				for _, tab := range []DeclutterTab{DeclutterSimilarPhotos, DeclutterDuplicates, DeclutterLargeFiles, DeclutterDownloads} {
					b := s.button(c, fmt.Sprintf("tab-%d", tab), s.title(tab), func() { a.SetTab(tab) }, a.SetTab != nil).Radius(999)
					if s.Tab == tab {
						b.Background(primary).TextColor(card)
					}
				}
			})
			s.heading(c, s.tr(fmt.Sprintf("heading%d", s.Tab)), s.tr(fmt.Sprintf("sub%d", s.Tab)))
			switch s.Tab {
			case DeclutterDownloads:
				s.downloads(c, a)
			case DeclutterLargeFiles:
				s.largeFiles(c, a)
			case DeclutterDuplicates:
				s.duplicates(c, a)
			case DeclutterSimilarPhotos:
				s.photos(c, a)
			}
		})
		if s.Tab != DeclutterOverview && !s.Cleaning {
			s.actionBar(c, a)
		}
	})
}

func (s DeclutterPage) title(tab DeclutterTab) string { return s.tr(fmt.Sprintf("tab%d", tab)) }

func (s DeclutterPage) heading(c *ui.Context, title, sub string) {
	ui.Column(c).FillWidth().Padding(16).Gap(4).Radius(12).Background(card).Border(1, outlineVariant.Alpha(.4)).Children(func() {
		ui.Text(c, title).FontSize(18).Bold().TextColor(text)
		if sub != "" {
			ui.Text(c, sub).FontSize(12).TextColor(muted)
		}
	})
}

func (s DeclutterPage) empty(c *ui.Context) {
	ui.Column(c).FillWidth().Padding(40).Center().Gap(8).Children(func() {
		ui.Text(c, s.tr(fmt.Sprintf("empty%d", s.Tab))).FontSize(16).Bold()
		ui.Text(c, s.tr(fmt.Sprintf("emptySub%d", s.Tab))).FontSize(12).TextColor(outline)
	})
}

func (s DeclutterPage) selected(tab DeclutterTab) (count int, size uint64) {
	add := func(selected bool, bytes uint64) {
		if selected {
			count++
			size += bytes
		}
	}
	switch tab {
	case DeclutterDownloads:
		for _, f := range s.Downloads {
			add(f.Selected, f.Size)
		}
	case DeclutterLargeFiles:
		for _, i := range s.FilteredLargeIndices {
			if i >= 0 && i < len(s.LargeFiles) {
				f := s.LargeFiles[i]
				add(f.Selected, f.Size)
			}
		}
	case DeclutterDuplicates:
		for _, g := range s.Duplicates {
			for _, f := range g.Files {
				add(f.Selected, g.SizePerCopy)
			}
		}
	case DeclutterSimilarPhotos:
		for _, g := range s.Photos {
			for _, f := range g.Photos {
				add(f.Selected, f.Size)
			}
		}
	}
	return
}

func (s DeclutterPage) actionBar(c *ui.Context, a DeclutterPageActions) {
	n, size := s.selected(s.Tab)
	ui.Row(c).FillWidth().Height(70).Shrink(0).Padding(12, 20).Gap(12).Background(card).BorderWidth(1, 0, 0, 0).BorderColor(outlineVariant.Alpha(.4)).Children(func() {
		ui.Text(c, fmt.Sprintf(s.tr("selected"), n, core.FmtSize(size))).Grow(1).FontSize(14)
		if s.Tab == DeclutterLargeFiles {
			s.button(c, "clear-selection", s.tr("cancel"), func() { a.ClearSelection(s.Tab) }, !s.busy() && a.ClearSelection != nil)
		}
		b := s.button(c, "remove", s.tr("remove"), func() { a.RequestRemoveSelected(s.Tab) }, n > 0 && !s.busy() && a.RequestRemoveSelected != nil)
		b.Background(primary).TextColor(card).Padding(12, 24)
	})
}

// DeclutterConfirmationCopy lets the shared confirmation renderer preserve the
// source disposal distinction. The controller supplies the authoritative count
// and real scope list; presentation does not authorize or execute any deletion.
func DeclutterConfirmationCopy(l Language, count int) (title, body, detail string) {
	s := DeclutterPage{Language: l}
	return s.tr("confirmTitle"), fmt.Sprintf(s.tr("confirmBody"), count), s.tr("confirmDetail")
}
