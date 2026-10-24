package ui

import (
	"fmt"
	"path/filepath"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
	"golang.org/x/text/cases"
	"golang.org/x/text/language"
)

type DevEnvFilter uint8

const (
	DevEnvAll DevEnvFilter = iota
	DevEnvPython
	DevEnvNode
	DevEnvTools
)

// DevEnvPage is the Apps master/detail subpage. Inventory, package loading,
// selection and removals belong to the controller; rendering never probes paths
// or invokes a package manager. ChangedLabels describe changes, not last use.
type DevEnvPage struct {
	Language                       Language
	Scanning, Removing, Windows    bool
	Assets                         []core.DevAssetItem
	Packages                       map[string][]core.DevAssetItem
	Selected                       map[string]bool
	ChangedLabels                  map[string]string
	ActiveID, LoadingSite, Search  string
	Filter                         DevEnvFilter
	TotalLogical, TotalReclaimable uint64
}

type DevEnvPageActions struct {
	Rescan                                                            func()
	SetSearch                                                         func(string)
	SetFilter                                                         func(DevEnvFilter)
	SelectAsset, ToggleSelected, RequestRemove                        func(string)
	SelectAll, InvertSelection, ClearSelection, RequestRemoveSelected func()
	SelectSitePackages, InvertSitePackages                            func(string)
}

func devEnvGroup(item core.DevAssetItem) int {
	switch item.Kind.Type {
	case core.DevNodePackage:
		return 1
	case core.DevPythonTool:
		return 2
	default:
		return 0
	}
}

func (s DevEnvPage) busy() bool { return s.Scanning || s.Removing }

func (s DevEnvPage) RenderContent(c *ui.Context, a DevEnvPageActions) {
	t := devEnvStrings(s.Language)
	ui.Column(c.Key("dev-env-page")).Grow(1).FillWidth().MinHeight(0).Gap(12).Children(func() {
		ui.Row(c).FillWidth().Gap(16).Children(func() {
			ui.Column(c).Grow(1).MinWidth(0).Clip().Gap(2).Children(func() {
				ui.Text(c, t.title).FontSize(14).Bold()
				ui.Text(c, t.description).FillWidth().FontSize(12).TextColor(outline).SingleLine().Tooltip(t.description)
			})
			for i, value := range []string{fmt.Sprint(len(s.Assets)), core.FmtSize(s.TotalReclaimable), core.FmtSize(s.TotalLogical)} {
				ui.Row(c).Shrink(0).Padding(3, 10).Gap(6).Radius(999).Background(surfaceHigh).Border(1, outlineVariant.Alpha(.4)).Children(func() {
					ui.Text(c, t.metrics[i]).FontSize(12).TextColor(outline)
					fg := muted
					if i == 1 {
						fg = primary
					}
					ui.Text(c, value).FontSize(12).Bold().TextColor(fg)
				})
			}
			appsPageButton(c, "dev-rescan", t.rescan, s.busy() || a.Rescan == nil, false, a.Rescan).Shrink(0)
		})
		if s.Scanning {
			ui.Column(c).Grow(1).FillWidth().Center().Gap(12).Children(func() {
				ui.Text(c, t.title).FontSize(14).Bold()
				ui.Text(c, t.description).MaxWidth(480).FontSize(12).TextColor(outline)
			})
			return
		}
		query := cases.Lower(language.Und).String(strings.TrimSpace(s.Search))
		ui.Row(c).Grow(1).FillWidth().MinHeight(0).Gap(12).Children(func() {
			s.master(c, a, t, query)
			s.detail(c, a, t, query)
		})
		s.actionBar(c, a, t)
	})
}

func (s DevEnvPage) matches(item core.DevAssetItem, query string) bool {
	lower := cases.Lower(language.Und)
	if query == "" || strings.Contains(lower.String(item.DisplayName()), query) || strings.Contains(lower.String(item.Path), query) {
		return true
	}
	for _, p := range s.Packages[item.ID] {
		if strings.Contains(lower.String(p.DisplayName()), query) {
			return true
		}
	}
	return false
}

