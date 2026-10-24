package ui

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

var declutterError = ui.Hex("#ba1a1a")

// Geometry is the source's four declutter icons, not font-dependent emoji.
func declutterIcon(c *ui.Context, tab DeclutterTab, fg ui.Color, size float32) {
	switch tab {
	case DeclutterDownloads:
		ui.Column(c).Size(size, size).Shrink(0).Center().Gap(2).Children(func() {
			ui.Column(c).AlignItems(ui.Center).Gap(0).Children(func() {
				ui.Box(c).Size(2.5, size*.35).Background(fg)
				ui.Box(c).Size(size*.45, size*.25).BorderWidth(0, 0, 2, 2).BorderColor(fg)
			})
			ui.Box(c).Size(size*.75, 2.5).Radius(999).Background(fg)
		})
	case DeclutterLargeFiles:
		ui.Column(c).Size(size, size).Shrink(0).Center().Gap(0).Children(func() {
			ui.Box(c).Size(size*.7*.45, size*.2).Radius(999, 999, 0, 0).Border(2, fg)
			ui.Box(c).Size(size*.7, size*.65).Radius(4).Background(fg).Center().Children(func() { ui.Box(c).Size(size*.7*.4, 2).Radius(999).Background(card) })
		})
	case DeclutterDuplicates:
		ui.Box(c).Size(size, size).Shrink(0).Children(func() {
			ui.Box(c).Absolute().Left(1).Top(1).Size(size*.6, size*.75).Radius(2).Border(1, fg.Alpha(.5)).Background(fg.Alpha(.1))
			ui.Column(c).Absolute().Right(1).Bottom(1).Size(size*.6, size*.75).Radius(2).Border(2, fg).Background(card).Padding(2).Justify(ui.SpaceAround).Children(func() {
				ui.Box(c).FillWidth().Height(1.5).Radius(999).Background(fg)
				ui.Box(c).Size(size*.6*.6, 1.5).Radius(999).Background(fg)
			})
		})
	case DeclutterSimilarPhotos:
		ui.Box(c).Size(size, size).Shrink(0).Center().Children(func() {
			ui.Column(c).Size(size*.85, size*.7).Radius(4).Border(2, fg).Padding(2).Justify(ui.SpaceBetween).Children(func() {
				ui.Box(c).Size(3.5, 3.5).Radius(999).Background(fg)
				ui.Row(c).FillWidth().Height(size * .7 * .4).AlignItems(ui.End).Gap(2).Children(func() {
					ui.Box(c).Size(size*.85*.35, size*.7*.35).Radius(2, 2, 0, 0).Background(fg.Alpha(.8))
					ui.Box(c).Size(size*.85*.45, size*.7*.25).Radius(2, 2, 0, 0).Background(fg)
				})
			})
		})
	}
}

