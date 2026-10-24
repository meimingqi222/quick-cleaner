package ui

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

type AppsPageTab uint8

const (
	AppsPageDesktop AppsPageTab = iota
	AppsPageDevEnvironments
)

type AppsPageUninstallPhase uint8

const (
	AppsPageDiscovering AppsPageUninstallPhase = iota
	AppsPageRemoving
	AppsPageVerifying
)

// AppsPageApp contains display values and controller-resolved capabilities.
// ID must be unique and stable across filtering, sorting and inventory refreshes.
// No filesystem probes or platform-dependent CanUninstall calls happen in the view.
type AppsPageApp struct {
	ID, Name, Version, Publisher, LastUsed, InstalledDate, Size string
	Icon                                                        *ui.Bitmap
	Discovered, CanUninstall, HasLocation                       bool
	ManualUninstall, UninstallDeletesData                       bool
}

// UninstallGuidance supplies the existing native-policy copy for the parent's
// confirmation/guidance flow. ManualUninstall must never trigger main-app deletion.
func (app AppsPageApp) UninstallGuidance(l Language) string {
	t := appsPageStrings(l)
	if app.ManualUninstall {
		return fmt.Sprintf(t.manual, app.Name)
	}
	if app.UninstallDeletesData {
		return t.dataWarning
	}
	return ""
}

type AppsPageResidual struct {
	ID, Path, Size   string
	Confidence       core.Confidence
	Source           core.ResidualSource
	Kind             core.ResidualType
	Selected, Manual bool
}

// AppsPage is a read-only presentation snapshot. View holds the controller's
// filtered/sorted Apps indices (including an intentionally empty result).
// All sizes, counts, permissions and native outcome messages come from the
// controller. Rendering never authorizes deletion or infers success from exit codes.
type AppsPage struct {
	Language                                                  Language
	Tab                                                       AppsPageTab
	MacOS, Scanning, ResidualScanning, Cleaning, Uninstalling bool
	UninstallPhase                                            AppsPageUninstallPhase
	UninstallAppName                                          string
	Apps                                                      []AppsPageApp
	View                                                      []int
	Preset                                                    core.AppFilterPreset
	Sort                                                      core.AppSortState
	Search, TotalSize, FilteredSize                           string
	StaleCount                                                int
	// ResidualsOpen presents the existing residual review, not a destructive fallback.
	ResidualsOpen                               bool
	OrphanLeftovers                             bool
	ResidualAppName, ResidualSize, SelectedSize string
	Residuals                                   []AppsPageResidual
	// Notices preserve native occupancy, manual/reboot and verification distinctions.
	// The controller supplies bilingual payloads; an empty notice is not a success.
	// Occupancy is explanatory scan evidence, not a deletion gate.
	ResidualTitle, ResidualDescription, Notice core.Text
	Occupancy                                  core.ResidualOccupancy
}

type AppsPageActions struct {
	SetTab                                                              func(AppsPageTab)
	SetPreset                                                           func(core.AppFilterPreset)
	SetSearch                                                           func(string)
	SortBy                                                              func(core.AppSortColumn)
	ScanOrphanLeftovers                                                 func()
	RequestUninstall, ScanResiduals, OpenLocation, CopyInstallPath      func(id string)
	RenderDevEnvironments                                               func(*ui.Context)
	SetResidualSelected                                                 func(id string, selected bool)
	SelectRecommended, SelectAllResiduals, ClearResidualSelection       func()
	CloseResiduals, CleanSelectedResiduals, OpenSystemExtensionSettings func()
}

