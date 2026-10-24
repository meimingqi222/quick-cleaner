package ui

import (
	"cmp"
	"fmt"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
	"golang.org/x/text/cases"
	"golang.org/x/text/language"
)

type SearchSortColumn uint8

const (
	SearchSortSize SearchSortColumn = iota
	SearchSortName
	SearchSortPath
)

// SortSearchResults orders a controller-owned result slice, never the index.
// Kind grouping stays ascending regardless of the selected column's direction.
func SortSearchResults(hits []core.SearchHit, group bool, column SearchSortColumn, ascending bool) {
	lower := cases.Lower(language.Und)
	slices.SortStableFunc(hits, func(a, b core.SearchHit) int {
		if group {
			if order := cmp.Compare(FileKind(a.Name, a.IsDir), FileKind(b.Name, b.IsDir)); order != 0 {
				return order
			}
		}
		var order int
		switch column {
		case SearchSortName:
			order = strings.Compare(lower.String(a.Name), lower.String(b.Name))
		case SearchSortPath:
			order = strings.Compare(lower.String(a.Path), lower.String(b.Path))
		default:
			order = cmp.Compare(a.Size, b.Size)
		}
		if !ascending {
			order = -order
		}
		if order != 0 {
			return order
		}
		if order = cmp.Compare(b.Size, a.Size); order != 0 {
			return order
		}
		return strings.Compare(lower.String(a.Name), lower.String(b.Name))
	})
}

// SearchPage presents an already bounded, sorted result snapshot. Index work,
// 120 ms debounce, generation rejection and filesystem actions belong to its owner.
type SearchPage struct {
	Language                   Language
	Ready, Indexing, Searching bool
	Query                      string
	Results                    []core.SearchHit
	GroupByKind                bool
	SortColumn                 SearchSortColumn
	Ascending                  bool
}

type SearchPageActions struct {
	SetQuery          func(string)
	ToggleGroupByKind func()
	SortBy            func(SearchSortColumn)
	Open, Reveal      func(path string)
}

func (s SearchPage) RenderContent(c *ui.Context, a SearchPageActions) {
	c.SetTheme(theme())
	t := searchPageStrings(s.Language)
	ui.Column(c.Key("file-search-view")).Fill().MinWidth(0).MinHeight(0).Padding(20).Gap(16).Background(card).TextColor(text).Children(func() {
		ui.Column(c).FillWidth().Gap(4).Children(func() {
			ui.Text(c, t.heading).FontSize(18).Bold()
			ui.Text(c, t.subheading).FontSize(12).TextColor(muted)
		})
		ui.Row(c).FillWidth().Padding(12).Gap(12).Radius(12).Border(1, outlineVariant.Alpha(.5)).Children(func() {
			ui.Row(c).Width(380).Shrink(0).Height(34).Gap(0).Radius(6).Border(1, outlineVariant.Alpha(.6)).Background(surfaceLow).Children(func() {
				ui.Box(c).Width(30).FillHeight().Center().Children(func() { icon(c, Search, outline, 14) })
				query := s.Query
				input := ui.TextInput(c.Key("file-search-box"), &query).Grow(1).MinWidth(0).FillHeight().Padding(0).FontSize(13).Background(ui.Transparent).Border(0, ui.Transparent).Placeholder(t.placeholder).Label(t.placeholder)
				if input.Changed() && a.SetQuery != nil {
					a.SetQuery(query)
				}
				input.OnTextKey(func(k ui.TextKey) bool {
					if k.Key != ui.KeyEscape || k.Mods != 0 {
						return false
					}
					k.SetText("")
					if a.SetQuery != nil {
						a.SetQuery("")
					}
					return true
				})
				if s.Query != "" {
					button := ui.ButtonBase(c.Key("file-search-clear")).Size(28, 32).Padding(0).Label(t.clear).Tooltip(t.clear).Children(func() { ui.Text(c, "×").FontSize(16).TextColor(outline) })
					if button.Clicked() && a.SetQuery != nil {
						a.SetQuery("")
						input.Focus()
					}
				}
			})
			status, fg := s.status(t)
			ui.Text(c, status).MinWidth(0).Shrink(1).SingleLine().FontSize(12).TextColor(fg).Padding(4, 12).Radius(999).Background(surfaceHigh).Tooltip(status)
			ui.Spacer(c)
			appsPageButton(c, "search-group-kind", t.group, a.ToggleGroupByKind == nil, s.GroupByKind, a.ToggleGroupByKind)
		})
		ui.Column(c).Grow(1).MinHeight(0).FillWidth().Gap(0).Radius(12).Border(1, outlineVariant.Alpha(.5)).Clip().Children(func() {
			if s.Indexing || (s.Searching && len(s.Results) == 0) {
				ui.Column(c).Fill().Center().Gap(8).Children(func() {
					ui.Text(c, t.indexing).FontSize(14).TextColor(muted)
					ui.Text(c, t.building).FontSize(12).TextColor(outline)
				})
				return
			}
			ui.Row(c).FillWidth().Padding(8, 20).Gap(12).Background(surfaceLow).Children(func() {
				for i, col := range []SearchSortColumn{SearchSortName, SearchSortPath, SearchSortSize} {
					searchColumn(c, i).Children(func() {
						label := t.columns[i]
						if s.SortColumn == col {
							if s.Ascending {
								label += " ▲"
							} else {
								label += " ▼"
							}
						}
						tableHeaderButton(c, fmt.Sprintf("search-sort-%d", col), label, s.SortColumn == col, col == SearchSortSize, func() {
							if a.SortBy != nil {
								a.SortBy(col)
							}
						})
					})
				}
				ui.Text(c, t.columns[3]).Width(140).Shrink(0).FontSize(12).TextColor(muted).TextAlign(ui.Center)
			})
			if len(s.Results) == 0 {
				title, hint := s.emptyCopy(t)
				ui.Column(c).Grow(1).MinHeight(0).FillWidth().Center().Gap(8).Children(func() {
					icon(c, Search, outline, 28)
					ui.Text(c, title).FontSize(14).FontWeight(600).TextColor(muted)
					ui.Text(c, hint).FontSize(12).TextColor(outline)
				})
			} else {
				ui.List(c.Key("search-results-rows"), nil, len(s.Results)).Grow(1).MinHeight(0).FillWidth().Gap(0).ItemKey(func(i int) any { return s.Results[i].Path }).Rows(func(row ui.ListRow) {
					s.resultRow(row.Context, s.Results[row.Index], t, a)
				})
			}
			// The count lives in the status chip above; the footer keeps the hints.
			ui.Row(c).FillWidth().Padding(8, 20).Gap(16).Background(surfaceLow).Children(func() {
				ui.Spacer(c)
				ui.Text(c, t.hint).MinWidth(0).SingleLine().FontSize(12).TextColor(muted)
				ui.Text(c, t.limit).Shrink(0).FontSize(12).TextColor(outline)
			})
		})
	})
}