func (s DeclutterPage) contextMenu(a DeclutterPageActions, name, path string) func(*ui.Menu) {
	return func(m *ui.Menu) {
		name = strings.ReplaceAll(name, `\`, "/")
		name = name[strings.LastIndexByte(name, '/')+1:]
		m.Item(name).Disabled(true)
		m.Item(path).Disabled(true)
		m.Separator()
		key := "ctxReveal"
		if s.Platform == "windows" {
			key = "ctxWindows"
		} else if s.Platform == "macos" {
			key = "ctxMac"
		}
		for _, item := range []struct {
			label  string
			action func(string)
		}{{s.tr(key), a.Reveal}, {s.tr("ctxOpen"), a.Open}, {s.tr("ctxCopy"), a.CopyPath}} {
			if m.Item(item.label).Disabled(item.action == nil).Chosen() && item.action != nil {
				item.action(path)
			}
		}
	}
}

func (s DeclutterPage) fileRow(c *ui.Context, a DeclutterPageActions, key, name, path, sub, kind, date string, size uint64, selected bool, toggle func(), selectable bool) {
	ui.Row(c.Key(key)).FillWidth().Shrink(0).Padding(10, 16).Gap(12).Background(func() ui.Color {
		if selected {
			if s.Tab == DeclutterDuplicates {
				return declutterError.Alpha(.06)
			}
			return primary.Alpha(.05)
		}
		return card
	}()).BorderWidth(0, 0, 1, 0).BorderColor(outlineVariant.Alpha(.2)).ContextMenu(s.contextMenu(a, name, path)).Children(func() {
		row := ui.ButtonBase(c.Key(key + "-select")).Grow(1).MinWidth(0).Padding(0).Gap(12).Justify(ui.Start).Disabled(s.busy() || !selectable).Label(name)
		row.Children(func() {
			mark := "□"
			if selected {
				mark = "☑"
			}
			ui.Text(c, mark).FontSize(16).TextColor(primary)
			if s.Tab == DeclutterDownloads {
				ui.Box(c).Size(28, 28).Shrink(0).Radius(8).Background(ui.Hex("#e0f2fe")).Center().Children(func() { declutterIcon(c, DeclutterDownloads, primaryBright, 14) })
			}
			ui.Column(c).Grow(1).MinWidth(0).Clip().Gap(2).Children(func() {
				ui.Text(c, name).FontSize(12).Bold().SingleLine()
				ui.Text(c, sub).FontSize(12).TextColor(outline).SingleLine()
			})
		})
		if row.Clicked() && toggle != nil {
			toggle()
		}
		s.button(c, key+"-reveal", s.tr("reveal"), func() { a.Reveal(path) }, a.Reveal != nil).Padding(3, 8)
		s.button(c, key+"-open", s.tr("open"), func() { a.Open(path) }, a.Open != nil).Padding(3, 8)
		if s.Tab == DeclutterDuplicates {
			if kind != "" {
				fg, bg := outline, surfaceHigh
				if kind == s.tr("duplicate") {
					fg, bg = declutterError, ui.Hex("#ffdad6")
				}
				ui.Text(c, kind).FontSize(12).Bold().Padding(2, 8).Radius(999).TextColor(fg).Background(bg)
			}
		} else {
			ui.Text(c, kind).Width(85).FontSize(12).TextColor(muted).SingleLine()
			ui.Text(c, core.FmtSize(size)).Width(90).FontSize(12).Bold().TextColor(primary)
		}
		if date != "" {
			ui.Text(c, date).Width(90).FontSize(12).TextColor(muted).SingleLine()
		}
	})
}

func (s DeclutterPage) columns(c *ui.Context, date string) {
	ui.Row(c).FillWidth().Padding(12, 16).Gap(12).Background(surfaceLow).Children(func() {
		ui.Text(c, s.tr("name")).Grow(1).FontSize(12).Bold().TextColor(outline)
		ui.Box(c).Width(140)
		for _, k := range []string{"kind", "size", date} {
			ui.Text(c, s.tr(k)).Width(100).Shrink(0).FontSize(12).Bold().TextColor(outline).NoWrap()
		}
	})
}

func (s DeclutterPage) downloads(c *ui.Context, a DeclutterPageActions) {
	ui.Text(c, fmt.Sprintf(s.tr("count"), len(s.Downloads))).FontSize(14).Bold().TextColor(primary)
	ui.Column(c).FillWidth().Shrink(0).Radius(12).Border(1, outlineVariant.Alpha(.4)).Clip().Gap(0).Children(func() {
		s.columns(c, "downloaded")
		if len(s.Downloads) == 0 {
			s.empty(c)
		}
		for i, f := range s.Downloads[:min(s.limit(DeclutterDownloads), len(s.Downloads))] {
			s.fileRow(c, a, fmt.Sprintf("declutter-down-%d", i), f.Filename, f.Path, f.Path, f.Kind.Get(s.Language), f.DownloadedAtStr.Get(s.Language), f.Size, f.Selected, func() { a.ToggleDownload(i) }, a.ToggleDownload != nil)
		}
	})
	s.showMore(c, a, DeclutterDownloads, len(s.Downloads), false)
}

func (s DeclutterPage) largeFiles(c *ui.Context, a DeclutterPageActions) {
	var total uint64
	indices := make([]int, 0, len(s.FilteredLargeIndices))
	for _, i := range s.FilteredLargeIndices {
		if i >= 0 && i < len(s.LargeFiles) {
			indices = append(indices, i)
			total += s.LargeFiles[i].Size
		}
	}
	ui.Row(c).FillWidth().Gap(8).Children(func() {
		label := s.tr("sizeAll")
		if s.MinSizeFilter > 0 {
			size := "1GB"
			switch s.MinSizeFilter {
			case 50_000_000:
				size = "50MB"
			case 100_000_000:
				size = "100MB"
			case 500_000_000:
				size = "500MB"
			}
			label = fmt.Sprintf(s.tr("sizeFilter"), size)
		}
		s.button(c, "size-filter", label, a.CycleSizeFilter, !s.busy())
		label = s.tr("kindAll")
		if s.KindFilter != nil && *s.KindFilter >= 0 && *s.KindFilter <= 3 {
			label = s.tr(fmt.Sprintf("kind%d", *s.KindFilter))
		}
		s.button(c, "kind-filter", label, a.CycleKindFilter, !s.busy())
		s.button(c, "clear-filters", s.tr("clearFilters"), a.ClearFilters, !s.busy())
		ui.Text(c, s.tr("found")+" · "+core.FmtSize(total)).Grow(1).FontSize(12).TextColor(primary)
	})
	ui.Column(c).FillWidth().Shrink(0).Radius(12).Border(1, outlineVariant.Alpha(.4)).Clip().Gap(0).Children(func() {
		s.columns(c, "accessed")
		if len(indices) == 0 {
			s.empty(c)
		}
		for _, i := range indices[:min(s.limit(DeclutterLargeFiles), len(indices))] {
			f := s.LargeFiles[i]
			s.fileRow(c, a, fmt.Sprintf("declutter-large-%d", i), f.Filename, f.Path, f.PathDisplay, f.Kind.Get(s.Language), f.LastAccessedStr.Get(s.Language), f.Size, f.Selected, func() { a.ToggleLargeFile(i) }, a.ToggleLargeFile != nil)
		}
	})
	s.showMore(c, a, DeclutterLargeFiles, len(indices), false)
}

func (s DeclutterPage) duplicates(c *ui.Context, a DeclutterPageActions) {
	ui.Row(c).FillWidth().Gap(8).Children(func() {
		label := fmt.Sprintf(s.tr("dupSummary"), len(s.Duplicates))
		if shown := s.limit(DeclutterDuplicates); len(s.Duplicates) > shown {
			label += fmt.Sprintf(s.tr("dupCap"), shown)
		}
		ui.Text(c, label).Grow(1).FontSize(12).TextColor(muted)
		s.button(c, "newest", s.tr("newest"), a.KeepNewestDuplicates, !s.busy())
		s.button(c, "oldest", s.tr("oldest"), a.KeepOldestDuplicates, !s.busy())
	})
	if len(s.Duplicates) == 0 {
		s.empty(c)
	}
	defer s.showMore(c, a, DeclutterDuplicates, len(s.Duplicates), true)
	for gi, g := range s.Duplicates[:min(s.limit(DeclutterDuplicates), len(s.Duplicates))] {
		ui.Column(c).FillWidth().Shrink(0).Radius(12).Border(1, outlineVariant.Alpha(.4)).Clip().Gap(0).Children(func() {
			ui.Row(c).FillWidth().Padding(12, 16).Background(surfaceLow).Gap(12).Children(func() {
				ui.Box(c).Size(32, 32).Shrink(0).Radius(8).Background(ui.Hex("#efdbff")).Center().Children(func() { declutterIcon(c, DeclutterDuplicates, ui.Hex("#7547ab"), 18) })
				ui.Column(c).Grow(1).MinWidth(0).Gap(2).Children(func() {
					ui.Text(c, g.Filename).FontSize(14).Bold().SingleLine()
					ui.Text(c, fmt.Sprintf(s.tr("dupSub"), core.FmtSize(g.SizePerCopy), len(g.Files))).FontSize(12).TextColor(outline)
				})
			})
			for fi, f := range g.Files {
				badge := ""
				if f.IsOriginal {
					badge = s.tr("original")
				} else if f.Selected {
					badge = s.tr("duplicate")
				}
				s.fileRow(c, a, fmt.Sprintf("declutter-dup-%d-%d", gi, fi), f.PathDisplay, f.Path, fmt.Sprintf(s.tr("modified"), f.ModifiedAtStr), badge, "", g.SizePerCopy, f.Selected, func() { a.ToggleDuplicate(gi, fi) }, a.ToggleDuplicate != nil)
			}
		})
	}
}

func (s DeclutterPage) overview(c *ui.Context, a DeclutterPageActions) {
	var potential, downloads, large uint64
	for _, f := range s.Downloads {
		downloads += f.Size
		if f.Selected {
			potential += f.Size
		}
	}
	for _, f := range s.LargeFiles {
		large += f.Size
		if f.Selected {
			potential += f.Size
		}
	}
	for _, g := range s.Duplicates {
		potential += g.CleanableSize()
	}
	for _, g := range s.Photos {
		potential += g.CleanableSize()
	}
	ui.Row(c).FillWidth().Gap(24).Children(func() {
		ui.Column(c).Grow(1).MinWidth(0).Gap(8).Children(func() {
			if s.Scanning {
				ui.Text(c, s.tr("scanning")).FontSize(12).TextColor(primary)
			} else if s.ScanElapsedSecs != nil {
				ui.Text(c, fmt.Sprintf(s.tr("elapsed"), *s.ScanElapsedSecs)).FontSize(12).TextColor(primary)
			}
			ui.Text(c, s.tr("heading0")).FontSize(18).Bold()
			ui.Text(c, s.tr("sub0")).FontSize(12).TextColor(muted)
		})
		ui.Column(c).Width(260).Gap(8).Children(func() {
			label := s.tr("scan")
			if s.Scanning {
				label = s.tr("scanBusy")
			}
			s.button(c, "scan", label, a.Scan, !s.busy()).Padding(12, 24).Background(primary).TextColor(card)
			label = s.tr("eta")
			if s.Scanned {
				label = s.tr("savings") + " " + core.FmtSize(potential)
			}
			ui.Text(c, label).FontSize(12).TextColor(outline)
		})
	})
	for _, pair := range [][2]DeclutterTab{{DeclutterDownloads, DeclutterLargeFiles}, {DeclutterDuplicates, DeclutterSimilarPhotos}} {
		ui.Row(c).FillWidth().Gap(16).Children(func() {
			for _, tab := range pair {
				b := ui.ButtonBase(c.Key(fmt.Sprintf("declutter-card-%d", tab))).MinWidth(0).Height(195).Padding(16).Radius(12).Background(card).Border(1, outlineVariant.Alpha(.45)).Disabled(a.SetTab == nil).Label(s.title(tab))
				if tab == DeclutterLargeFiles {
					b.Width(310).Shrink(0)
				} else {
					b.Grow(1)
				}
				b.Children(func() {
					ui.Column(c).FillWidth().Gap(10).Children(func() {
						title := s.title(tab)
						if tab == DeclutterDownloads {
							title = s.tr("card4")
						}
						ui.Row(c).FillWidth().Gap(12).Children(func() {
							fg, bg := primaryBright, ui.Hex("#e0f2fe")
							switch tab {
							case DeclutterLargeFiles:
								fg, bg = declutterError, ui.Hex("#ffdad6")
							case DeclutterDuplicates:
								fg, bg = ui.Hex("#7547ab"), ui.Hex("#efdbff")
							case DeclutterSimilarPhotos:
								fg, bg = caution, cautionContainer
							}
							ui.Box(c).Size(30, 30).Shrink(0).Radius(8).Background(bg).Center().Children(func() { declutterIcon(c, tab, fg, 16) })
							ui.Text(c, title).Grow(1).MinWidth(0).FontSize(14).Bold()
						})
						ui.Text(c, s.tr(fmt.Sprintf("cardSub%d", tab))).FontSize(12).TextColor(outline)
						switch tab {
						case DeclutterDownloads:
							ui.Row(c).FillWidth().Gap(24).Children(func() {
								ui.Column(c).Grow(1).Gap(4).Children(func() {
									ui.Text(c, s.tr("potential")).FontSize(12).Bold().TextColor(outline)
									label := s.tr("pendingShort")
									if s.Scanned {
										label = core.FmtSize(downloads)
									}
									ui.Text(c, label).FontSize(18).Bold().TextColor(primary)
								})
								ui.Row(c).Gap(16).AlignItems(ui.End).Children(func() {
									for i, ext := range []string{".DMG", ".ZIP", ".PKG"} {
										ui.Column(c).Gap(4).AlignItems(ui.Center).Children(func() {
											ui.Box(c).Width(8).Height([]float32{36, 22, 14}[i]).Background([]ui.Color{primaryBright, ui.Hex("#7547ab"), caution}[i]).Radius(2, 2, 0, 0)
											ui.Text(c, ext).FontSize(12).TextColor(outline)
										})
									}
								})
							})
						case DeclutterLargeFiles:
							ui.Row(c).FillWidth().Gap(16).Children(func() {
								ui.Column(c).Size(64, 64).Radius(999).Border(4, declutterError).Background(surfaceHigh).Center().Children(func() {
									ui.Text(c, fmt.Sprint(len(s.LargeFiles))).FontSize(16).Bold()
									ui.Text(c, s.tr("files")).FontSize(12).TextColor(outline)
								})
							})
							label := s.tr("pendingShort")
							if s.Scanned {
								label = fmt.Sprintf(s.tr("total"), core.FmtSize(large))
							}
							ui.Text(c, label).FontSize(12).TextColor(declutterError)
						case DeclutterDuplicates:
							ui.Text(c, fmt.Sprintf("%d %s", len(s.Duplicates), s.tr("sets"))).FontSize(18).Bold()
							ui.Text(c, s.tr("select")).FontSize(12).TextColor(primary)
						case DeclutterSimilarPhotos:
							status := s.tr("pending")
							if s.Scanned {
								status = s.tr("analyzed")
							}
							ui.Text(c, status).FontSize(12).TextColor(caution)
							ui.Text(c, fmt.Sprintf("%s · %d", s.tr("photoGroups"), len(s.Photos))).FontSize(16).Bold()
							ui.Box(c).FillWidth().Height(4).Radius(999).Background(surfaceHigh).Children(func() { ui.Box(c).Width(80).Height(4).Radius(999).Background(caution) })
						}
					})
				})
				if b.Clicked() && a.SetTab != nil {
					a.SetTab(tab)
				}
			}
		})
	}
}