func (s DevEnvPage) master(c *ui.Context, a DevEnvPageActions, t devEnvCopy, query string) {
	counts := [4]int{len(s.Assets)}
	for _, item := range s.Assets {
		counts[devEnvGroup(item)+1]++
	}
	ui.Column(c).Width(360).Shrink(0).FillHeight().MinHeight(0).Radius(12).Border(1, outlineVariant.Alpha(.5)).Clip().Gap(0).Children(func() {
		ui.Column(c).FillWidth().Padding(12).Gap(8).BorderWidth(0, 0, 1, 0).BorderColor(outlineVariant.Alpha(.4)).Children(func() {
			ui.Row(c).FillWidth().Wrap().Gap(4).Children(func() {
				for i, label := range t.filters {
					appsPageButton(c, fmt.Sprintf("dev-filter-%d", i), fmt.Sprintf("%s (%d)", label, counts[i]), a.SetFilter == nil, s.Filter == DevEnvFilter(i), func() { a.SetFilter(DevEnvFilter(i)) }).Padding(3, 8).Radius(999)
				}
			})
			query := s.Search
			input := ui.TextInput(c.Key("dev-search"), &query).FillWidth().Height(32).FontSize(12).Placeholder(t.search).Label(t.search)
			if input.Changed() && a.SetSearch != nil {
				a.SetSearch(query)
			}
		})
		ui.Scroll(c.Key("dev-master-scroll")).Grow(1).FillWidth().MinHeight(0).Padding(8).Gap(4).Children(func() {
			visible := 0
			for group := range 3 {
				if s.Filter != DevEnvAll && int(s.Filter) != group+1 {
					continue
				}
				var items []core.DevAssetItem
				for _, item := range s.Assets {
					if devEnvGroup(item) == group && s.matches(item, query) {
						items = append(items, item)
					}
				}
				if len(items) == 0 {
					continue
				}
				visible += len(items)
				ui.Text(c, fmt.Sprintf("%s (%d)", t.sections[group], len(items))).FillWidth().Padding(8, 8, 4, 8).FontSize(12).Bold().TextColor(muted)
				for _, item := range items {
					s.masterRow(c, a, t, item)
				}
			}
			if visible == 0 {
				ui.Text(c, t.empty).Padding(20).FontSize(12).TextColor(outline)
			}
		})
	})
}

func (s DevEnvPage) check(c *ui.Context, a DevEnvPageActions, item core.DevAssetItem) {
	if !core.CanRemoveDevAsset(item) {
		ui.Box(c).Size(16, 16).Shrink(0)
		return
	}
	selected := s.Selected[item.ID]
	check := ui.Checkbox(c.Key("dev-check-"+item.ID), &selected, "").Label(fmt.Sprintf(devEnvStrings(s.Language).selectionLabel, item.DisplayName())).Disabled(s.busy() || a.ToggleSelected == nil)
	if check.Changed() && a.ToggleSelected != nil {
		a.ToggleSelected(item.ID)
	}
}

func (s DevEnvPage) masterRow(c *ui.Context, a DevEnvPageActions, t devEnvCopy, item core.DevAssetItem) {
	row := ui.ButtonBase(c.Key("dev-asset-" + item.ID)).FillWidth().Padding(8).Gap(6).Radius(8).Justify(ui.Start)
	if item.ID == s.ActiveID {
		row.Background(primaryFixed)
	} else if row.Hovered() {
		row.Background(surfaceLow)
	}
	row.Children(func() {
		s.check(c, a, item)
		ui.Column(c).Grow(1).MinWidth(0).Clip().Gap(1).Children(func() {
			ui.Row(c).Gap(8).Children(func() {
				ui.Text(c, item.DisplayName()).MinWidth(0).FontSize(12).FontWeight(600).NoWrap().MaxLines(1)
				badge, accent := string(item.Kind.Type), false
				switch item.Kind.Type {
				case core.DevCondaEnv:
					badge = "Conda"
					if item.Kind.IsBase {
						badge, accent = t.base, true
					}
				case core.DevPythonInterpreter:
					badge = "System"
					if item.Kind.Scope == core.SiteUser {
						badge, accent = "User", true
					}
				case core.DevVirtualEnv:
					badge = string(item.Kind.VenvManager)
				case core.DevNodePackage:
					badge = item.Kind.Manager
				case core.DevPythonTool:
					badge, accent = string(item.Kind.ToolKind), item.Kind.ToolKind == core.PythonToolUv
				case core.DevPipPackage:
					badge = "pip"
				}
				devBadge(c, badge, accent)
			})
			sub := s.displayPath(item.Path)
			if item.Kind.PackageCount != nil {
				sub = fmt.Sprintf(t.packages, *item.Kind.PackageCount)
			}
			if item.Kind.Type == core.DevNodePackage && item.Kind.Version != nil {
				sub = "v" + *item.Kind.Version
			}
			if item.Kind.Type == core.DevPythonTool && len(item.Kind.Executables) > 0 {
				sub = strings.Join(item.Kind.Executables, ", ")
			}
			ui.Text(c, sub).FillWidth().FontSize(12).TextColor(outline).SingleLine()
		})
		ui.Text(c, core.FmtSize(item.Size.ExclusiveReclaimableBytes)).Shrink(0).FontSize(12).TextColor(muted)
		if item.ID == s.ActiveID {
			ui.Text(c, "▸").FontSize(12).TextColor(primary)
		}
	})
	if (row.Clicked() || row.RightClicked()) && a.SelectAsset != nil {
		a.SelectAsset(item.ID)
	}
}