// uninstallProgress mirrors the Rust uninstall page: the apps view is replaced by
// a full-size card holding a 560px centred column, an app badge, the phase meter
// and one row per stage.
func (s AppsPage) uninstallProgress(c *ui.Context, t appsPageCopy) {
	phase := int(s.UninstallPhase)
	if phase >= len(t.phases) {
		phase = len(t.phases) - 1
	}
	title := fmt.Sprintf("正在卸载「%s」", s.UninstallAppName)
	if s.Language == English {
		title = fmt.Sprintf("Uninstalling \"%s\"", s.UninstallAppName)
	}
	ratio := []float32{0.24, 0.64, 0.92}[phase]
	ui.Column(c.Key("apps-uninstall-progress")).Fill().FillWidth().Background(card).Padding(40).Center().Children(func() {
		ui.Column(c).Width(560).Center().Gap(20).Children(func() {
			ui.Box(c).Size(64, 64).Shrink(0).Radius(16).Background(primaryFixed).Border(1, primary.Alpha(.4)).Center().Children(func() {
				icon(c, Apps, primary, 28)
			})
			ui.Column(c).Center().Gap(8).Children(func() {
				ui.Text(c, title).FontSize(24).Bold().TextColor(text)
				ui.Text(c, t.phases[phase]).FontSize(14).TextColor(muted)
			})
			ui.Box(c).Width(420).Height(6).Radius(999).Clip().Background(surfaceHighest).Children(func() {
				ui.Box(c).Height(6).Width(ratio * 420).Radius(999).Background(primary)
			})
			ui.Column(c).Width(420).Padding(8, 0).Children(func() {
				for i, label := range t.stages {
					fg := outline
					if i <= phase {
						fg = text
					}
					row := ui.Row(c).FillWidth().Height(36).Gap(12)
					if i == phase {
						row.Children(func() {
							loadingSpinner(c, 24)
							ui.Text(c, label).FontSize(14).Bold().TextColor(fg)
						})
					} else if i < phase {
						row.Children(func() {
							ui.Box(c).Size(24, 24).Radius(999).Background(primary).Center().Children(func() {
								ui.Text(c, "✓").FontSize(12).TextColor(card)
							})
							ui.Text(c, label).FontSize(14).TextColor(fg)
						})
					} else {
						row.Children(func() {
							ui.Box(c).Size(24, 24).Radius(999).Border(1, outlineVariant)
							ui.Text(c, label).FontSize(14).TextColor(fg)
						})
					}
				}
			})
			ui.Text(c, t.keepOpen).FontSize(12).TextColor(outline)
		})
	})
}

func (s AppsPage) busy() bool {
	return s.ResidualScanning || s.Cleaning || s.Uninstalling
}

