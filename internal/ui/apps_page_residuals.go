package ui

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func (s AppsPage) residualReview(c *ui.Context, a AppsPageActions, t appsPageCopy) {
	open := true
	width, _ := c.Size()
	modal := ui.Modal(c.Key("apps-residual-review"), &open, func() {
		ui.Column(c).Width(712).MaxWidth(width - 80).MaxHeight(530).MinHeight(0).Gap(16).Children(func() {
			title, description := s.ResidualTitle.Get(s.Language), s.ResidualDescription.Get(s.Language)
			if title == "" {
				title = fmt.Sprintf(t.residualTitle, len(s.Residuals), s.ResidualAppName)
				if s.OrphanLeftovers {
					title = fmt.Sprintf(t.orphanTitle, len(s.Residuals))
				}
			}
			if description == "" {
				description = fmt.Sprintf(t.residualSub, s.ResidualSize)
				if s.OrphanLeftovers {
					description = fmt.Sprintf(t.orphanSub, s.ResidualSize)
				}
			}
			ui.Text(c, title).FontSize(18).Bold()
			ui.Text(c, description).FontSize(12).TextColor(muted)
			if notice := s.Notice.Get(s.Language); notice != "" {
				ui.Text(c, notice).FontSize(12).Padding(12).Background(cautionContainer).TextColor(caution).Radius(8)
			}
			s.occupancyBanner(c, t)
			for _, item := range s.Residuals {
				if item.Kind == core.ResidualSystemExtension {
					ui.Column(c).FillWidth().Padding(12).Gap(8).Background(cautionContainer).Radius(8).Children(func() {
						ui.Text(c, t.extensionTitle).FontSize(14).TextColor(caution)
						ui.Text(c, t.extensionAdvice).FontSize(12).TextColor(caution)
						appsPageButton(c, "apps-extension-settings", t.extensionOpen, a.OpenSystemExtensionSettings == nil, false, a.OpenSystemExtensionSettings)
					})
					break
				}
			}
			ui.Scroll(c.Key("apps-residual-list")).FillWidth().MaxHeight(280).MinHeight(50).Shrink(1).Gap(4).Padding(8).Radius(12).Border(1, outlineVariant.Alpha(.5)).Children(func() {
				if len(s.Residuals) == 0 {
					ui.Text(c, t.residualEmpty).FontSize(14).TextColor(muted).Padding(20)
				}
				for _, item := range s.Residuals {
					ui.Row(c.Key("apps-residual-"+item.ID)).FillWidth().Padding(8, 12).Gap(8).Children(func() {
						selected := item.Selected
						check := ui.Checkbox(c.Key("apps-residual-check-"+item.ID), &selected, "").Label(item.Path).Disabled(s.busy() || item.Manual || a.SetResidualSelected == nil)
						if check.Changed() && a.SetResidualSelected != nil {
							a.SetResidualSelected(item.ID, selected)
						}
						fg, bg := caution, cautionContainer
						if item.Confidence == core.Certain {
							fg, bg = primary, primaryFixed
						}
						ui.Text(c, item.Confidence.LabelLang(s.Language)).Shrink(0).FontSize(12).Padding(1, 8).Radius(6).TextColor(fg).Background(bg)
						ui.Text(c, item.Source.LabelLang(s.Language)).Shrink(0).FontSize(12).TextColor(outline)
						ui.Text(c, (core.ResidualKind{Type: item.Kind}).KindLabelLang(s.Language)).Shrink(0).FontSize(12).TextColor(outline)
						ui.Text(c, item.Path).Grow(1).MinWidth(0).FontSize(12).TextColor(text)
						ui.Text(c, item.Size).Shrink(0).FontSize(12).TextColor(outline)
					})
				}
			})
			// 常驻说明不是警告：它每个面板都有、内容固定，用橙色 caution 画出来
			// 就变成一条要用户处理的提示。放在列表下方用灰字小字，真正的失败
			// 提示（s.Notice）和占用横幅仍然保持警示样式。
			if notice := s.ResidualNotice.Get(s.Language); notice != "" && len(s.Residuals) > 0 {
				ui.Text(c, notice).FontSize(11).TextColor(outline)
			}
			s.RenderActionBar(c, a)
		})
	})
	if modal.Dismissed() && a.CloseResiduals != nil {
		a.CloseResiduals()
	}
}

