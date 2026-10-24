package ui

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

type JunkSelection uint8

const (
	JunkRecommended JunkSelection = iota
	JunkAll
	JunkInvert
	JunkNone
)

// JunkPage presents frozen scan results. Selection, exclusions, confirmation and
// cache-release eligibility are supplied by the controller, never inferred here.
type JunkPage struct {
	Language                                 Language
	Scanned, Scanning, Cleaning, Discovering bool
	Categories                               []core.CategorySummary
	Selected                                 map[string]bool
	Expanded                                 map[core.CategoryID]bool
	ReleaseEligible                          map[string]bool
	Exclusions                               []string
	ShowExclusions, ShowFailedDetails        bool
	FailedDetails                            []string
	RecommendedSelected                      bool
	Scroll                                   ui.ScrollState
	lists                                    map[core.CategoryID]*ui.ListState
}

type JunkPageActions struct {
	ToggleCategory                         func(core.CategoryID)
	ToggleExpanded                         func(core.CategoryID)
	ToggleItem                             func(string)
	Select                                 func(JunkSelection)
	ToggleExclusions                       func()
	Exclude, RemoveExclusion, ReleaseCache func(string)
	ToggleFailedDetails                    func()
	RequestClean                           func()
}

var junkError = ui.Hex("#ba1a1a")
var junkErrorContainer = ui.Hex("#ffdad6")

func (s *JunkPage) RenderContent(c *ui.Context, actions JunkPageActions) {
	t := junkStrings(s.Language)
	ui.Column(c).Fill().Gap(0).Children(func() {
		ui.Scroll(c.Key("junk-scroll")).Grow(1).MinHeight(0).FillWidth().TrackScroll(&s.Scroll).Children(func() {
			ui.Column(c).FillWidth().Padding(20).Gap(20).Children(func() {
				ui.Row(c).Gap(16).Children(func() {
					ui.Column(c).Grow(1).MinWidth(0).Gap(4).Children(func() {
						ui.Text(c, dashboardStrings(s.Language).nav[Junk]).FontSize(18).Bold()
						ui.Text(c, t.headingSub).FontSize(12).TextColor(outline)
					})
					if junkButton(c.Key("exclusions-toggle"), fmt.Sprintf(t.exclusions, len(s.Exclusions)), true, s.ShowExclusions) && actions.ToggleExclusions != nil {
						actions.ToggleExclusions()
					}
					ui.Row(c).Padding(6, 12).Gap(12).Shrink(0).Radius(12).Border(1, outlineVariant.Alpha(.4)).Children(func() {
						size := "—"
						var total uint64
						for _, category := range s.Categories {
							total += category.TotalSize
						}
						if s.Scanned {
							size = core.FmtSize(total)
						}
						fg, bg := primary, primaryFixed
						if total > 0 {
							fg, bg = junkError, junkErrorContainer
						}
						ui.Column(c).Gap(0).Children(func() {
							ui.Text(c, t.found).FontSize(12).TextColor(outline)
							ui.Text(c, size).FontSize(14).Bold().TextColor(fg)
						})
						ui.Box(c).Size(26, 26).Radius(999).Center().Background(bg).Children(func() { icon(c, Junk, fg, 13) })
					})
				})
				if s.ShowExclusions {
					s.exclusions(c, actions)
				}
				if len(s.FailedDetails) > 0 {
					s.failures(c, actions)
				}
				if s.Scanning {
					ui.Column(c).FillWidth().Padding(48).Gap(12).AlignItems(ui.Center).Children(func() {
						ui.Spinner(c).Size(28, 28)
						ui.Text(c, t.loadingTitle).FontSize(16).Bold()
						ui.Text(c, t.loadingSub).FontSize(12).TextColor(outline)
					})
				} else {
					ui.Column(c).FillWidth().Gap(8).Padding(0, 0, 80, 0).Children(func() {
						for _, category := range s.Categories {
							s.category(c.Key(string(category.Category)), category, actions)
						}
					})
				}
			})
		})
		s.cleanBar(c, actions)
	})
}

