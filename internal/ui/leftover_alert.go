package ui

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

// Pre-parsed vector SVGs for crisp, modern macOS aesthetics without emojis.
var (
	alertTrashSVG = ui.MustParseSVG([]byte(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"></polyline><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path><line x1="10" y1="11" x2="10" y2="17"></line><line x1="14" y1="11" x2="14" y2="17"></line></svg>`))
	alertCloseSVG = ui.MustParseSVG([]byte(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"></line><line x1="6" y1="6" x2="18" y2="18"></line></svg>`))
	alertBroomSVG = ui.MustParseSVG([]byte(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 2l-7 7m5-5l2 2m-7 7l-5 5a2.83 2.83 0 0 0 4 4l5-5m-9-9l7 7"></path></svg>`))
	alertLockSVG  = ui.MustParseSVG([]byte(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="11" width="18" height="11" rx="2" ry="2"></rect><path d="M7 11V7a5 5 0 0 1 10 0v4"></path></svg>`))
	alertInfoSVG  = ui.MustParseSVG([]byte(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"></circle><line x1="12" y1="16" x2="12" y2="12"></line><line x1="12" y1="8" x2="12.01" y2="8"></line></svg>`))
	alertRightSVG = ui.MustParseSVG([]byte(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"></polyline></svg>`))
)

// LeftoverAlert is the floating panel the uninstall watcher surfaces: its own
// window, not the tray flyout, modeled on clean, modern macOS system dialogs —
// an app header, the pickable leftovers grouped by semantics, and a solid,
// never-occluded bottom action bar.
type LeftoverAlert struct {
	Language  Language
	Leftovers []TrayLeftover
}

type LeftoverAlertActions struct {
	Review      func(id string)
	Dismiss     func()
	Clean       func(id string)
	Toggle      func(id string, item int)
	SelectGroup func(id string, confirm bool)
	SelectAll   func(id string)
	SelectSafe  func(id string)
}

type leftoverAlertCopy struct {
	title, scanning, review, close, summary, inTrash           string
	selected, selectAll, selectSafe, clean, cleanAll, cleaning string
	certain, confirm, manual, authTip                          string
	safeBadge, authBadge, skip, empty                          string
}

func leftoverAlertStrings(l Language) leftoverAlertCopy {
	if l == English {
		return leftoverAlertCopy{
			title: "Leftover Detection", scanning: "Scanning for leftovers…",
			review: "Details", close: "Close Window",
			summary: "%d leftover items · %s", inTrash: "In Trash",
			selected: "%d selected · %s", selectAll: "Select all", selectSafe: "Safe only",
			clean: "Clean Selected (%s)", cleanAll: "Clean All %s", cleaning: "Cleaning…",
			certain: "Configuration & Caches", confirm: "Privileged & Background Services", manual: "System Extension",
			safeBadge: "Safe to clean", authBadge: "Auth required",
			authTip: "Cleaning privileged items requires Touch ID or Admin password",
			skip:    "Skip", empty: "Nothing left to clean",
		}
	}
	return leftoverAlertCopy{
		title: "卸载残留检测", scanning: "正在查找残留…",
		review: "详情", close: "关闭窗口",
		summary: "发现 %d 处关联残留 · 共计 %s", inTrash: "已入废纸篓",
		selected: "已选 %d 项 · %s", selectAll: "全选", selectSafe: "仅选常规",
		clean: "清理已选项 (%s)", cleanAll: "一键彻底清理 %s", cleaning: "清理中…",
		certain: "常规配置与缓存", confirm: "特权与自启后台服务", manual: "系统扩展组件",
		safeBadge: "推荐清理", authBadge: "需系统授权",
		authTip: "清理特权服务项将在执行时触发一次 Touch ID / 密码确认",
		skip:    "稍后 / 跳过", empty: "没有可清理的项目",
	}
}

func (s LeftoverAlert) Render(c *ui.Context, a LeftoverAlertActions) {
	t := leftoverAlertStrings(s.Language)
	if c.Shortcut(0, ui.KeyEscape) && a.Dismiss != nil {
		a.Dismiss()
	}
	// Frameless, clean macOS floating card with subtle border and rounded corners.
	ui.Column(c.Key("leftover-alert")).Fill().MinHeight(0).Clip().Radius(14).Background(background).Border(1, outlineVariant.Alpha(.4)).TextColor(text).Children(func() {
		// Header draggable bar
		ui.Row(c.Key("leftover-alert-header")).FillWidth().Justify(ui.SpaceBetween).Padding(10, 14, 8, 14).DragWindow().BorderWidth(0, 0, 1, 0).BorderColor(outlineVariant.Alpha(.25)).Children(func() {
			ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
				ui.Icon(c, alertTrashSVG).Size(15, 15).TextColor(primary)
				ui.Text(c, t.title).FontSize(13).FixedLineHeight(18).FontWeight(600).TextColor(text)
			})
			b := ui.ButtonBase(c.Key("leftover-alert-close")).Size(22, 22).Padding(4).Radius(6).Cursor(ui.CursorPointer).Label(t.close).Tooltip(t.close).Disabled(a.Dismiss == nil)
			interactive(b, ui.Transparent, surfaceLow, surfaceHigh, a.Dismiss != nil)
			b.Children(func() {
				ui.Icon(c, alertCloseSVG).Size(11, 11).TextColor(muted)
			})
			if b.Clicked() && a.Dismiss != nil {
				a.Dismiss()
			}
		})

		// App content stream
		ui.Scroll(c.Key("leftover-alert-list")).FillWidth().Grow(1).MinHeight(0).Padding(10, 14, 12, 14).Gap(12).Children(func() {
			for i, l := range s.Leftovers {
				s.renderApp(c, a, t, i, l)
			}
		})
	})
}

// renderApp lays out one detected app with its footprint, semantic groupings,
// and a guaranteed unoccluded footer.
func (s LeftoverAlert) renderApp(c *ui.Context, a LeftoverAlertActions, t leftoverAlertCopy, i int, l TrayLeftover) {
	entry := l
	var certain, confirm []int
	count := 0
	var picked uint64
	hasPrivilegedSelected := false
	for j, it := range entry.Entries {
		if it.Manual {
			continue
		}
		if it.Confirm {
			confirm = append(confirm, j)
		} else {
			certain = append(certain, j)
		}
		if it.Selected {
			count++
			picked += it.Bytes
			if it.Confirm {
				hasPrivilegedSelected = true
			}
		}
	}

	ui.Column(c.Key(fmt.Sprintf("leftover-app-%d", i))).FillWidth().Gap(10).Children(func() {
		// App header: icon, name, status, and detail trigger
		ui.Row(c).FillWidth().Gap(12).AlignItems(ui.Center).Children(func() {
			if entry.Icon != nil {
				ui.Image(c, entry.Icon).Size(44, 44).Radius(10).Shrink(0)
			} else {
				ui.Box(c).Size(44, 44).Radius(10).Background(primaryFixed).Center().Children(func() {
					ui.Icon(c, alertTrashSVG).Size(20, 20).TextColor(primary)
				})
			}
			ui.Column(c).Grow(1).MinWidth(0).Gap(3).Children(func() {
				ui.Row(c).Gap(6).AlignItems(ui.Center).Children(func() {
					ui.Text(c, entry.Name).MinWidth(0).Shrink(1).FontSize(15).FixedLineHeight(19).Bold().SingleLine().Tooltip(entry.Name)
					ui.Text(c, t.inTrash).FontSize(10).FixedLineHeight(14).Padding(1, 5).Radius(4).Background(surfaceLow).TextColor(muted)
				})
				sub := fmt.Sprintf(t.summary, entry.Items, entry.Size)
				if entry.Scanning {
					sub = t.scanning
				}
				ui.Text(c, sub).FontSize(12).FixedLineHeight(15).TextColor(muted).SingleLine()
			})
			if !entry.Scanning {
				review := ui.ButtonBase(c.Key(fmt.Sprintf("leftover-review-%d", i))).Shrink(0).Padding(3, 8).Radius(6).Cursor(ui.CursorPointer).Disabled(a.Review == nil || entry.Cleaning)
				interactive(review, ui.Transparent, surfaceLow, surfaceHigh, a.Review != nil && !entry.Cleaning)
				review.Children(func() {
					ui.Row(c).Gap(3).AlignItems(ui.Center).Children(func() {
						ui.Text(c, t.review).FontSize(12).FixedLineHeight(15).FontWeight(500).TextColor(primary)
						ui.Icon(c, alertRightSVG).Size(10, 10).TextColor(primary)
					})
				})
				if review.Clicked() && a.Review != nil {
					a.Review(entry.ID)
				}
			}
		})

		if entry.Scanning || len(entry.Entries) == 0 {
			if !entry.Scanning {
				ui.Text(c, t.empty).FontSize(12).FixedLineHeight(16).TextColor(muted)
			}
			return
		}

		// Selection bar with unified "All" and "Safe only" controls
		ui.Row(c).FillWidth().Justify(ui.SpaceBetween).AlignItems(ui.Center).Padding(6, 8).Radius(8).Background(surfaceLow.Alpha(.6)).Children(func() {
			ui.Text(c, fmt.Sprintf(t.selected, count, core.FmtSize(picked))).FontSize(12).FixedLineHeight(15).Bold().TextColor(text)
			ui.Row(c).Gap(6).AlignItems(ui.Center).Children(func() {
				// Select All
				btnAll := ui.ButtonBase(c.Key(fmt.Sprintf("leftover-all-%d", i))).Padding(2, 6).Radius(4).Cursor(ui.CursorPointer).Disabled(entry.Cleaning)
				interactive(btnAll, ui.Transparent, surfaceHigh, surfaceHighest, !entry.Cleaning)
				btnAll.Children(func() {
					ui.Text(c, t.selectAll).FontSize(11).FixedLineHeight(14).FontWeight(600).TextColor(primary)
				})
				if btnAll.Clicked() {
					if a.SelectAll != nil {
						a.SelectAll(entry.ID)
					} else if a.SelectGroup != nil {
						a.SelectGroup(entry.ID, false)
						a.SelectGroup(entry.ID, true)
					}
				}

				ui.Text(c, "|").FontSize(10).TextColor(outlineVariant)

				// Select Safe only
				btnSafe := ui.ButtonBase(c.Key(fmt.Sprintf("leftover-safe-%d", i))).Padding(2, 6).Radius(4).Cursor(ui.CursorPointer).Disabled(entry.Cleaning)
				interactive(btnSafe, ui.Transparent, surfaceHigh, surfaceHighest, !entry.Cleaning)
				btnSafe.Children(func() {
					ui.Text(c, t.selectSafe).FontSize(11).FixedLineHeight(14).TextColor(muted)
				})
				if btnSafe.Clicked() {
					if a.SelectSafe != nil {
						a.SelectSafe(entry.ID)
					} else if a.SelectGroup != nil {
						a.SelectGroup(entry.ID, false)
					}
				}
			})
		})

		// Groups
		if len(certain) > 0 {
			s.renderGroup(c, a, t, i, entry, certain, t.certain, t.safeBadge, false)
		}
		if len(confirm) > 0 {
			s.renderGroup(c, a, t, i, entry, confirm, t.confirm, t.authBadge, true)
		}

		// Manual items (system extensions) show for awareness
		for j, it := range entry.Entries {
			if !it.Manual {
				continue
			}
			ui.Row(c.Key(fmt.Sprintf("leftover-manual-%d-%d", i, j))).FillWidth().Gap(8).Padding(4, 6).Radius(6).Background(surfaceLow).Children(func() {
				ui.Text(c, t.manual).Shrink(0).FontSize(10).FixedLineHeight(13).Padding(1, 4).Radius(4).TextColor(caution).Background(cautionContainer)
				ui.Text(c, it.Name).Grow(1).MinWidth(0).FontSize(12).FixedLineHeight(15).Bold().TextColor(muted).SingleLine().Tooltip(it.Label)
				ui.Text(c, it.Size).Shrink(0).FontSize(12).FixedLineHeight(15).TextColor(outline).SingleLine()
			})
		}

		// Privilege authorization tip
		if hasPrivilegedSelected {
			ui.Row(c).FillWidth().Gap(6).AlignItems(ui.Center).Padding(6, 8).Radius(6).Background(cautionContainer.Alpha(.4)).Border(1, caution.Alpha(.2)).Children(func() {
				ui.Icon(c, alertLockSVG).Size(12, 12).TextColor(caution)
				ui.Text(c, t.authTip).FontSize(11).FixedLineHeight(14).TextColor(caution).Grow(1)
			})
		}

		// Unoccluded Sticky Footer (Right-aligned macOS style)
		ui.Row(c).FillWidth().Justify(ui.End).AlignItems(ui.Center).Gap(8).Padding(8, 0, 0, 0).BorderWidth(1, 0, 0, 0).BorderColor(outlineVariant.Alpha(.3)).Children(func() {
			// Skip / Dismiss
			skip := ui.ButtonBase(c.Key(fmt.Sprintf("leftover-skip-%d", i))).Padding(7, 14).Radius(8).Background(surfaceLow).Cursor(ui.CursorPointer).Disabled(a.Dismiss == nil)
			interactive(skip, surfaceLow, surfaceHigh, outlineVariant.Alpha(.3), a.Dismiss != nil)
			skip.Children(func() {
				ui.Text(c, t.skip).FontSize(12).FixedLineHeight(16).FontWeight(500).TextColor(text)
			})
			if skip.Clicked() && a.Dismiss != nil {
				a.Dismiss()
			}

			// Clean action
			totalSelectable := len(certain) + len(confirm)
			label := fmt.Sprintf(t.clean, core.FmtSize(picked))
			if count == totalSelectable && count > 0 {
				label = fmt.Sprintf(t.cleanAll, core.FmtSize(picked))
			}
			if entry.Cleaning {
				label = t.cleaning
			}

			btn := ui.ButtonBase(c.Key(fmt.Sprintf("leftover-clean-%d", i))).Padding(7, 18).Radius(8).Cursor(ui.CursorPointer).Disabled(entry.Cleaning || count == 0 || a.Clean == nil)
			bg, fg := surfaceLow, muted
			if !entry.Cleaning && count > 0 && a.Clean != nil {
				bg, fg = primary, card
			}
			btn.Background(bg)
			interactive(btn, bg, primaryBright, primary.Alpha(.8), !entry.Cleaning && count > 0 && a.Clean != nil)
			btn.Children(func() {
				ui.Row(c).Gap(6).AlignItems(ui.Center).Children(func() {
					if hasPrivilegedSelected {
						ui.Icon(c, alertLockSVG).Size(13, 13).TextColor(fg)
					} else {
						ui.Icon(c, alertBroomSVG).Size(13, 13).TextColor(fg)
					}
					ui.Text(c, label).FontSize(13).FixedLineHeight(16).Bold().TextColor(fg)
				})
			})
			if btn.Clicked() && a.Clean != nil {
				a.Clean(entry.ID)
			}
		})
	})
}

// renderGroup lists one confidence section with clear badges and human-readable names
func (s LeftoverAlert) renderGroup(c *ui.Context, a LeftoverAlertActions, t leftoverAlertCopy, i int, entry TrayLeftover, indexes []int, heading, badge string, confirm bool) {
	ui.Column(c.Key(fmt.Sprintf("leftover-group-%d-%v", i, confirm))).FillWidth().Gap(4).Children(func() {
		// Group header row
		ui.Row(c).FillWidth().Justify(ui.SpaceBetween).AlignItems(ui.Center).Padding(2, 2).Children(func() {
			ui.Row(c).Gap(6).AlignItems(ui.Center).Children(func() {
				ui.Text(c, fmt.Sprintf("%s (%d)", heading, len(indexes))).FontSize(11).FixedLineHeight(14).Bold().TextColor(text.Alpha(.75))
				badgeBg, badgeFg := successContainer.Alpha(.6), success
				if confirm {
					badgeBg, badgeFg = cautionContainer.Alpha(.6), caution
				}
				ui.Text(c, badge).FontSize(10).FixedLineHeight(13).Padding(1, 5).Radius(4).Background(badgeBg).TextColor(badgeFg)
			})
		})

		// Items list
		for _, j := range indexes {
			index := j
			item := entry.Entries[j]
			ui.Row(c.Key(fmt.Sprintf("leftover-item-%d-%d", i, j))).FillWidth().Gap(8).Padding(5, 8).Radius(6).Background(card).Border(1, outlineVariant.Alpha(.25)).AlignItems(ui.Center).Children(func() {
				selected := item.Selected
				check := ui.Checkbox(c.Key(fmt.Sprintf("leftover-check-%d-%d", i, j)), &selected, "").Disabled(entry.Cleaning || a.Toggle == nil)
				if check.Changed() && a.Toggle != nil {
					a.Toggle(entry.ID, index)
				}
				ui.Column(c).Grow(1).MinWidth(0).Gap(1).Children(func() {
					displayName := item.Name
					if displayName == "" {
						displayName = item.Label
					}
					ui.Text(c, displayName).MinWidth(0).Shrink(1).FontSize(12).FixedLineHeight(15).FontWeight(600).SingleLine().Tooltip(item.Name)
					if item.Path != "" {
						ui.Text(c, item.Path).MinWidth(0).Shrink(1).FontSize(10).FixedLineHeight(13).TextColor(muted).SingleLine().Tooltip(item.Path)
					}
				})
				ui.Text(c, item.Size).Shrink(0).FontSize(12).FixedLineHeight(15).TextColor(outline).SingleLine()
			})
		}
	})
}
