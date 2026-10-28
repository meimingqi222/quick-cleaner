package ui

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

type DiskTab uint8

const (
	DiskTree DiskTab = iota
	DiskFiles
)

type DiskPageSpace struct{ Total, Free uint64 }
type DiskPageVolume struct {
	ID    core.VolumeID
	Space *DiskPageSpace
}
type DiskPageCrumb struct {
	ID         uint32
	Name, Path string
}

// Selection includes inherited inclusions and local exclusions, resolved by the
// controller. A missing capability is deliberately not deletion authority.
type DiskPageRow struct {
	Node                                                core.Node
	Path                                                string
	Protected, Selected, CanSelect, CanDelete, CanEnter bool
}

// DiskPage is an immutable presentation snapshot. Rows are controller-ordered
// children or the largest 200 files. No tree, selection resolver, native scanner
// or filesystem handle crosses this boundary. Partial/IndexStatus are native
// acquisition facts, never a completeness guess made from visible rows.
type DiskPage struct {
	Language                                     Language
	Windows, Scanning, Loaded, Cleaning, Partial bool
	Error, IndexStatus                           string
	Volume                                       core.VolumeID
	Volumes                                      []DiskPageVolume
	Space                                        *DiskPageSpace
	UniqueSize, CurrentSize                      uint64
	CurrentName, CurrentPath                     string
	AtRoot                                       bool
	Tab                                          DiskTab
	Crumbs                                       []DiskPageCrumb
	CrumbsExpanded                               bool
	Rows                                         []DiskPageRow
	SelectedCount                                int
	SelectedSize                                 uint64
	HeaderSelected, HeaderMixed, CanSelectView   bool
	CanRequestDelete                             bool
	ToRecycleBin                                 bool
}

// RequestDelete and RequestDeleteSelected open controller-owned confirmation;
// they must revalidate native safety, coverage and identity before any removal.
// Callback row values are frozen facts, not authorization to delete that path.
type DiskPageActions struct {
	StartScan, Up, ClearSelection, RequestDeleteSelected func()
	SwitchVolume                                         func(core.VolumeID)
	SetTab                                               func(DiskTab)
	Navigate                                             func(uint32)
	SetCrumbsExpanded                                    func(bool)
	ToggleSelected, RequestDelete                        func(DiskPageRow)
	SetViewSelected                                      func(bool)
	SetRecycleBin                                        func(bool)
	CopyPath                                             func(string)
}

func (s DiskPage) RenderContent(c *ui.Context, a DiskPageActions) {
	t := diskStrings(s.Language)
	ui.Column(c.Key("disk-page")).Grow(1).FillWidth().MinHeight(0).Padding(20).Gap(20).Children(func() {
		ui.Column(c).FillWidth().Gap(4).Children(func() {
			ui.Text(c, t.heading).FontSize(18).Bold()
			ui.Text(c, t.subheading).FontSize(12).TextColor(muted)
		})
		if s.Scanning {
			s.message(c, fmt.Sprintf(t.loading, s.Volume.Display()), diskLoadingSub(s.Language, s.Windows))
			return
		}
		if s.Error != "" {
			s.message(c, t.errorPrefix+s.Error, t.errorHint)
			return
		}
		if !s.Loaded {
			ui.Column(c).Grow(1).FillWidth().Center().Gap(20).Children(func() {
				ui.Box(c).Size(64, 64).Radius(999).Background(primaryFixed).Center().Children(func() { icon(c, Disk, primary, 28) })
				ui.Text(c, t.prompt).FontSize(16).Bold()
				ui.Row(c).MaxWidth(720).Wrap().Justify(ui.Center).Gap(12).Children(func() {
					for _, v := range s.Volumes {
						b := ui.ButtonBase(c.Key("disk-initial-volume-"+v.ID.Mount)).Width(220).Padding(12, 16).Radius(12).Border(2, outlineVariant.Alpha(.6)).Gap(12).Disabled(a.SwitchVolume == nil)
						fg := text
						if v.ID == s.Volume {
							b.Background(primaryFixed).BorderColor(primary)
							fg = primary
						} else if b.Hovered() {
							b.Background(surfaceHigh)
						}
						b.Children(func() {
							ui.Box(c).Size(36, 36).Shrink(0).Radius(999).Background(surfaceLow).Center().Children(func() { icon(c, Disk, fg, 18) })
							ui.Column(c).Grow(1).MinWidth(0).Gap(1).Children(func() {
								ui.Text(c, s.volumeLabel(v.ID)).FontSize(14).Bold().TextColor(fg).SingleLine()
								if usage := s.volumeUsage(v.Space); usage != "" {
									ui.Text(c, usage).FontSize(12).TextColor(outline)
								}
							})
						})
						if b.Clicked() && a.SwitchVolume != nil {
							a.SwitchVolume(v.ID)
						}
					}
				})
				appsPageButton(c, "disk-start", fmt.Sprintf(t.scan, s.volumeLabel(s.Volume)), a.StartScan == nil, true, a.StartScan)
			})
			return
		}
		if s.Partial || s.IndexStatus != "" {
			label := s.IndexStatus
			if s.Partial {
				label = t.partial
				if s.IndexStatus != "" {
					label += " · " + s.IndexStatus
				}
			}
			ui.Text(c, label).FillWidth().FontSize(12).TextColor(caution).Padding(6, 12).Radius(6).Background(cautionContainer)
		}
		ui.Row(c).Grow(1).FillWidth().MinHeight(0).Gap(24).Children(func() {
			s.lens(c, a)
			s.browser(c, a)
		})
		if s.SelectedCount > 0 && !s.Cleaning {
			s.cleanBar(c, a)
		}
	})
}