// occupancyBanner restates scan-time process and launchd evidence. It does not
// authorize or block deletion; live occupancy is rechecked by the cleaner.
func (s AppsPage) occupancyBanner(c *ui.Context, t appsPageCopy) {
	occ := s.Occupancy
	if !occ.IsOccupied() {
		return
	}
	title, advice := t.registeredTitle, t.registeredAdvice
	if len(occ.Processes) > 0 {
		title, advice = t.occupiedTitle, t.occupiedAdvice
	}
	var lines []string
	for _, name := range occ.Processes {
		if len(lines) == 3 {
			break
		}
		lines = append(lines, "· "+truncateRunes(name, 78))
	}
	if len(occ.LaunchdLabels) > 0 {
		var labels []string
		for _, label := range occ.LaunchdLabels {
			if len(labels) == 3 {
				break
			}
			labels = append(labels, label)
		}
		sep := ": "
		if s.Language != English {
			sep = "："
		}
		lines = append(lines, "· "+t.launchdGroup+sep+strings.Join(labels, t.listSep))
	}
	hidden := saturatingSub(len(occ.Processes), 3) + saturatingSub(len(occ.LaunchdLabels), 3)
	if hidden > 0 {
		lines = append(lines, fmt.Sprintf(t.occupancyMore, hidden))
	}
	lines = append(lines, advice)
	ui.Column(c.Key("apps-occupancy-banner")).FillWidth().Padding(12).Gap(4).Background(cautionContainer).Radius(8).Children(func() {
		ui.Text(c, title).FontSize(14).Bold().TextColor(caution)
		for i, line := range lines {
			ui.Text(c.Key(fmt.Sprintf("apps-occupancy-line-%d", i)), line).FontSize(12).TextColor(caution)
		}
	})
}

func truncateRunes(s string, n int) string {
	r := []rune(s)
	if len(r) <= n {
		return s
	}
	return string(r[:n])
}

func saturatingSub(a, b int) int {
	if a < b {
		return 0
	}
	return a - b
}

// RenderActionBar is also available to a parent-owned residual dialog. Ordinary
// Apps inventory has no bulk-delete bar in Rust; this bar belongs to review only.
func (s AppsPage) RenderActionBar(c *ui.Context, a AppsPageActions) {
	if !s.ResidualsOpen {
		return
	}
	t := appsPageStrings(s.Language)
	count := 0
	for _, item := range s.Residuals {
		if item.Selected && !item.Manual {
			count++
		}
	}
	ui.Row(c.Key("apps-residual-actions")).FillWidth().Padding(8, 0, 0, 0).Gap(8).BorderWidth(1, 0, 0, 0).BorderColor(outlineVariant.Alpha(.4)).Children(func() {
		if len(s.Residuals) == 0 {
			ui.Spacer(c)
			appsPageButton(c, "apps-residual-done", t.done, s.busy(), true, a.CloseResiduals)
			return
		}
		appsPageButton(c, "apps-select-recommended", t.recommended, s.busy(), false, a.SelectRecommended)
		appsPageButton(c, "apps-select-all", t.all, s.busy(), false, a.SelectAllResiduals)
		appsPageButton(c, "apps-select-none", t.clear, s.busy() || count == 0, false, a.ClearResidualSelection)
		ui.Spacer(c)
		appsPageButton(c, "apps-residual-cancel", t.cancel, false, false, a.CloseResiduals)
		appsPageButton(c, "apps-residual-clean", fmt.Sprintf(t.clean, count, s.SelectedSize), s.busy() || count == 0 || a.CleanSelectedResiduals == nil, true, a.CleanSelectedResiduals)
	})
}
