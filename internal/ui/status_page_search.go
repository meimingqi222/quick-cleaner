package ui

import (
	"fmt"
	"slices"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

type statusSearchState struct {
	open      bool
	selected  string
	previous  string
	completed bool
	list      ui.ListState
}

type statusCandidate struct {
	id, query, label, detail string
	keepOpen                 bool
}

func statusCompletion(template, query string) (string, bool) {
	compact := strings.ToLower(strings.Map(func(r rune) rune {
		if unicode.IsSpace(r) {
			return -1
		}
		return r
	}, query))
	for _, field := range []string{"mem", "cpu"} {
		if strings.HasPrefix(template, field) && strings.HasPrefix(compact, field) {
			rest := strings.TrimPrefix(compact, field)
			if rest == "" {
				return field + " > ", true
			}
			if rest == ">" || rest == "<" || rest == ">=" || rest == "<=" {
				return field + " " + rest + " ", true
			}
			return "", false
		}
	}
	flat := strings.ToLower(strings.ReplaceAll(template, " ", ""))
	return template, strings.Contains(flat, compact)
}

func statusPortQuery(query string) bool {
	q := strings.ToLower(strings.TrimSpace(query))
	return strings.HasPrefix(q, ":") || strings.HasPrefix(q, "port:")
}

func (s *StatusPage) candidates(filtered []core.ProcInfo, query string) []statusCandidate {
	if statusPortQuery(query) {
		_, sub, _ := strings.Cut(query, ":")
		sub = strings.ToLower(strings.TrimSpace(sub))
		type portEntry struct {
			port uint16
			p    core.ProcInfo
		}
		var ports []portEntry
		for _, p := range s.Snapshot.Processes {
			for _, port := range p.Ports {
				if sub == "" || strings.Contains(strconv.Itoa(int(port)), sub) || strings.Contains(strings.ToLower(p.Name), sub) {
					ports = append(ports, portEntry{port, p})
				}
			}
		}
		slices.SortStableFunc(ports, func(a, b portEntry) int { return int(a.port) - int(b.port) })
		var out []statusCandidate
		for _, entry := range ports[:min(80, len(ports))] {
			out = append(out, statusCandidate{id: fmt.Sprintf("port-%d-%d", entry.port, entry.p.PID), query: fmt.Sprintf(":%d", entry.port), label: fmt.Sprintf(":%d  %s", entry.port, entry.p.Name), detail: fmt.Sprintf("#%d", entry.p.PID)})
		}
		return out
	}
	var out []statusCandidate
	if strings.TrimSpace(query) != "" {
		out = append(out, statusCandidate{id: "query", query: query, label: s.tr("searchQuery", query)})
		groups := make(map[string][]core.ProcInfo)
		for _, p := range filtered {
			key := strings.ToLower(p.Name)
			groups[key] = append(groups[key], p)
		}
		keys := make([]string, 0, len(groups))
		for k := range groups {
			keys = append(keys, k)
		}
		slices.Sort(keys)
		for _, key := range keys[:min(8, len(keys))] {
			g := groups[key]
			out = append(out, statusCandidate{id: "process-" + key, query: `"` + g[0].Name + `"`, label: g[0].Name, detail: s.tr("processCount", len(g))})
		}
	}
	for i, item := range [][3]string{{`msedge, "NVIDIA Overlay"`, "hintMulti", `msedge, "NVIDIA Overlay"`}, {"cpu >", "hintCPU", "cpu > "}, {"mem >", "hintMem", "mem > "}, {"user:", "hintUser", "user:"}, {":3000", "hintPort", ":"}, {"-helper", "hintExclude", "-helper"}} {
		if completion, ok := statusCompletion(item[2], query); ok {
			label := item[0]
			if strings.HasPrefix(label, "mem") || strings.HasPrefix(label, "cpu") {
				label = strings.TrimSpace(completion)
			}
			out = append(out, statusCandidate{id: fmt.Sprintf("syntax-%d", i), query: completion, label: label, detail: s.tr(item[1]), keepOpen: true})
		}
	}
	lower := strings.ToLower(strings.TrimSpace(query))
	if strings.HasPrefix(lower, "mem") || strings.HasPrefix(lower, "cpu") || strings.HasPrefix(lower, "user") {
		slices.SortStableFunc(out, func(a, b statusCandidate) int {
			if a.keepOpen == b.keepOpen {
				return 0
			}
			if a.keepOpen {
				return -1
			}
			return 1
		})
	}
	return out
}

func statusCandidateIndex(id string, items []statusCandidate) int {
	for i, item := range items {
		if item.id == id {
			return i
		}
	}
	if len(items) > 0 {
		return 0
	}
	return -1
}

func (s *StatusPage) renderSearch(c *ui.Context, filtered []core.ProcInfo, a StatusPageActions) {
	query := s.Query
	input := ui.TextInput(c.Key("status-search"), &query).Width(210).Height(28).FontSize(11.5).Placeholder(s.tr("search")).Label("status-search")
	if s.search.completed {
		end := utf8.RuneCountInString(query)
		input.SetTextSelection(end, end)
		s.search.completed = false
	}
	if input.Changed() {
		s.search.open = true
		s.search.selected = ""
		if a.SetQuery != nil {
			a.SetQuery(query)
		}
	}
	if input.Clicked() {
		s.search.open = true
	}
	if !input.Focused() {
		s.search.open = false
	}
	if s.search.previous != s.Query {
		s.search.selected = ""
		s.search.previous = s.Query
	}
	items := s.candidates(filtered, query)
	choose := func(item statusCandidate) {
		s.search.open = item.keepOpen
		s.search.selected = ""
		s.search.completed = true
		if a.SetQuery != nil {
			a.SetQuery(item.query)
		}
	}
	input.OnTextKey(func(k ui.TextKey) bool {
		if k.Mods != 0 {
			return false
		}
		current := s.candidates(s.filteredProcesses(), k.Text())
		switch k.Key {
		case ui.KeyDown, ui.KeyUp:
			s.search.open = true
			index := statusCandidateIndex(s.search.selected, current)
			if index < 0 {
				return true
			}
			step := 1
			if k.Key == ui.KeyUp {
				step = -1
			}
			index = (index + step + len(current)) % len(current)
			s.search.selected = current[index].id
			s.search.list.ScrollTo(index, ui.Center)
			return true
		case ui.KeyEnter:
			if !s.search.open {
				return false
			}
			index := statusCandidateIndex(s.search.selected, current)
			if index >= 0 {
				item := current[index]
				k.SetText(item.query)
				choose(item)
			}
			return true
		case ui.KeyEscape:
			if s.search.open {
				s.search.open = false
			} else {
				k.SetText("")
				if a.SetQuery != nil {
					a.SetQuery("")
				}
			}
			return true
		}
		return false
	})
	ui.PopoverBase(c.Key("status-search-popover"), input, &s.search.open, func(panel ui.Element) {
		panel.Width(320).MaxHeight(320).Padding(0).Gap(0).Radius(8).Border(1, outlineVariant.Alpha(.5)).Background(card).Shadow(0, 6, 16, 0, ui.RGBA(0, 0, 0, .15)).Children(func() {
			title := "searchHelp"
			if statusPortQuery(query) {
				title = "ports"
			}
			ui.Text(c, s.tr(title)).FillWidth().Padding(8, 12).FontSize(12).Bold().TextColor(muted)
			if len(items) == 0 {
				label := s.tr("noPorts")
				if _, sub, ok := strings.Cut(query, ":"); ok && strings.TrimSpace(sub) != "" {
					label = s.tr("noMatchingPorts", strings.TrimSpace(sub))
				}
				ui.Text(c, label).Padding(16, 8).FontSize(12).TextColor(outline)
			} else {
				selected := statusCandidateIndex(s.search.selected, items)
				s.search.list.Key = func(i int) any { return items[i].id }
				ui.List(c.Key("suggestions"), &s.search.list, len(items)).FillWidth().Height(float32(min(240, len(items)*32))).Rows(func(row ui.ListRow) {
					item := items[row.Index]
					b := ui.ButtonBase(row.Context.Key(item.id)).FillWidth().Height(32).Padding(4, 8).Gap(8).Justify(ui.Start)
					if row.Index == selected || b.Hovered() {
						b.Background(primaryFixed)
					}
					b.Children(func() {
						ui.Text(row.Context, item.label).Grow(1).MinWidth(0).Clip().FontSize(12).TextColor(text)
						if item.detail != "" {
							ui.Text(row.Context, item.detail).FontSize(12).TextColor(outline)
						}
					})
					if b.Clicked() {
						choose(item)
						input.Focus()
					}
				})
			}
			if statusPortQuery(query) {
				ui.Text(c, s.tr("portTip")).FillWidth().Padding(8, 12).FontSize(12).TextColor(outline)
			}
		})
	})
}