func (s DiskPage) message(c *ui.Context, title, sub string) {
	ui.Column(c).Grow(1).FillWidth().Center().Gap(12).Children(func() {
		fg := text
		if s.Scanning {
			loadingSpinner(c, 28)
		} else {
			fg = ui.Hex("#ba1a1a")
			ui.Box(c).Size(56, 56).Radius(999).Background(ui.Hex("#ffdad6")).Center().Children(func() { icon(c, Junk, fg, 24) })
		}
		ui.Text(c, title).FontSize(16).Bold().TextColor(fg)
		ui.Text(c, sub).FontSize(12).TextColor(outline)
		if s.IndexStatus != "" {
			ui.Text(c, s.IndexStatus).FontSize(12).TextColor(outline)
		}
	})
}

func (s DiskPage) browser(c *ui.Context, a DiskPageActions) {
	t := diskStrings(s.Language)
	ui.Column(c.Key("disk-browser")).Grow(1).MinWidth(0).FillHeight().MinHeight(0).Radius(12).Border(1, outlineVariant.Alpha(.5)).Clip().Gap(0).Children(func() {
		ui.Row(c).FillWidth().MinHeight(46).Padding(8, 16).Gap(8).Background(surfaceLow).Children(func() {
			appsPageButton(c, "disk-up", t.up, len(s.Crumbs) <= 1 || a.Up == nil, false, a.Up).Shrink(0)
			ui.Row(c).Grow(1).MinWidth(0).Clip().Gap(4).Children(func() {
				for i, crumb := range s.Crumbs {
					if !s.CrumbsExpanded && len(s.Crumbs) > 3 && i > 0 && i < len(s.Crumbs)-2 {
						if i == 1 {
							appsPageButton(c, "disk-crumb-expand", "…", a.SetCrumbsExpanded == nil, false, func() { a.SetCrumbsExpanded(true) }).Tooltip(s.CurrentPath)
						}
						continue
					}
					appsPageButton(c, fmt.Sprintf("disk-crumb-%d", i), crumb.Name, i == len(s.Crumbs)-1 || a.Navigate == nil, false, func() { a.Navigate(crumb.ID) }).Padding(2, 4).Tooltip(crumb.Path).Shrink(0)
					if i < len(s.Crumbs)-1 {
						ui.Text(c, "›").FontSize(12).TextColor(outline)
					}
				}
				if s.CrumbsExpanded && len(s.Crumbs) > 3 {
					appsPageButton(c, "disk-crumb-collapse", t.collapse, a.SetCrumbsExpanded == nil, false, func() { a.SetCrumbsExpanded(false) })
				}
			})
			if s.SelectedCount > 0 {
				appsPageButton(c, "disk-cross-folder-clear", fmt.Sprintf(t.selectedBadge, s.SelectedCount, core.FmtSize(s.SelectedSize))+" ✕", a.ClearSelection == nil, false, a.ClearSelection).Radius(999).Background(ui.Hex("#ffdad6")).TextColor(ui.Hex("#ba1a1a"))
			}
			for i, label := range []string{t.tree, t.files} {
				appsPageButton(c, fmt.Sprintf("disk-tab-%d", i), label, a.SetTab == nil, s.Tab == DiskTab(i), func() { a.SetTab(DiskTab(i)) }).Shrink(0)
			}
		})
		if s.Tab == DiskTree {
			ui.Row(c).FillWidth().Padding(4, 16).Gap(8).Children(func() {
				ui.Text(c, s.CurrentPath).Grow(1).MinWidth(0).FontSize(12).TextColor(outline).SingleLine().Tooltip(s.CurrentPath)
				appsPageButton(c, "disk-copy-path", t.copy, a.CopyPath == nil, false, func() { a.CopyPath(s.CurrentPath) }).Shrink(0)
			})
		}
		ui.Row(c).FillWidth().Padding(8, 16).Gap(12).Background(surfaceLow).Children(func() {
			on := s.HeaderSelected
			check := ui.Checkbox(c.Key("disk-select-view"), &on, "").Label(t.selectView).Disabled(s.Cleaning || !s.CanSelectView || a.SetViewSelected == nil).Width(20).Shrink(0)
			if s.HeaderMixed {
				// Mixed sets accessibility, not Checkbox's painted bool value.
				check.Mixed().DrawOver(func(p *ui.Painter, r ui.Rect) {
					p.Fill(ui.Rect{X: r.X, Y: r.Y + (r.H-16)/2, W: 16, H: 16}, primary, 4)
					p.Fill(ui.Rect{X: r.X + 4, Y: r.Y + r.H/2 - 1, W: 8, H: 2}, card, 1)
				})
			}
			if check.Changed() && a.SetViewSelected != nil {
				a.SetViewSelected(!s.HeaderSelected)
			}
			ui.Text(c, t.name).Grow(1).FontSize(12).Bold().TextColor(outline)
			ui.Text(c, t.size).Width(90).Shrink(0).FontSize(12).Bold().TextColor(outline)
			ui.Text(c, t.action).Width(40).Shrink(0).FontSize(12).TextColor(outline)
		})
		ui.Scroll(c.Key(fmt.Sprintf("disk-rows-%s-%d-%s", s.Volume.Mount, s.Tab, s.CurrentPath))).Grow(1).FillWidth().MinHeight(0).Gap(0).Children(func() {
			if len(s.Rows) == 0 {
				label := t.empty
				if s.Tab == DiskFiles {
					label = t.noFiles
				}
				ui.Text(c, label).Padding(24).FontSize(14).TextColor(outline)
			}
			for _, row := range s.Rows {
				s.row(c, a, row)
			}
		})
	})
}