func devBadge(c *ui.Context, label string, accent bool) {
	fg, bg := text, surfaceHigh
	if accent {
		fg, bg = primary, primaryFixed
	}
	ui.Text(c, label).Shrink(0).FontSize(12).Padding(1, 8).Radius(999).TextColor(fg).Background(bg)
}

func (s DevEnvPage) displayPath(path string) string {
	if s.Windows {
		return strings.ReplaceAll(path, "/", `\`)
	}
	return path
}

func (s DevEnvPage) detail(c *ui.Context, a DevEnvPageActions, t devEnvCopy, query string) {
	ui.Column(c.Key("dev-detail")).Grow(1).MinWidth(0).FillHeight().MinHeight(0).Radius(12).Border(1, outlineVariant.Alpha(.5)).Clip().Gap(0).Children(func() {
		var active *core.DevAssetItem
		for i := range s.Assets {
			if s.Assets[i].ID == s.ActiveID {
				active = &s.Assets[i]
				break
			}
		}
		if active == nil {
			ui.Column(c).Grow(1).FillWidth().Center().Children(func() { ui.Text(c, t.noSelection).FontSize(14).TextColor(outline) })
			return
		}
		item := *active
		removable := core.CanRemoveDevAsset(item)
		ui.Column(c).FillWidth().Padding(16).Gap(12).Background(surfaceLow).BorderWidth(0, 0, 1, 0).BorderColor(outlineVariant.Alpha(.4)).Children(func() {
			ui.Row(c).FillWidth().Gap(12).Children(func() {
				ui.Column(c).Grow(1).MinWidth(0).Clip().Gap(4).Children(func() {
					ui.Row(c).Gap(8).Children(func() {
						ui.Text(c, item.DisplayName()).MinWidth(0).FontSize(16).Bold().NoWrap().MaxLines(1)
						if item.IsActiveEnv {
							ui.Text(c, t.active).Shrink(0).FontSize(12).Padding(1, 8).Radius(999).TextColor(caution).Background(cautionContainer)
						}
						if !removable {
							devBadge(c, t.displayOnly, false)
						}
					})
					ui.Text(c, s.displayPath(item.Path)).FillWidth().FontSize(12).TextColor(outline).SingleLine().Tooltip(s.displayPath(item.Path))
				})
				if removable {
					s.removeButton(c, a, t, item)
				}
			})
			changed := s.ChangedLabels[item.ID]
			if changed == "" {
				changed = t.unknown
			}
			ui.Row(c).FillWidth().Gap(16).Children(func() {
				for i, value := range []string{core.FmtSize(item.Size.ExclusiveReclaimableBytes), core.FmtSize(item.Size.LogicalBytes), changed} {
					ui.Column(c).Grow(1).MinWidth(0).Padding(12).Gap(4).Radius(8).Background(card).Children(func() {
						ui.Text(c, []string{t.exclusive, t.size, t.changed}[i]).FontSize(12).TextColor(outline)
						fg := muted
						if i == 0 {
							fg = primary
						}
						ui.Text(c, value).FontSize(14).Bold().TextColor(fg)
					})
				}
			})
		})
		container := item.Kind.Type == core.DevVirtualEnv || item.Kind.Type == core.DevPythonInterpreter && item.Kind.Scope == core.SiteUser && item.Kind.SitePackages != nil
		if container {
			s.packageList(c, a, t, item, query)
			return
		}
		ui.Scroll(c.Key("dev-profile-" + item.ID)).Grow(1).FillWidth().MinHeight(0).Padding(24).Gap(16).Children(func() {
			ui.Text(c, t.path).FontSize(12).Bold().TextColor(outline)
			ui.Text(c, s.displayPath(item.Path)).FillWidth().Padding(12).FontSize(12).Radius(6).Background(surfaceLow).Border(1, outlineVariant.Alpha(.3))
			executables := item.Kind.Executables
			if item.Kind.Type == core.DevNodePackage {
				executables = item.Kind.BinShims
			}
			if len(executables) > 0 {
				ui.Text(c, t.executables).FontSize(12).Bold().TextColor(outline)
				ui.Row(c).FillWidth().Wrap().Padding(12).Gap(8).Radius(6).Background(surfaceLow).Children(func() {
					for _, executable := range executables {
						devBadge(c, executable, true)
					}
				})
			}
		})
	})
}

func (s DevEnvPage) removeButton(c *ui.Context, a DevEnvPageActions, t devEnvCopy, item core.DevAssetItem) {
	appsPageButton(c, "dev-remove-"+item.ID, t.remove, s.busy() || a.RequestRemove == nil, false, func() { a.RequestRemove(item.ID) }).Background(ui.Hex("#ffdad6")).TextColor(ui.Hex("#ba1a1a"))
}

func (s DevEnvPage) packageList(c *ui.Context, a DevEnvPageActions, t devEnvCopy, parent core.DevAssetItem, query string) {
	packages, loaded := s.Packages[parent.ID]
	if !loaded || s.LoadingSite == parent.ID || len(packages) == 0 {
		label := t.loading
		if loaded && s.LoadingSite != parent.ID {
			label = t.noPackages
		}
		ui.Column(c).Grow(1).FillWidth().Center().Children(func() { ui.Text(c, label).FontSize(12).TextColor(outline) })
		return
	}
	lower := cases.Lower(language.Und)
	var visible []core.DevAssetItem
	for _, item := range packages {
		if query == "" || strings.Contains(lower.String(item.DisplayName()), query) {
			visible = append(visible, item)
		}
	}
	ui.Row(c).FillWidth().Padding(8, 16).Gap(8).BorderWidth(0, 0, 1, 0).BorderColor(outlineVariant.Alpha(.3)).Children(func() {
		ui.Text(c, fmt.Sprintf(t.matched, len(visible), len(packages))).FontSize(12).TextColor(muted)
		ui.Spacer(c)
		appsPageButton(c, "dev-site-all", t.siteAll, s.busy() || a.SelectSitePackages == nil, false, func() { a.SelectSitePackages(parent.ID) })
		appsPageButton(c, "dev-site-invert", t.invert, s.busy() || a.InvertSitePackages == nil, false, func() { a.InvertSitePackages(parent.ID) })
	})
	ui.Scroll(c.Key("dev-packages-" + parent.ID)).Grow(1).FillWidth().MinHeight(0).Padding(12).Gap(4).Children(func() {
		for _, item := range visible {
			ui.Row(c.Key("dev-package-" + item.ID)).FillWidth().Padding(8).Gap(8).Radius(6).Children(func() {
				s.check(c, a, item)
				ui.Column(c).Grow(1).MinWidth(0).Clip().Gap(2).Children(func() {
					ui.Row(c).Gap(8).Children(func() {
						ui.Text(c, item.DisplayName()).MinWidth(0).NoWrap().MaxLines(1).FontSize(14).Bold()
						if item.Kind.Version != nil {
							devBadge(c, "v"+*item.Kind.Version, true)
						}
						if changed := s.ChangedLabels[item.ID]; changed != "" {
							ui.Text(c, "· "+changed).FontSize(12).TextColor(outline)
						}
					})
					path := filepath.Base(item.Path)
					if rel, err := filepath.Rel(parent.Path, item.Path); err == nil && rel != "." && rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
						path = rel
					}
					ui.Text(c, "📁 "+s.displayPath(path)).FillWidth().FontSize(12).TextColor(muted).SingleLine().Tooltip(s.displayPath(item.Path))
				})
				if core.CanRemoveDevAsset(item) {
					s.removeButton(c, a, t, item)
				}
			})
		}
	})
}

func (s DevEnvPage) actionBar(c *ui.Context, a DevEnvPageActions, t devEnvCopy) {
	count, bytes := 0, uint64(0)
	counted := map[string]bool{}
	countItem := func(item core.DevAssetItem) {
		if s.Selected[item.ID] && !counted[item.ID] {
			count++
			bytes += item.Size.ExclusiveReclaimableBytes
			counted[item.ID] = true
		}
	}
	for _, item := range s.Assets {
		countItem(item)
	}
	for _, packages := range s.Packages {
		for _, item := range packages {
			countItem(item)
		}
	}
	ui.Row(c).FillWidth().Justify(ui.Center).Children(func() {
		ui.Row(c.Key("dev-bottom-bar")).Padding(8, 16).Gap(16).Radius(999).Border(1, outlineVariant.Alpha(.6)).Background(card).Shadow(0, 10, 15, -3, ui.RGBA(0, 0, 0, .1)).Children(func() {
			ui.Column(c).Gap(0).Children(func() {
				ui.Text(c, t.selected).FontSize(12).TextColor(outline)
				summary := t.zeroSelection
				if count > 0 {
					summary = fmt.Sprintf(t.selection, count, core.FmtSize(bytes))
				}
				ui.Text(c, summary).FontSize(14).Bold().TextColor(muted)
			})
			ui.Box(c).Width(1).Height(20).Background(outlineVariant)
			for i, action := range []func(){a.SelectAll, a.InvertSelection, a.ClearSelection} {
				appsPageButton(c, fmt.Sprintf("dev-batch-select-%d", i), []string{t.all, t.invert, t.clear}[i], s.busy() || action == nil || i == 2 && count == 0, false, action)
			}
			appsPageButton(c, "dev-batch-remove", fmt.Sprintf(t.batchRemove, count), s.busy() || count == 0 || a.RequestRemoveSelected == nil, false, a.RequestRemoveSelected).Background(ui.Hex("#ba1a1a")).TextColor(card)
		})
	})
}