// RenderContent renders only the Apps body and its residual-review action bar.
// Navigation, title bar, refresh, global progress and confirmation ownership stay
// with the parent shell. Callbacks request operations; they do not bypass confirmation.
func (s AppsPage) RenderContent(c *ui.Context, actions AppsPageActions) {
	c.SetTheme(theme())
	t := appsPageStrings(s.Language)
	if s.Uninstalling {
		// Rust replaces the whole apps view with the uninstall page. Keeping the
		// tabs, stat cards and list behind it painted them through the progress.
		ui.Column(c.Key("apps-page")).Fill().MinWidth(0).MinHeight(0).Gap(0).Background(card).TextColor(text).Children(func() {
			s.uninstallProgress(c, t)
		})
		return
	}
	ui.Column(c.Key("apps-page")).Fill().MinWidth(0).MinHeight(0).Gap(0).Background(card).TextColor(text).Children(func() {
		ui.Column(c).Grow(1).MinHeight(0).FillWidth().Padding(20).Gap(16).Children(func() {
			ui.Row(c).FillWidth().Gap(16).Children(func() {
				ui.Column(c).Grow(1).MinWidth(0).Gap(4).Children(func() {
					ui.Text(c, t.heading).FontSize(18).Bold()
					ui.Text(c, t.subheading).FontSize(12).TextColor(muted)
				})
				ui.Row(c).Padding(2).Gap(4).Radius(8).Background(surfaceHigh).Children(func() {
					for tab, label := range []string{t.desktop, t.dev} {
						appsPageButton(c, fmt.Sprintf("apps-tab-%d", tab), label, false, s.Tab == AppsPageTab(tab), func() {
							if actions.SetTab != nil {
								actions.SetTab(AppsPageTab(tab))
							}
						})
					}
				})
				if s.MacOS {
					appsPageButton(c, "apps-orphans", t.leftovers, s.busy(), false, actions.ScanOrphanLeftovers)
				}
			})
			if s.Tab == AppsPageDevEnvironments {
				if actions.RenderDevEnvironments != nil {
					actions.RenderDevEnvironments(c)
				}
				return
			}
			if notice := s.Notice.Get(s.Language); notice != "" {
				ui.Text(c, notice).FillWidth().FontSize(12).Padding(12).Radius(8).Background(cautionContainer).TextColor(caution)
			}
			ui.Row(c).FillWidth().Gap(16).Children(func() {
				values := []string{s.TotalSize, fmt.Sprintf(t.count, len(s.Apps)), fmt.Sprintf(t.count, s.StaleCount)}
				for i, label := range t.stats {
					ui.Row(c).Grow(1).Basis(0).MinWidth(0).Padding(16).Gap(12).Border(1, outlineVariant.Alpha(.5)).Radius(12).Children(func() {
						bg := primaryFixed
						if i == 2 {
							bg = cautionContainer
						}
						ui.Box(c).Size(36, 36).Shrink(0).Radius(10).Background(bg).Center().Children(func() {
							switch i {
							case 0:
								icon(c, Disk, primary, 18)
							case 1:
								icon(c, Apps, primary, 18)
							case 2:
								ui.Box(c).Size(18, 18).Radius(999).Border(1.5, caution).Children(func() {
									ui.Box(c).Absolute().Left(7).Top(3).Size(1.5, 5).Background(caution)
									ui.Box(c).Absolute().Left(7).Top(7).Size(5, 1.5).Background(caution)
								})
							}
						})
						ui.Column(c).MinWidth(0).Gap(4).Children(func() {
							ui.Text(c, label).FontSize(12).TextColor(muted)
							fg := text
							if i == 2 {
								fg = caution
							}
							ui.Text(c, values[i]).FontSize(20).Bold().TextColor(fg)
						})
					})
				}
			})
			s.controls(c, actions, t)
			s.table(c, actions, t)
		})
	})
	if s.ResidualsOpen {
		s.residualReview(c, actions, t)
	}
}

func appsPageButton(c *ui.Context, key, label string, disabled, accent bool, action func()) ui.Element {
	button := ui.ButtonBase(c.Key(key)).Padding(5, 10).Radius(6).Border(1, outlineVariant.Alpha(.6)).Disabled(disabled).Cursor(ui.CursorPointer)
	fg := muted
	if accent {
		interactive(button, primaryFixed, primary.Alpha(.2), primary.Alpha(.28), !disabled)
		fg = primary
	} else {
		interactive(button, ui.Transparent, surfaceLow, surfaceHigh, !disabled)
	}
	if disabled {
		fg = outline
	}
	button.TextColor(fg).Children(func() { ui.Text(c, label).FontSize(12).FixedLineHeight(16) })
	if button.Clicked() && !disabled && action != nil {
		action()
	}
	return button
}

func (s AppsPage) controls(c *ui.Context, a AppsPageActions, t appsPageCopy) {
	ui.Row(c).FillWidth().Padding(12).Gap(8).Border(1, outlineVariant.Alpha(.5)).Radius(12).Children(func() {
		ui.Text(c, t.presets).FontSize(12).TextColor(outline)
		for preset := core.AppFilterAll; preset <= core.AppFilterOrphan; preset++ {
			appsPageButton(c, fmt.Sprintf("apps-preset-%d", preset), preset.LabelLang(s.Language), false, s.Preset == preset, func() {
				if a.SetPreset != nil {
					a.SetPreset(preset)
				}
			})
		}
		label := fmt.Sprintf(t.filterAll, len(s.View))
		if s.Search != "" {
			label = fmt.Sprintf(t.filterMatch, len(s.View), len(s.Apps))
		}
		ui.Text(c, label).FontSize(12).TextColor(outline).MinWidth(0)
		ui.Spacer(c)
		query := s.Search
		input := ui.TextInput(c.Key("apps-search"), &query).Width(240).Height(32).FontSize(12).Placeholder(t.search).Label(t.search)
		if input.Changed() && a.SetSearch != nil {
			a.SetSearch(query)
		}
		if s.Search != "" {
			appsPageButton(c, "apps-clear-search", "×", false, false, func() {
				if a.SetSearch != nil {
					a.SetSearch("")
				}
			})
		}
	})
}