func (s DiskPage) row(c *ui.Context, a DiskPageActions, r DiskPageRow) {
	t := diskStrings(s.Language)
	key := fmt.Sprintf("disk-row-%d", r.Node.Idx)
	row := ui.Row(c.Key(key)).FillWidth().MinHeight(44).Shrink(0).Padding(8, 16).Gap(12).BorderWidth(0, 0, 1, 0).BorderColor(outlineVariant.Alpha(.25))
	interactiveRow(row, ui.Transparent, surfaceLow)
	row.Children(func() {
		on := r.Selected
		check := ui.Checkbox(c.Key(key+"-check"), &on, "").Label(fmt.Sprintf(t.selectItem, r.Node.Name)).Width(20).Shrink(0).Disabled(s.Cleaning || r.Protected || !r.CanSelect || a.ToggleSelected == nil)
		if check.Changed() && a.ToggleSelected != nil {
			a.ToggleSelected(r)
		}
		name := r.Node.Name
		if s.Tab == DiskFiles {
			name = r.Path
		}
		b := ui.ButtonBase(c.Key(key + "-name")).Grow(1).MinWidth(0).Padding(0).Justify(ui.Start).Gap(12).Tooltip(r.Path)
		b.Children(func() {
			// Files show their type, as in Search; a trash can read as a delete
			// button on rows that only open on click.
			if r.Node.IsDir {
				ui.Box(c).Size(28, 28).Shrink(0).Radius(8).Background(primaryFixed).Center().Children(func() { icon(c, Overview, primary, 14) })
			} else {
				ui.Box(c).Size(28, 28).Shrink(0).Radius(8).Background(surfaceHigh).Center().Children(func() { FileKind(r.Node.Name, false).Render(c, 16) })
			}
			ui.Column(c).Grow(1).MinWidth(0).Clip().Gap(0).Children(func() {
				label := ui.Text(c, name).FillWidth().FontSize(14).SingleLine()
				if r.Node.IsDir {
					label.FontWeight(600)
				}
				if r.Protected {
					ui.Text(c, t.protected).FontSize(12).TextColor(outline)
				}
			})
		})
		if b.Clicked() && s.Tab == DiskTree && r.CanEnter && a.Navigate != nil {
			a.Navigate(r.Node.Idx)
		}
		fg := text
		if r.Node.Size >= 1<<30 {
			fg = primary
		}
		ui.Text(c, core.FmtSize(r.Node.Size)).Width(90).Shrink(0).FontSize(14).Bold().TextColor(fg).TextAlign(ui.End)
		ui.Box(c).Width(40).Shrink(0).Children(func() {
			if s.Tab == DiskTree && r.Node.IsDir && r.CanEnter {
				appsPageButton(c, key+"-enter", "›", a.Navigate == nil, false, func() { a.Navigate(r.Node.Idx) })
			} else if !r.Node.IsDir && !r.Protected && r.CanDelete {
				appsPageButton(c, key+"-delete", t.delete, s.Cleaning || a.RequestDelete == nil, false, func() { a.RequestDelete(r) }).Padding(2, 0).TextColor(ui.Hex("#ba1a1a"))
			}
		})
	})
}