func (s *JunkPage) category(c *ui.Context, category core.CategorySummary, actions JunkPageActions) {
	t := junkStrings(s.Language)
	id := category.Category
	copy := junkCategories[id]
	fg, bg := primary, primaryFixed
	switch id.Policy().Safety {
	case "caution":
		fg, bg = caution, cautionContainer
	case "danger":
		fg, bg = junkError, junkErrorContainer
	}
	var picked uint
	for _, item := range category.Items {
		if s.Selected[item.Path] {
			picked++
		}
	}
	ui.Column(c).FillWidth().Gap(0).Radius(12).Clip().Background(card).Border(1, outlineVariant.Alpha(.4)).Children(func() {
		ui.Row(c).Padding(10, 16).Gap(12).Children(func() {
			check := ui.ButtonBase(c.Key("category-check")).Padding(0).Cursor(ui.CursorPointer)
			check.Children(func() { junkCheck(c, core.CheckFromCounts(picked, uint(len(category.Items)))) })
			if check.Clicked() && actions.ToggleCategory != nil {
				actions.ToggleCategory(id)
			}
			head := ui.ButtonBase(c.Key("category-expand")).Grow(1).MinWidth(0).Padding(0).Gap(12).Justify(ui.Start).Cursor(ui.CursorPointer)
			head.Children(func() {
				ui.Box(c).Size(28, 28).Shrink(0).Radius(8).Background(bg).Center().Children(func() { junkCategoryIcon(c, id, fg) })
				ui.Column(c).Grow(1).MinWidth(0).Gap(2).Children(func() {
					ui.Row(c).Gap(8).Children(func() {
						ui.Text(c, copy[0].Get(s.Language)).FontSize(14).FontWeight(600)
						if id.IsDeveloper() {
							junkBadge(c, t.manual, fg, bg)
						}
						if category.Partial {
							junkBadge(c, t.partial, caution, cautionContainer)
						}
					})
					ui.Text(c, copy[1].Get(s.Language)).FontSize(12).TextColor(muted)
				})
				size := core.FmtSize(category.TotalSize)
				if s.Discovering && id.IsDiscovered() {
					size = t.discovering
				} else if category.Partial && category.TotalSize > 0 {
					size = "≥ " + size
				}
				color := fg
				if category.TotalSize == 0 {
					color = outline
				}
				ui.Text(c, size).Shrink(0).FontSize(14).Bold().TextColor(color)
				arrow := "▸"
				if s.Expanded[id] {
					arrow = "▾"
				}
				ui.Text(c, arrow).Width(14).FontSize(12).Bold().TextColor(outline)
			})
			if head.Clicked() && actions.ToggleExpanded != nil {
				actions.ToggleExpanded(id)
			}
		})
		if !s.Expanded[id] {
			return
		}
		if len(category.Items) == 0 {
			ui.Text(c, t.empty).FillWidth().Padding(16, 20).FontSize(12).TextColor(outline).BorderWidth(1, 0, 0, 0).BorderColor(outlineVariant.Alpha(.3))
			return
		}
		if s.lists == nil {
			s.lists = make(map[core.CategoryID]*ui.ListState)
		}
		state := s.lists[id]
		if state == nil {
			state = &ui.ListState{}
			s.lists[id] = state
		}
		state.Key = func(i int) any { return category.Items[i].Path }
		ui.List(c.Key("items"), state, len(category.Items), func(i int) { s.item(c.Key(category.Items[i].Path), category.Items[i], actions) }).
			FillWidth().Height(float32(min(len(category.Items)*50, 380))).BorderWidth(1, 0, 0, 0).BorderColor(outlineVariant.Alpha(.4))
	})
}