// emptyCopy explains an empty table: no index yet, nothing typed yet, or a
// query that matched nothing. Only the last is a "not found".
func (s SearchPage) emptyCopy(t searchPageCopy) (string, string) {
	switch {
	case !s.Ready:
		return t.notReadyTitle, t.notReadyHint
	case strings.TrimSpace(s.Query) == "":
		return t.startTitle, t.startHint
	default:
		return t.noResults, t.noMatchHint
	}
}

func (s SearchPage) status(t searchPageCopy) (string, ui.Color) {
	switch {
	case s.Indexing:
		return t.building, primary
	case !s.Ready:
		return t.noIndex, caution
	case s.Searching:
		return t.searching, primary
	case strings.TrimSpace(s.Query) == "":
		return fmt.Sprintf(t.top, len(s.Results)), muted
	case len(s.Results) == 0:
		return t.noResults, outline
	default:
		return searchResultCount(s.Language, len(s.Results)), primary
	}
}

func searchColumn(c *ui.Context, i int) ui.Element {
	e := ui.Box(c).MinWidth(0).Clip()
	switch i {
	case 0:
		return e.Width(260).Shrink(0)
	case 2:
		return e.Width(120).Shrink(0)
	default:
		return e.Grow(1)
	}
}

func (s SearchPage) resultRow(c *ui.Context, hit core.SearchHit, t searchPageCopy, a SearchPageActions) {
	row := ui.Row(c.Key(hit.Path)).FillWidth().Height(34).Shrink(0).Padding(0, 20).Gap(12).BorderWidth(0, 0, 1, 0).BorderColor(outlineVariant.Alpha(.2))
	interactiveRow(row, ui.Transparent, surfaceLow)
	row.Children(func() {
		searchColumn(c, 0).Children(func() {
			ui.Row(c).FillWidth().Gap(8).Tooltip(hit.Name).Children(func() {
				FileKind(hit.Name, hit.IsDir).Render(c, 18)
				ui.Text(c, core.Truncate(hit.Name, 35)).Grow(1).MinWidth(0).SingleLine().FontSize(14).FontWeight(500)
			})
		})
		searchColumn(c, 1).Children(func() {
			ui.Text(c, core.Truncate(hit.Path, 110)).FillWidth().SingleLine().FontSize(12).TextColor(muted).Tooltip(hit.Path)
		})
		searchColumn(c, 2).Children(func() {
			fg := text
			if hit.Size >= 1<<30 {
				fg = primary
			}
			ui.Text(c, core.FmtSize(hit.Size)).FillWidth().TextAlign(ui.End).SingleLine().FontSize(12).Bold().TextColor(fg)
		})
		ui.Row(c).Width(140).Shrink(0).Gap(8).Justify(ui.Center).Children(func() {
			appsPageButton(c, "search-open", t.open, a.Open == nil, true, func() { a.Open(hit.Path) }).Padding(3.5, 10)
			appsPageButton(c, "search-reveal", "↗ "+t.reveal, a.Reveal == nil, false, func() { a.Reveal(hit.Path) }).Padding(3.5, 10)
		})
	})
	if row.DoubleClicked() && a.Open != nil {
		a.Open(hit.Path)
	}
}