func (s DiskPage) cleanBar(c *ui.Context, a DiskPageActions) {
	t := diskStrings(s.Language)
	ui.Row(c).FillWidth().Justify(ui.Center).Children(func() {
		ui.Row(c.Key("disk-clean-bar")).Padding(12, 24).Gap(16).Radius(999).Background(card).Border(1, outlineVariant.Alpha(.6)).Shadow(0, 10, 15, -3, ui.RGBA(0, 0, 0, .1)).Children(func() {
			ui.Column(c).Gap(0).Children(func() {
				ui.Text(c, t.selectedTitle).FontSize(12).TextColor(outline)
				ui.Text(c, fmt.Sprintf(t.selectedCount, s.SelectedCount)).FontSize(12).Bold()
			})
			ui.Column(c).Gap(0).Children(func() {
				ui.Text(c, core.FmtSize(s.SelectedSize)).FontSize(16).Bold().TextColor(ui.Hex("#ba1a1a"))
				label := t.freed
				if s.ToRecycleBin {
					label = t.recycled
				}
				ui.Text(c, label).FontSize(12).TextColor(outline)
			})
			on := s.ToRecycleBin
			check := ui.Checkbox(c.Key("disk-recycle"), &on, t.recycle).Disabled(s.Cleaning || a.SetRecycleBin == nil)
			if check.Changed() && a.SetRecycleBin != nil {
				a.SetRecycleBin(on)
			}
			appsPageButton(c, "disk-clear", t.clear, s.Cleaning || a.ClearSelection == nil, false, a.ClearSelection)
			appsPageButton(c, "disk-confirm", t.confirm, s.Cleaning || !s.CanRequestDelete || a.RequestDeleteSelected == nil, false, a.RequestDeleteSelected).Background(ui.Hex("#ba1a1a")).TextColor(card)
		})
	})
}