func (s *JunkPage) item(c *ui.Context, item core.ScanItem, actions JunkPageActions) {
	t := junkStrings(s.Language)
	ui.Row(c).Height(50).FillWidth().Padding(8, 16).Gap(12).BorderWidth(0, 0, 1, 0).BorderColor(outlineVariant.Alpha(.25)).Children(func() {
		row := ui.ButtonBase(c.Key("toggle")).Grow(1).MinWidth(0).FillHeight().Padding(0).Gap(12).Justify(ui.Start).Cursor(ui.CursorPointer)
		if row.Hovered() {
			row.Background(surfaceLow)
		}
		row.Children(func() {
			check := core.Off
			if s.Selected[item.Path] {
				check = core.On
			}
			junkCheck(c, check)
			ui.Column(c).Grow(1).MinWidth(0).Gap(2).Children(func() {
				ui.Row(c).Gap(8).Children(func() {
					ui.Text(c, item.Label.Get(s.Language)).FontSize(12).FontWeight(600)
					if reason := item.Rule.BlockedReason(); reason != "" {
						label := "规则约束冲突，已阻止清理"
						if s.Language == English {
							label = "Conflicting rule constraints; cleanup blocked"
						}
						junkBadge(c, label, junkError, junkErrorContainer)
					} else if item.Busy != nil {
						if label, app, ok := item.Busy.Badge(); ok {
							fg, bg := junkError, junkErrorContainer
							if app {
								fg, bg = caution, cautionContainer
							}
							junkBadge(c, label.Get(s.Language), fg, bg)
						}
					}
				})
				ui.Text(c, core.Truncate(item.Path, 70)).FontSize(12).TextColor(outline)
			})
			files := "—"
			if item.FileCount > 0 {
				files = fmt.Sprintf(t.files, core.Commas(item.FileCount))
			}
			ui.Text(c, files).Width(96).Shrink(0).FontSize(12).TextColor(outline).TextAlign(ui.End)
			ui.Text(c, core.FmtSize(item.Size)).Width(80).Shrink(0).FontSize(12).FontWeight(600).TextAlign(ui.End)
		})
		if row.Clicked() && actions.ToggleItem != nil {
			actions.ToggleItem(item.Path)
		}
		if s.ReleaseEligible[item.Path] && !s.Cleaning {
			if junkButton(c.Key("release"), t.release, true, false) && actions.ReleaseCache != nil {
				actions.ReleaseCache(item.Path)
			}
		}
		// These controls are siblings of the selection button: exclusion/release
		// must not bubble into an accidental selection of the protected target.
		if junkButton(c.Key("exclude"), t.exclude, true, false) && actions.Exclude != nil {
			actions.Exclude(item.Path)
		}
	})
}

func (s *JunkPage) exclusions(c *ui.Context, actions JunkPageActions) {
	t := junkStrings(s.Language)
	ui.Column(c).FillWidth().Padding(16).Gap(8).Radius(12).Border(1, outlineVariant.Alpha(.4)).Children(func() {
		ui.Text(c, t.exclusionsHint).FontSize(12).TextColor(outline)
		if len(s.Exclusions) == 0 {
			ui.Text(c, t.exclusionsEmpty).Padding(8, 0).FontSize(14).TextColor(muted)
		}
		for i, path := range s.Exclusions {
			ui.Row(c.Key(fmt.Sprint("wl-", i))).FillWidth().Padding(4, 8).Gap(12).Radius(6).Background(surfaceLow).Children(func() {
				ui.Text(c, core.Truncate(path, 90)).Grow(1).MinWidth(0).FontSize(12)
				if junkButton(c.Key(fmt.Sprint("remove-", i)), t.remove, true, false) && actions.RemoveExclusion != nil {
					actions.RemoveExclusion(path)
				}
			})
		}
	})
}

func (s *JunkPage) failures(c *ui.Context, actions JunkPageActions) {
	t := junkStrings(s.Language)
	heading := fmt.Sprintf(t.failed, len(s.FailedDetails))
	toggle := t.showDetails
	if s.ShowFailedDetails {
		toggle = t.hideDetails
	}
	ui.Column(c).FillWidth().Padding(16).Gap(8).Radius(12).Background(junkErrorContainer).Border(1, junkError.Alpha(.4)).Children(func() {
		ui.Row(c).FillWidth().Justify(ui.SpaceBetween).Children(func() {
			ui.Text(c, heading).FontSize(14).Bold().TextColor(junkError)
			if junkButton(c.Key("failed-details"), toggle, true, false) && actions.ToggleFailedDetails != nil {
				actions.ToggleFailedDetails()
			}
		})
		if s.ShowFailedDetails {
			ui.Column(c).FillWidth().Padding(8).Gap(4).Radius(6).Background(card.Alpha(.6)).Children(func() {
				for _, detail := range s.FailedDetails[:min(20, len(s.FailedDetails))] {
					ui.Text(c, core.Truncate(detail, 120)).FontSize(12).TextColor(junkError)
				}
			})
		}
	})
}

