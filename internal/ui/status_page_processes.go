package ui

import (
	"fmt"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

type statusProcessRow struct {
	process   core.ProcInfo
	group     string
	count     int
	child     bool
	childLast bool
}

func (s *StatusPage) processRows(filtered []core.ProcInfo) []statusProcessRow {
	groups := make(map[string][]core.ProcInfo)
	for _, p := range filtered {
		key := strings.ToLower(p.Name)
		groups[key] = append(groups[key], p)
	}
	keys := make([]string, 0, len(groups))
	for key := range groups {
		keys = append(keys, key)
	}
	slices.Sort(keys)
	var top []statusProcessRow
	for _, key := range keys {
		members := groups[key]
		p := members[0]
		if len(members) == 1 {
			top = append(top, statusProcessRow{process: p})
			continue
		}
		p.CPU, p.MemBytes, p.Threads, p.CPUTimeMS, p.Ports = 0, 0, 0, 0, nil
		for _, member := range members {
			p.CPU += member.CPU
			p.MemBytes += member.MemBytes
			p.Threads += member.Threads
			p.CPUTimeMS += member.CPUTimeMS
		}
		for _, member := range members {
			if member.ParentPID == nil {
				continue
			}
			if slices.ContainsFunc(members, func(other core.ProcInfo) bool { return other.PID == *member.ParentPID }) {
				p.PID = *member.ParentPID
				break
			}
		}
		top = append(top, statusProcessRow{process: p, group: key, count: len(members)})
	}
	slices.SortStableFunc(top, func(a, b statusProcessRow) int { return s.compareProcesses(a.process, b.process) })
	var rows []statusProcessRow
	for _, row := range top {
		rows = append(rows, row)
		if row.group != "" && s.ExpandedGroups[row.group] {
			children := groups[row.group]
			for i, p := range children {
				rows = append(rows, statusProcessRow{
					process:   p,
					child:     true,
					childLast: i == len(children)-1,
				})
			}
		}
	}
	return rows
}

var statusColumnWidths = []float32{0, 72, 80, 54, 74, 58, 82}
var statusColumnKeys = []StatusSortKey{StatusSortName, StatusSortCPU, StatusSortMemory, StatusSortThreads, StatusSortCPUTime, StatusSortPID, StatusSortUser}

func (s *StatusPage) renderProcesses(c *ui.Context, a StatusPageActions) {
	snap := s.Snapshot
	filtered := s.filteredProcesses()
	rows := s.cachedProcessRows(filtered)
	var threads uint64
	for _, p := range snap.Processes {
		threads += uint64(p.Threads)
	}
	ui.Column(c).Grow(1).MinHeight(0).FillWidth().Padding(20).Gap(12).Children(func() {
		ui.Column(c).Gap(4).Children(func() {
			ui.Text(c, s.tr("manager")).FontSize(16).FixedLineHeight(24).Bold().TextColor(text)
			ui.Text(c, s.tr("running", snap.ProcessCount)+" · "+s.tr("threadCount", threads)).FontSize(12).TextColor(muted)
		})
		ui.Column(c).FillWidth().Grow(1).MinHeight(0).Radius(12).Border(1, outlineVariant.Alpha(.5)).Clip().Gap(0).Background(card).Children(func() {
			ui.Row(c).FillWidth().Padding(10, 16).Gap(12).BorderWidth(0, 0, 1, 0).BorderColor(outlineVariant.Alpha(.3)).Children(func() {
				ui.Text(c, s.tr("processes")).FontSize(14).Bold().TextColor(text)
				summary := s.tr("running", snap.ProcessCount)
				if len(filtered) != snap.ProcessCount {
					summary = s.tr("matched", snap.ProcessCount, len(filtered))
				}
				ui.Text(c, summary).FontSize(12).TextColor(muted)
				ui.Spacer(c)
				s.renderSearch(c, filtered, a)
				ui.Row(c.Key("process-filters")).Gap(4).Children(func() {
					for i, key := range []string{"all", "apps", "myProcesses", "system"} {
						tab := core.ProcFilterTab(i)
						statusButton(c, "filter-"+key, s.tr(key), s.Filter == tab, a.SetFilter == nil, func() { a.SetFilter(tab) })
					}
				})
			})
			ui.Row(c).FillWidth().Grow(1).MinHeight(0).Gap(0).Children(func() {
				ui.Column(c).Grow(1).MinWidth(0).FillHeight().Gap(0).Children(func() {
					ui.Row(c).FillWidth().Padding(8, 16).Gap(0).Background(surfaceLow).Children(func() {
						for i, label := range []string{s.tr("name"), "CPU", s.tr("memory"), s.tr("threads"), s.tr("cpuTime"), "PID", s.tr("user")} {
							key := statusColumnKeys[i]
							b := ui.ButtonBase(c.Key(fmt.Sprintf("sort-%d", key))).Padding(0).Gap(3).MinWidth(0).Cursor(ui.CursorPointer)
							if i == 0 {
								b.Grow(1).Justify(ui.Start)
							} else {
								b.Width(statusColumnWidths[i]).Shrink(0).Justify(ui.End)
							}
							b.Children(func() {
								ui.Text(c, label).FontSize(12).FontWeight(500).TextColor(muted)
								if s.Sort.Key == key {
									arrow := "↓"
									if s.Sort.Ascending {
										arrow = "↑"
									}
									ui.Text(c, arrow).FontSize(10).TextColor(primary)
								}
							})
							if b.Clicked() && a.SetSort != nil {
								a.SetSort(s.Sort.Toggled(key))
							}
						}
					})
					s.list.KeepOffset = true
					s.list.Key = func(i int) any {
						if rows[i].group != "" {
							return "group-" + rows[i].group
						}
						return fmt.Sprintf("proc-%d", rows[i].process.PID)
					}
					ui.List(c.Key("process-list"), &s.list, len(rows)).Grow(1).MinHeight(0).FillWidth().Rows(func(row ui.ListRow) { s.renderProcessRow(row.Context, rows[row.Index], a) })
					ui.Row(c).FillWidth().Padding(6, 16).Background(surfaceLow).Justify(ui.SpaceBetween).Children(func() {
						ui.Text(c, s.tr("showing", len(filtered), snap.ProcessCount)).FontSize(12).TextColor(muted)
						var cpu float32
						var mem uint64
						for _, p := range filtered {
							cpu += p.CPU
							mem += p.MemBytes
						}
						ui.Text(c, fmt.Sprintf("CPU: %.1f%%    %s: %s", cpu, s.tr("memory"), core.FmtMem(mem))).FontSize(12).TextColor(muted)
					})
				})
				if s.SelectedPID != nil {
					for _, p := range snap.Processes {
						if p.PID == *s.SelectedPID {
							s.renderProcessDetail(c, p, a)
							break
						}
					}
				}
			})
		})
	})
}

func (s *StatusPage) processIcon(c *ui.Context, p core.ProcInfo, size float32) {
	if p.IconSource != nil {
		if bitmap := s.Icons[*p.IconSource]; bitmap != nil {
			ui.Image(c, bitmap).Size(size, size).Shrink(0).Radius(3)
			return
		}
	}
	runes := []rune(p.Name)
	label := strings.ToUpper(string(runes[:min(2, len(runes))]))
	ui.Box(c).Size(size, size).Shrink(0).Radius(3).Background(surfaceLow).Border(1, outlineVariant.Alpha(.5)).Center().Children(func() { ui.Text(c, label).FontSize(12).Bold().TextColor(muted) })
}

func processTreeBranch(c *ui.Context, isLast bool, color ui.Color) ui.Element {
	return ui.Box(c).Width(24).Height(32).Shrink(0).PassThrough().Draw(func(p *ui.Painter, r ui.Rect) {
		x := r.X + 8
		midY := r.Y + r.H/2
		rightX := r.X + r.W
		stroke := float32(1.5)

		var path ui.Path
		if isLast {
			path.MoveTo(x, r.Y).LineTo(x, midY-3).QuadTo(x, midY, x+3, midY).LineTo(rightX, midY)
		} else {
			path.MoveTo(x, r.Y).LineTo(x, r.Y+r.H)
			path.MoveTo(x, midY).LineTo(rightX, midY)
		}
		p.StrokePath(&path, stroke, color)
	})
}

func (s *StatusPage) renderProcessRow(c *ui.Context, row statusProcessRow, a StatusPageActions) {
	p := row.process
	b := ui.ButtonBase(c).Height(32).FillWidth().Padding(0, 16).Gap(0).Justify(ui.Start).BorderWidth(0, 0, 1, 0).BorderColor(outlineVariant.Alpha(.35)).Cursor(ui.CursorPointer)
	if row.group == "" && s.SelectedPID != nil && *s.SelectedPID == p.PID {
		interactiveRow(b, primary.Alpha(.15), primary.Alpha(.15))
	} else {
		interactiveRow(b, ui.Transparent, surfaceLow)
	}
	b.Children(func() {
		ui.Row(c).Grow(1).MinWidth(0).Clip().Gap(6).Children(func() {
			switch {
			case row.group != "":
				ui.Box(c).Width(16).Shrink(0).Center().Children(func() { chevron(c, s.ExpandedGroups[row.group], outline, 12) })
			case row.child:
				processTreeBranch(c, row.childLast, outlineVariant)
			default:
				ui.Box(c).Width(16).Shrink(0)
			}
			iconSize := float32(18)
			nameWeight := 500
			nameColor := text
			if row.child {
				iconSize = 16
				nameWeight = 400
				nameColor = muted
			}
			s.processIcon(c, p, iconSize)
			ui.Text(c, p.Name).Grow(1).MinWidth(0).Clip().FontSize(12).FontWeight(nameWeight).TextColor(nameColor)
			if row.group != "" {
				ui.Text(c, fmt.Sprint(row.count)).Shrink(0).Padding(0, 6).Radius(6).Background(outlineVariant.Alpha(.5)).FontSize(12).TextColor(muted)
			}
			if len(p.Ports) > 0 {
				ui.Row(c).Shrink(0).Gap(3).Label(fmt.Sprintf("status-ports-%d", p.PID)).Children(func() {
					for _, port := range p.Ports[:min(2, len(p.Ports))] {
						ui.Text(c, fmt.Sprintf(":%d", port)).FontSize(10).TextColor(primary).Padding(1, 4).Radius(4).Background(primaryFixed)
					}
					if len(p.Ports) > 2 {
						ui.Text(c, fmt.Sprintf("+%d", len(p.Ports)-2)).FontSize(10).TextColor(outline)
					}
				})
			}
		})
		fg := muted
		if p.CPU > 50 {
			fg = statusError
		} else if p.CPU > 15 {
			fg = caution
		}
		ui.Row(c).Width(72).Shrink(0).Justify(ui.End).Gap(6).Label(fmt.Sprintf("status-cpu-%d", p.PID)).Children(func() {
			ui.Box(c).Size(24, 3).Radius(999).Background(surfaceHighest).Children(func() { ui.Box(c).WidthPercent(min(float32(100), max(float32(0), p.CPU))).Height(3).Background(fg) })
			ui.Text(c, fmt.Sprintf("%.1f%%", p.CPU)).Width(38).FontSize(12).TextColor(fg).TextAlign(ui.End)
		})
		pid := fmt.Sprint(p.PID)
		if row.group != "" {
			pid = "-"
		}
		for i, value := range []string{core.FmtMem(p.MemBytes), fmt.Sprint(p.Threads), core.FmtCPUTime(p.CPUTimeMS), pid, statusString(p.User)} {
			ui.Text(c, value).Width(statusColumnWidths[i+2]).Shrink(0).Clip().FontSize(12).FixedLineHeight(16).TextAlign(ui.End).TextColor(outline)
		}
	})
	if b.Clicked() {
		if row.group != "" {
			if a.ToggleGroup != nil {
				a.ToggleGroup(row.group)
			}
		} else if a.SelectProcess != nil {
			a.SelectProcess(p.PID)
		}
	}
}

func (s *StatusPage) renderProcessDetail(c *ui.Context, p core.ProcInfo, a StatusPageActions) {
	ui.Scroll(c.Key("process-detail")).Width(330).Shrink(0).FillHeight().Padding(16).Gap(12).BorderWidth(0, 1, 0, 0).BorderColor(outlineVariant.Alpha(.4)).Children(func() {
		ui.Row(c).FillWidth().Gap(10).Children(func() {
			s.processIcon(c, p, 36)
			ui.Column(c).Grow(1).MinWidth(0).Gap(0).Children(func() {
				ui.Text(c, p.Name).FontSize(14).Bold().Clip()
				ui.Text(c, fmt.Sprintf("PID %d · %s", p.PID, statusString(p.User))).FontSize(12).TextColor(muted)
			})
			statusButton(c, "close-detail", "×", false, a.CloseProcess == nil, a.CloseProcess)
		})
		ui.Row(c).Gap(8).Children(func() {
			statusButton(c, "quit", s.tr("quit"), false, a.RequestQuit == nil, func() { a.RequestQuit(p) })
			statusButton(c, "force-quit", s.tr("forceQuit"), false, a.ForceQuit == nil, func() { a.ForceQuit(p) })
			if p.ExePath != nil {
				statusButton(c, "reveal", s.tr("reveal"), false, a.RevealProcess == nil, func() { a.RevealProcess(*p.ExePath) })
			}
		})
		for i, label := range []string{"CPU", s.tr("memory")} {
			fg := primary
			bg := primary.Alpha(.08)
			value := fmt.Sprintf("%.1f%%", p.CPU)
			hist := s.SelectedCPUHistory
			if i == 1 {
				fg = ui.Hex("#006e1c")
				bg = fg.Alpha(.08)
				value = core.FmtMem(p.MemBytes)
				hist = nil
				maximum := uint64(1)
				for _, v := range s.SelectedMemHistory {
					maximum = max(maximum, v)
				}
				for _, v := range s.SelectedMemHistory {
					hist = append(hist, float32(v)/float32(maximum)*100)
				}
			}
			ui.Column(c).FillWidth().Padding(10).Radius(8).Border(1, fg.Alpha(.2)).Background(bg).Gap(0).Children(func() {
				ui.Row(c).FillWidth().Justify(ui.SpaceBetween).Children(func() { ui.Text(c, label).FontSize(12).Bold().TextColor(fg); ui.Text(c, value).FontSize(12).Bold() })
				statusHistory(c, hist, 24)
			})
		}
		ui.Text(c, s.tr("details")).FontSize(12).Bold().TextColor(muted)
		state := "stateNormal"
		switch p.StatusText {
		case "Run", "Running":
			state = "stateRunning"
		case "Sleep", "Sleeping":
			state = "stateSleeping"
		case "Idle":
			state = "stateIdle"
		case "Stop", "Stopped":
			state = "stateStopped"
		case "Zombie":
			state = "stateZombie"
		}
		for _, kv := range [][2]string{{"cpuTime", core.FmtCPUTime(p.CPUTimeMS)}, {"threads", fmt.Sprint(p.Threads)}, {"rss", core.FmtMem(p.MemBytes)}, {"vms", core.FmtMem(p.VirtualMemBytes)}, {"read", core.FmtMem(p.DiskReadBytes) + " · " + core.FmtSize(uint64(max(0, p.DiskReadBPS))) + "/s"}, {"write", core.FmtMem(p.DiskWrittenBytes) + " · " + core.FmtSize(uint64(max(0, p.DiskWrittenBPS))) + "/s"}, {"started", s.startTime(p.StartTime, c.Now())}, {"state", s.tr(state)}} {
			ui.Row(c).FillWidth().Justify(ui.SpaceBetween).Gap(8).Children(func() {
				ui.Text(c, s.tr(kv[0])).FontSize(12).TextColor(outline)
				ui.Text(c, kv[1]).MinWidth(0).FontSize(12).TextColor(text)
			})
		}
		if p.ParentPID != nil {
			ui.Row(c).FillWidth().Justify(ui.SpaceBetween).Children(func() {
				ui.Text(c, s.tr("parent")).FontSize(12).TextColor(outline)
				name := "process"
				if p.ParentName != nil {
					name = *p.ParentName
				}
				statusButton(c, "parent", fmt.Sprintf("%s %d", name, *p.ParentPID), false, a.SelectProcess == nil, func() { a.SelectProcess(*p.ParentPID) })
			})
		}
		if p.CmdLine != nil {
			ui.Row(c).FillWidth().Justify(ui.SpaceBetween).Children(func() {
				ui.Text(c, s.tr("command")).FontSize(12).Bold().TextColor(muted)
				label := "copy"
				if s.CommandCopied {
					label = "copied"
				}
				statusButton(c, "copy", s.tr(label), false, a.CopyCommand == nil, func() { a.CopyCommand(*p.CmdLine) })
			})
			ui.Text(c, *p.CmdLine).FillWidth().Padding(8).Radius(6).Background(surfaceLow).FontSize(12).TextColor(text)
		}
	})
}