var appsPageColumns = []core.AppSortColumn{core.AppSortName, core.AppSortPublisher, core.AppSortLastUsed, core.AppSortInstallDate, core.AppSortSize}

func (s AppsPage) column(c *ui.Context, i int) ui.Element {
	e := ui.Box(c).Shrink(0).MinWidth(0).Clip()
	if i == 0 {
		return e.Grow(1).Shrink(1)
	}
	return e.Width([]float32{0, 130, 110, 100, 95}[i])
}

// tableHeaderButton is a sortable column title: flat like the rows below it
// and aligned with their content, end-aligned for numeric columns.
func tableHeaderButton(c *ui.Context, key, label string, active, end bool, action func()) ui.Element {
	b := ui.ButtonBase(c.Key(key)).FillWidth().Padding(4, 6).Radius(6).Cursor(ui.CursorPointer)
	if end {
		b.Justify(ui.End)
	} else {
		b.Justify(ui.Start)
	}
	interactive(b, ui.Transparent, surfaceHigh, surfaceHighest, true)
	fg := muted
	if active {
		fg = primary
	}
	b.Children(func() { ui.Text(c, label).FontSize(12).FixedLineHeight(16).FontWeight(600).TextColor(fg).SingleLine() })
	if b.Clicked() && action != nil {
		action()
	}
	return b
}

func (s AppsPage) table(c *ui.Context, a AppsPageActions, t appsPageCopy) {
	ui.Column(c).Grow(1).MinHeight(0).FillWidth().Gap(0).Border(1, outlineVariant.Alpha(.5)).Radius(12).Clip().Children(func() {
		ui.Row(c).FillWidth().Padding(8, 20).Gap(12).Background(surfaceLow).Children(func() {
			for i, col := range appsPageColumns {
				s.column(c, i).Children(func() {
					label := t.columns[i]
					if i == 0 {
						label = fmt.Sprintf(t.name, len(s.View))
					}
					tableHeaderButton(c, "apps-sort-"+col.ID(), label+s.Sort.Indicator(col), s.Sort.Column == col, i == len(appsPageColumns)-1, func() {
						if a.SortBy != nil {
							a.SortBy(col)
						}
					})
				})
			}
			ui.Text(c, t.columns[5]).Width(s.actionsWidth()).Shrink(0).FontSize(12).TextColor(outline).TextAlign(ui.Center)
		})
		// Directory measurement keeps Scanning set. Rows already found stay
		// visible; the placeholder is only for a list that is still empty.
		if len(s.View) == 0 {
			ui.Scroll(c.Key("apps-placeholder")).Grow(1).MinHeight(0).FillWidth().Children(func() {
				ui.Column(c).FillWidth().Padding(48, 20).Center().Gap(8).Children(func() {
					label := t.empty
					if s.Scanning {
						label = t.loading
					}
					ui.Text(c, label).FontSize(14).TextColor(muted)
					if s.Scanning {
						ui.Text(c, t.loadingSub).FontSize(12).TextColor(outline)
					}
				})
			})
		} else {
			ui.List(c.Key("apps-rows"), nil, len(s.View)).Grow(1).MinHeight(0).FillWidth().Gap(0).ItemKey(func(i int) any {
				index := s.View[i]
				if index >= 0 && index < len(s.Apps) {
					return s.Apps[index].ID
				}
				return index
			}).Rows(func(row ui.ListRow) {
				index := s.View[row.Index]
				if index >= 0 && index < len(s.Apps) {
					s.appRow(row.Context, a, t, s.Apps[index])
				}
			})
		}
		ui.Row(c).FillWidth().Padding(8, 20).Gap(12).Background(surfaceLow).Children(func() {
			format := t.footer
			if s.Search != "" {
				format = t.footerMatch
			}
			ui.Text(c, fmt.Sprintf(format, len(s.View), len(s.Apps))).FontSize(12).TextColor(outline)
			ui.Spacer(c)
			ui.Text(c, fmt.Sprintf(t.size, s.FilteredSize)).FontSize(12).Bold()
		})
	})
}