func (s *JunkPage) cleanBar(c *ui.Context, actions JunkPageActions) {
	t := junkStrings(s.Language)
	total, picked := 0, 0
	var size uint64
	for _, category := range s.Categories {
		for _, item := range category.Items {
			total++
			if s.Selected[item.Path] {
				picked++
				size += item.Size
			}
		}
	}
	ui.Row(c).FillWidth().Shrink(0).Padding(10, 20).Justify(ui.Center).Background(background).BorderWidth(1, 0, 0, 0).BorderColor(outlineVariant.Alpha(.5)).Children(func() {
		ui.Row(c).Padding(4, 5, 4, 20).Gap(20).Radius(999).Background(card).Border(1, outlineVariant.Alpha(.6)).Children(func() {
			ui.Column(c).Gap(0).Children(func() {
				ui.Text(c, t.selected).FontSize(12).TextColor(outline)
				ui.Row(c).Gap(8).Children(func() {
					ui.Text(c, core.FmtSize(size)).FontSize(16).Bold()
					ui.Text(c, fmt.Sprintf(t.selectedItems, picked)).FontSize(12).TextColor(outline)
				})
			})
			ui.Row(c).Gap(8).Children(func() {
				ui.Box(c).Width(1).Height(24).Background(outlineVariant.Alpha(.7))
				active := [4]bool{s.RecommendedSelected, picked == total && total > 0, false, picked == 0}
				for i, label := range t.batch {
					if junkButton(c.Key(fmt.Sprint("select-", i)), label, s.Scanned && !s.Cleaning, active[i]) && actions.Select != nil {
						actions.Select(JunkSelection(i))
					}
				}
				ui.Text(c, fmt.Sprintf(t.items, total)).FontSize(12).TextColor(outline)
			})
			label := t.clean
			if s.Cleaning {
				label = dashboardStrings(s.Language).cleaning
			}
			enabled := s.Scanned && !s.Cleaning && !s.Scanning && picked > 0
			button := ui.ButtonBase(c.Key("clean-now")).Padding(12, 20).Radius(999).Background(junkError).Disabled(!enabled).Cursor(ui.CursorPointer)
			button.Children(func() { ui.Text(c, label).FontSize(14).Bold().TextColor(card) })
			if button.Clicked() && actions.RequestClean != nil {
				actions.RequestClean()
			}
		})
	})
}

func junkButton(c *ui.Context, label string, enabled, active bool) bool {
	b := ui.ButtonBase(c).Padding(4, 12).Radius(6).Background(surfaceLow).Disabled(!enabled).Cursor(ui.CursorPointer)
	fg := muted
	if active {
		b.Background(primaryFixed)
		fg = primary
	} else if b.Hovered() {
		b.Background(surfaceHigh)
	}
	b.Children(func() { ui.Text(c, label).FontSize(12).FixedLineHeight(16).FontWeight(500).TextColor(fg) })
	return b.Clicked()
}

func junkBadge(c *ui.Context, label string, fg, bg ui.Color) {
	ui.Text(c, label).FontSize(10).FixedLineHeight(14).FontWeight(500).Padding(1, 6).Radius(999).TextColor(fg).Background(bg)
}

func junkCheck(c *ui.Context, state core.Check) {
	bg, border := card, outline
	if state != core.Off {
		bg, border = primary, primary
	}
	ui.Box(c).Size(20, 20).Shrink(0).Radius(6).Border(2, border).Background(bg).Center().Children(func() {
		if state == core.Partial {
			ui.Box(c).Width(8).Height(2).Background(card)
		}
		if state == core.On {
			ui.Text(c, "✓").FontSize(12).Bold().TextColor(card)
		}
	})
}

func junkCategoryIcon(c *ui.Context, category core.CategoryID, fg ui.Color) {
	const size float32 = 14
	switch category {
	case core.UserTemp, core.UserCache, core.Thumbnails, core.AiAgents:
		ui.Box(c).Size(size, size).Shrink(0).Center().Children(func() {
			ui.Box(c).Size(size*.45, size*.45).Radius(2).Background(fg)
			ui.Box(c).Absolute().Size(size, 2).Radius(999).Background(fg)
			ui.Box(c).Absolute().Size(2, size).Radius(999).Background(fg)
		})
	case core.Logs, core.LocalSnapshots:
		ui.Box(c).Size(size, size).Shrink(0).Radius(999).Border(2, fg).Center().Children(func() {
			ui.Box(c).Size(1.5, size*.35).Radius(999).Background(fg)
		})
	case core.DevWorktrees, core.BrokenLoginItems:
		ui.Box(c).Size(size, size).Shrink(0).Radius(6, 6, 12, 12).Border(2, fg).Center().Children(func() {
			ui.Box(c).Size(size*.35, size*.35).Radius(2).Background(fg)
		})
	case core.BrowserCache:
		icon(c, Overview, fg, size)
	case core.PackageCache, core.UpdaterPackages, core.IosBackup, core.OldIdeData, core.DockerImages:
		icon(c, Apps, fg, size)
	case core.DevBuild:
		ui.Box(c).Size(size, size).Shrink(0).Radius(999).Border(2, fg).Center().Children(func() {
			ui.Box(c).Size(size*.4, size*.4).Radius(999).Background(fg)
		})
	default:
		icon(c, Junk, fg, size)
	}
}
