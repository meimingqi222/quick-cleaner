package ui

import (
	"fmt"
	"math"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

// TrayPopover is presentation only. The host replaces Snapshot on its UI thread
// when the overview controller publishes changes. MyGo retains keyed scroll state.
type TrayPopover struct {
	Language Language
	Snapshot TrayOverviewSnapshot
}

// Bind these callbacks to the current host TraySession's emit/dismiss operations.
// Emit uses OverviewAction's dismissal policy; content must not implement it.
// No model or host handle
// crosses this boundary, and rendering never starts or stops a controller.
type TrayPopoverActions struct {
	Emit    func(OverviewAction)
	Dismiss func()
}

func (a TrayPopoverActions) emit(action OverviewAction) {
	if a.Emit != nil {
		a.Emit(action)
	}
}

func (s TrayPopover) Render(c *ui.Context, a TrayPopoverActions) {
	t := trayPopoverStrings(s.Language)
	ui.Column(c.Key("tray-popover")).Fill().MinHeight(0).Gap(0).Clip().Background(background).Border(1, outlineVariant.Alpha(.6)).TextColor(text).Children(func() {
		ui.Row(c.Key("tray-header")).FillWidth().Shrink(0).Padding(8, 12).Justify(ui.SpaceBetween).BorderWidth(0, 0, 1, 0).BorderColor(outlineVariant.Alpha(.3)).Children(func() {
			ui.Row(c).Gap(8).Children(func() {
				icon(c, Disk, primary, 16)
				ui.Text(c, t.title).FontSize(14).FixedLineHeight(20).FontWeight(600).TextColor(primary)
			})
			b := ui.ButtonBase(c.Key("tray-close")).Size(22, 22).Padding(4).Radius(6).Cursor(ui.CursorPointer).Label(t.close).Tooltip(t.close).Disabled(a.Dismiss == nil)
			interactive(b, ui.Transparent, muted.Alpha(.15), muted.Alpha(.25), a.Dismiss != nil)
			b.Children(func() {
				ui.Text(c, "×").FontSize(14).FixedLineHeight(14).Bold().TextColor(muted)
			})
			if b.Clicked() && a.Dismiss != nil {
				a.Dismiss()
			}
		})
		// Disks take their own height and hardware follows them; the slack
		// goes above the quick actions, not between the two sections.
		ui.Scroll(c.Key("tray-content-viewport")).Shrink(1).FillWidth().MinHeight(0).Padding(12, 12, 8, 12).Gap(8).Children(func() {
			s.renderTrayLeftovers(c, a, t)
			s.renderTrayDisks(c, a, t)
		})
		ui.Column(c.Key("tray-hardware")).FillWidth().Shrink(0).Padding(6, 12).Gap(4).Children(func() {
			trayPopoverHeading(c, t.hardware)
			s.renderTrayHardware(c, a, t)
		})
		ui.Spacer(c)
		s.renderTrayActions(c, a, t)
	})
}

func trayPopoverHeading(c *ui.Context, label string) {
	ui.Text(c, label).FontSize(12).FixedLineHeight(16).Bold().TextColor(text.Alpha(.7))
}

// renderTrayLeftovers lists apps the trash watcher caught being uninstalled.
// Tapping opens the standard residual review in the main window; the row
// carries no cleanup of its own because deletion needs the review's gates.
func (s TrayPopover) renderTrayLeftovers(c *ui.Context, a TrayPopoverActions, t trayPopoverCopy) {
	if len(s.Snapshot.Leftovers) == 0 {
		return
	}
	ui.Column(c).FillWidth().Gap(4).Children(func() {
		trayPopoverHeading(c, t.leftovers)
		for i, l := range s.Snapshot.Leftovers {
			entry := l
			b := ui.ButtonBase(c.Key(fmt.Sprintf("tray-leftover-%d", i))).FillWidth().Padding(8).Radius(6).Background(card).Border(1, outlineVariant.Alpha(.35)).Row().Gap(8).Cursor(ui.CursorPointer).Disabled(a.Emit == nil)
			interactive(b, card, surfaceLow, surfaceHigh, a.Emit != nil)
			b.Children(func() {
				ui.Column(c).Grow(1).MinWidth(0).Gap(2).Children(func() {
					ui.Text(c, entry.Name).MinWidth(0).Shrink(1).FontSize(12).FixedLineHeight(16).Bold().SingleLine().Tooltip(entry.Name)
					ui.Text(c, fmt.Sprintf("%d · %s", entry.Items, entry.Size)).FontSize(11).FixedLineHeight(14).TextColor(muted).SingleLine()
				})
				ui.Text(c, t.review+" ›").Shrink(0).FontSize(12).FixedLineHeight(16).FontWeight(500).TextColor(primary)
			})
			if b.Clicked() {
				a.emit(OverviewAction{Kind: OverviewReviewLeftovers, Path: entry.ID})
			}
		}
	})
}

func (s TrayPopover) renderTrayDisks(c *ui.Context, a TrayPopoverActions, t trayPopoverCopy) {
	volumes := s.Snapshot.Volumes
	ui.Column(c).FillWidth().Gap(4).Children(func() {
		ui.Row(c).FillWidth().Justify(ui.SpaceBetween).Children(func() {
			trayPopoverHeading(c, t.disks)
			if len(volumes) >= 4 {
				ui.Text(c, fmt.Sprintf(t.volumes, len(volumes))).FontSize(12).FixedLineHeight(16).TextColor(muted)
			}
		})
		ui.Column(c.Key("tray-disk-list")).FillWidth().Gap(6).Children(func() {
			if len(volumes) == 0 {
				ui.Text(c, t.loading).FontSize(12).FixedLineHeight(16).TextColor(muted)
			} else if len(volumes) <= 3 {
				for i, v := range volumes {
					s.renderTrayDisk(c, a, t, i, v, false)
				}
			} else {
				for i := 0; i < len(volumes); i += 2 {
					ui.Row(c).FillWidth().Gap(6).Children(func() {
						for j := i; j < min(i+2, len(volumes)); j++ {
							s.renderTrayDisk(c, a, t, j, volumes[j], true)
						}
						if i+1 == len(volumes) {
							ui.Box(c).Grow(1).MinWidth(0)
						}
					})
				}
			}
		})
	})
}

func trayPopoverUsage(v TrayVolumeInfo) (uint64, float64, ui.Color) {
	used := v.TotalBytes - min(v.TotalBytes, v.FreeBytes)
	pct := float64(0)
	if v.TotalBytes > 0 {
		pct = float64(used) / float64(v.TotalBytes)
	}
	fg := primary
	if pct > .90 {
		fg = ui.Hex("#ba1a1a")
	} else if pct > .75 {
		fg = ui.Hex("#d97706")
	}
	return used, pct, fg
}

func (s TrayPopover) renderTrayDisk(c *ui.Context, a TrayPopoverActions, t trayPopoverCopy, i int, v TrayVolumeInfo, compact bool) {
	used, pct, fg := trayPopoverUsage(v)
	b := ui.ButtonBase(c.Key(fmt.Sprintf("tray-volume-%d", i))).MinWidth(0).Padding(8).Radius(6).Background(card).Border(1, outlineVariant.Alpha(.35)).Column().Gap(4).Cursor(ui.CursorPointer).Disabled(a.Emit == nil)
	if compact {
		b.Grow(1)
	} else {
		b.FillWidth()
	}
	interactive(b, card, surfaceLow, surfaceHigh, a.Emit != nil)
	if b.Hovered() {
		b.BorderColor(primary)
	}
	b.Children(func() {
		// Only the label may truncate: sizes and the percentage are what the
		// card is for, so they get lines of their own.
		ui.Row(c).FillWidth().Gap(6).Children(func() {
			ui.Text(c, v.Label).MinWidth(0).Shrink(1).FontSize(12).FixedLineHeight(16).Bold().TextColor(primary).Background(primary.Alpha(.12)).Radius(3).Padding(2, 6).SingleLine().Tooltip(v.Label)
			if compact {
				ui.Text(c, core.FmtSize(v.FreeBytes)).Shrink(0).FontSize(12).FixedLineHeight(16).TextColor(muted).SingleLine()
			}
			ui.Spacer(c)
			n := ui.Text(c, fmt.Sprintf("%.0f%%", math.Round(pct*100))).Shrink(0).FontSize(12).FixedLineHeight(16).TextColor(muted).SingleLine()
			if pct > .90 {
				n.TextColor(fg).Bold()
			}
		})
		ui.Row(c).FillWidth().Gap(8).Children(func() {
			h := float32(5)
			if compact {
				h = 4
			}
			ui.Box(c).Grow(1).MinWidth(0).Height(h).Radius(999).Background(surfaceHighest).Clip().Children(func() {
				ui.Box(c).WidthPercent(float32(pct) * 100).FillHeight().Radius(999).Background(fg)
			})
			if !compact {
				ui.Text(c, t.analyze).FontSize(12).FixedLineHeight(16).FontWeight(500).TextColor(primary)
			}
			ui.Text(c, "›").FontSize(10).FixedLineHeight(12).TextColor(primary)
		})
		if !compact {
			usage := fmt.Sprintf("%s / %s · %s", core.FmtSize(used), core.FmtSize(v.TotalBytes), fmt.Sprintf(t.free, core.FmtSize(v.FreeBytes)))
			ui.Text(c, usage).FontSize(12).FixedLineHeight(16).TextColor(muted).SingleLine()
		}
	})
	if b.Clicked() {
		a.emit(OverviewAction{Kind: OverviewAnalyzeDisk, Path: v.Label})
	}
}

func (s TrayPopover) renderTrayHardware(c *ui.Context, a TrayPopoverActions, t trayPopoverCopy) {
	state := s.Snapshot
	memPct := float64(0)
	if state.MemoryTotal > 0 {
		memPct = math.Round(float64(state.MemoryUsed) / float64(state.MemoryTotal) * 100)
	}
	success := ui.Hex("#059669")
	ui.Row(c).FillWidth().Gap(8).Children(func() {
		m := ui.ButtonBase(c.Key("tray-memory")).Grow(1).MinWidth(0).Clip().Padding(8).Radius(8).Border(1, outlineVariant.Alpha(.35)).Column().Gap(6).Cursor(ui.CursorPointer).Disabled(a.Emit == nil)
		interactive(m, card, surfaceLow, surfaceHigh, a.Emit != nil)
		if m.Hovered() {
			m.BorderColor(primary)
		}
		m.Children(func() {
			ui.Row(c).Gap(6).Children(func() {
				trayPopoverRAMIcon(c)
				ui.Text(c, fmt.Sprintf(t.memory, uint64(memPct))).FontSize(12).FixedLineHeight(16).Bold()
			})
			ui.Text(c, core.FmtMem(state.MemoryUsed)+" / "+core.FmtMem(state.MemoryTotal)).FontSize(12).FixedLineHeight(16).TextColor(muted).SingleLine()
			trayPopoverCardLink(c, t.topMemory)
		})
		if m.Clicked() {
			a.emit(OverviewAction{Kind: OverviewTopMemory})
		}
		b := ui.ButtonBase(c.Key("tray-cpu")).Grow(1).MinWidth(0).Clip().Padding(8).Radius(8).Border(1, outlineVariant.Alpha(.35)).Column().Gap(6).Cursor(ui.CursorPointer).Disabled(a.Emit == nil)
		interactive(b, card, surfaceLow, surfaceHigh, a.Emit != nil)
		if b.Hovered() {
			b.BorderColor(primary)
		}
		b.Children(func() {
			ui.Row(c).Gap(6).Children(func() {
				icon(c, Status, success, 15)
				ui.Text(c, fmt.Sprintf("CPU %d%%", state.CPUPct)).FontSize(12).FixedLineHeight(16).Bold()
			})
			label, color := t.unavailable, muted
			if state.TempC != nil {
				label = fmt.Sprintf("%.0f°C", *state.TempC)
				if *state.TempC > 80 {
					color = ui.Hex("#d97706")
				}
			}
			ui.Text(c, label).FontSize(12).FixedLineHeight(16).TextColor(color).SingleLine()
			trayPopoverCardLink(c, t.processes)
		})
		if b.Clicked() {
			a.emit(OverviewAction{Kind: OverviewOpenView, View: Status})
		}
	})
}

func trayPopoverRAMIcon(c *ui.Context) {
	ui.Row(c).Size(15, 15).Shrink(0).Center().Children(func() {
		ui.Row(c).Size(13.5, 7.5).Border(2, primary).Radius(2).Padding(0, 2).Gap(2).Children(func() {
			for i := 0; i < 3; i++ {
				ui.Box(c).Grow(1).Height(7.5 * .35).Radius(1).Background(primary)
			}
		})
	})
}

func trayPopoverCardLink(c *ui.Context, label string) {
	ui.Row(c).FillWidth().Height(24).Radius(6).Background(surfaceHighest.Alpha(.5)).Center().Gap(4).Children(func() {
		ui.Text(c, label).FontSize(12).FixedLineHeight(16).TextColor(muted)
		ui.Text(c, "›").FontSize(10).FixedLineHeight(12).TextColor(muted)
	})
}

func (s TrayPopover) renderTrayActions(c *ui.Context, a TrayPopoverActions, t trayPopoverCopy) {
	nav := dashboardStrings(s.Language).nav
	ui.Column(c.Key("tray-actions")).FillWidth().Shrink(0).Padding(8, 12).Gap(8).BorderWidth(1, 0, 0, 0).BorderColor(outlineVariant.Alpha(.3)).Children(func() {
		ui.Column(c).FillWidth().Gap(4).Children(func() {
			trayPopoverHeading(c, t.tools)
			ui.Row(c).FillWidth().Gap(6).Children(func() {
				for _, item := range []struct {
					key    string
					view   View
					action OverviewAction
				}{
					{"tray-scan", Junk, OverviewAction{Kind: OverviewScanJunk}},
					{"tray-apps", Apps, OverviewAction{Kind: OverviewOpenView, View: Apps}},
					{"tray-search", Search, OverviewAction{Kind: OverviewOpenView, View: Search}},
					{"tray-declutter", Declutter, OverviewAction{Kind: OverviewOpenView, View: Declutter}},
				} {
					b := ui.ButtonBase(c.Key(item.key)).Grow(1).MinWidth(0).Padding(6, 4).Radius(6).Border(1, outlineVariant.Alpha(.4)).Column().Center().Gap(4).Cursor(ui.CursorPointer).Disabled(a.Emit == nil)
					interactive(b, card, surfaceLow, surfaceHigh, a.Emit != nil)
					if b.Hovered() {
						b.BorderColor(primary)
					}
					b.Children(func() {
						icon(c, item.view, primary, 15)
						ui.Text(c, nav[item.view]).FontSize(12).FixedLineHeight(16).FontWeight(500)
					})
					if b.Clicked() {
						a.emit(item.action)
					}
				}
			})
		})
		b := ui.ButtonBase(c.Key("tray-main")).FillWidth().Height(30).Radius(6).Center().Cursor(ui.CursorPointer).Disabled(a.Emit == nil)
		interactive(b, primary, primaryBright, primary, a.Emit != nil)
		b.Children(func() { ui.Text(c, t.main).FontSize(12).FixedLineHeight(16).FontWeight(600).TextColor(card) })
		if b.Clicked() {
			a.emit(OverviewAction{Kind: OverviewOpenMain})
		}
		ui.Row(c).FillWidth().Center().Children(func() {
			q := ui.ButtonBase(c.Key("tray-quit")).Padding(0).Cursor(ui.CursorPointer).Disabled(a.Emit == nil)
			fg := muted
			if q.Hovered() {
				fg = ui.Hex("#ba1a1a")
			}
			q.Children(func() { ui.Text(c, t.quit).FontSize(12).FixedLineHeight(16).TextColor(fg) })
			if q.Clicked() {
				a.emit(OverviewAction{Kind: OverviewQuit})
			}
		})
	})
}