func (s AppsPage) actionsWidth() float32 {
	if s.MacOS {
		return 120
	}
	return 190
}

func (s AppsPage) appRow(c *ui.Context, a AppsPageActions, t appsPageCopy, app AppsPageApp) {
	menu := func(m *ui.Menu) {
		if m.Item(t.open).Disabled(!app.HasLocation || a.OpenLocation == nil).Chosen() {
			a.OpenLocation(app.ID)
		}
		if m.Item(t.standard).Disabled(!app.CanUninstall || s.busy() || s.Scanning || a.RequestUninstall == nil).Chosen() {
			a.RequestUninstall(app.ID)
		}
		if !s.MacOS {
			label := t.forceMenu
			if app.Discovered {
				label = t.removeMenu
			}
			if m.Item(label).Disabled(s.busy() || s.Scanning || a.ScanResiduals == nil).Chosen() {
				a.ScanResiduals(app.ID)
			}
		}
		if m.Item(t.copy).Disabled(a.CopyInstallPath == nil).Chosen() {
			a.CopyInstallPath(app.ID)
		}
	}
	row := ui.Row(c.Key("apps-row-"+app.ID)).FillWidth().Height(54).Shrink(0).Padding(0, 20).Gap(12).BorderWidth(0, 0, 1, 0).BorderColor(outlineVariant.Alpha(.3)).ContextMenu(menu)
	if guidance := app.UninstallGuidance(s.Language); guidance != "" {
		row.Tooltip(guidance)
	}
	row.Children(func() {
		s.column(c, 0).Children(func() {
			ui.Row(c).FillWidth().Gap(12).Children(func() {
				if app.Icon != nil {
					ui.Image(c, app.Icon).Size(32, 32).Shrink(0)
				} else {
					initial := "?"
					if r := []rune(app.Name); len(r) > 0 {
						initial = strings.ToUpper(string(r[0]))
					}
					ui.Box(c).Size(32, 32).Shrink(0).Center().Children(func() { ui.Text(c, initial).FontSize(12).Bold().TextColor(primary) })
				}
				ui.Column(c).MinWidth(0).Clip().Gap(0).Children(func() {
					ui.Text(c, app.Name).FontSize(14).FixedLineHeight(20).Bold().NoWrap().MaxLines(1)
					if app.Discovered {
						label := t.discovered
						if app.CanUninstall {
							label = t.official
						}
						ui.Text(c, label).FontSize(12).TextColor(outline)
					} else if app.Version != "" {
						ui.Text(c, "v"+app.Version).FontSize(12).TextColor(outline)
					}
				})
			})
		})
		for j, value := range []string{app.Publisher, app.LastUsed, app.InstalledDate, app.Size} {
			s.column(c, j+1).Children(func() {
				if value == "" {
					value = "—"
				}
				e := ui.Text(c, value).FontSize(12).TextColor(muted)
				if j == 3 {
					e.FillWidth().TextAlign(ui.End).Bold()
				}
			})
		}
		ui.Row(c).Width(s.actionsWidth()).Shrink(0).Gap(6).Justify(ui.End).Children(func() {
			appsPageButton(c, "apps-uninstall-"+app.ID, t.uninstall, !app.CanUninstall || s.busy() || s.Scanning || a.RequestUninstall == nil, false, func() { a.RequestUninstall(app.ID) }).Padding(4, 8)
			if !s.MacOS {
				label := t.force
				if app.Discovered {
					label = t.remove
				}
				appsPageButton(c, "apps-residual-"+app.ID, label, s.busy() || s.Scanning || a.ScanResiduals == nil, true, func() { a.ScanResiduals(app.ID) }).Padding(4, 8)
			}
			ui.ButtonBase(c.Key("apps-more-"+app.ID)).Size(24, 24).Padding(0).Radius(6).Border(1, outlineVariant.Alpha(.6)).Label("···").Menu(menu).Children(func() {
				ui.Text(c, "···").FontSize(12).TextColor(muted)
			})
		})
	})
}
