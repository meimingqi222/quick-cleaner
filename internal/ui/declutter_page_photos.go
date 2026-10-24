package ui

import (
	"fmt"
	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func (s DeclutterPage) photos(c *ui.Context, a DeclutterPageActions) {
	ui.Row(c).FillWidth().Gap(12).Children(func() {
		ui.Text(c, s.tr("kicker")).Grow(1).FontSize(12).TextColor(outline)
		s.button(c, "smart", s.tr("smart"), a.SmartSelectPhotos, !s.busy())
	})
	if len(s.Photos) == 0 {
		s.empty(c)
	}
	for gi, g := range s.Photos[:min(30, len(s.Photos))] {
		// This is the source's display fallback, not a selection policy: a group
		// without a best marker still shows its first photo in the hero slot.
		best := -1
		for i, p := range g.Photos {
			if p.IsBestShot {
				best = i
				break
			}
		}
		if best < 0 && len(g.Photos) > 0 {
			best = 0
		}
		var redundant []int
		for i := range g.Photos {
			if i != best {
				redundant = append(redundant, i)
			}
		}
		ui.Column(c).FillWidth().Shrink(0).Radius(12).Border(1, outlineVariant.Alpha(.4)).Clip().Gap(0).Background(card).Children(func() {
			ui.Row(c).FillWidth().Padding(12, 16).Gap(8).Background(surfaceLow).Children(func() {
				ui.Text(c, g.IndexStr).FontSize(12).Padding(4, 8).Radius(999).Background(primaryFixed).TextColor(primary)
				ui.Column(c).Grow(1).MinWidth(0).Gap(2).Children(func() {
					ui.Text(c, g.Title.Get(s.Language)).FontSize(14).Bold()
					stats := fmt.Sprintf(s.tr("groupKept"), len(g.Photos))
					if g.SelectedCount() > 0 {
						stats = fmt.Sprintf(s.tr("groupSelected"), len(g.Photos), g.SelectedCount(), core.FmtSize(g.CleanableSize()))
					}
					ui.Text(c, stats).FontSize(12).TextColor(muted)
				})
				s.button(c, fmt.Sprintf("best-%d", gi), s.tr("keepBest"), func() { a.KeepBestPhotos(gi) }, !s.busy() && a.KeepBestPhotos != nil)
				s.button(c, fmt.Sprintf("keep-%d", gi), s.tr("keepAll"), func() { a.KeepAllPhotos(gi) }, !s.busy() && a.KeepAllPhotos != nil)
			})
			ui.Row(c).FillWidth().Padding(24).Gap(24).AlignItems(ui.Start).Children(func() {
				if best >= 0 {
					s.photoCard(c, a, gi, best, g.Photos[best], true)
				}
				ui.Column(c).Grow(1).MinWidth(0).Gap(12).Children(func() {
					ui.Text(c, fmt.Sprintf(s.tr("redundant"), len(redundant))).FontSize(12).Bold().TextColor(muted)
					ui.Row(c).FillWidth().Wrap().Gap(16).AlignItems(ui.Start).Children(func() {
						limit := len(redundant)
						if !s.ExpandedPhotoGroups[gi] {
							limit = min(4, limit)
						}
						for _, pi := range redundant[:limit] {
							s.photoCard(c, a, gi, pi, g.Photos[pi], false)
						}
						if len(redundant) > 4 {
							label := s.tr("collapse")
							if !s.ExpandedPhotoGroups[gi] {
								label = fmt.Sprintf(s.tr("more"), len(redundant)-4)
							}
							s.button(c, fmt.Sprintf("expand-%d", gi), label, func() { a.TogglePhotoGroup(gi) }, a.TogglePhotoGroup != nil).Width(205).Height(180).Background(surfaceLow)
						}
					})
				})
			})
		})
	}
}

func (s DeclutterPage) photoCard(c *ui.Context, a DeclutterPageActions, gi, pi int, p core.PhotoItem, hero bool) {
	key := fmt.Sprintf("declutter-photo-%d-%d", gi, pi)
	w, h := float32(205), float32(115)
	if hero {
		w, h = 250, 180
	}
	ui.Column(c.Key(key)).Width(w).Shrink(0).Radius(12).Border(1, outlineVariant.Alpha(.5)).Clip().Gap(0).ContextMenu(s.contextMenu(a, p.Filename, p.Path)).Children(func() {
		thumb := ui.ButtonBase(c.Key(key + "-preview")).FillWidth().Height(h).Padding(0).Background(ui.Hex(fmt.Sprintf("#%06x", p.BGGradientSeed&0xffffff))).Label(s.tr("preview") + " " + p.Filename).Disabled(a.Open == nil)
		thumb.Children(func() {
			if bitmap := s.Thumbnails[p.Path]; bitmap != nil {
				ui.Image(c, bitmap).FillWidth().Height(h)
			} else {
				ui.Text(c, fmt.Sprintf("%d x %d", p.Dimensions[0], p.Dimensions[1])).FontSize(14).TextColor(card)
			}
		})
		if thumb.Clicked() && a.Open != nil {
			a.Open(p.Path)
		}
		ui.Column(c).FillWidth().Padding(12).Gap(6).Children(func() {
			if hero {
				ui.Text(c, s.tr("best")).FontSize(12).Bold().TextColor(primary)
				ui.Text(c, p.Filename).FontSize(12).Bold().SingleLine()
			} else {
				selected := p.Selected
				check := ui.Checkbox(c.Key(key+"-select"), &selected, p.Filename).Label(fmt.Sprintf(s.tr("selectionLabel"), p.Filename)).Disabled(s.busy() || a.TogglePhoto == nil)
				if check.Changed() && a.TogglePhoto != nil {
					a.TogglePhoto(gi, pi)
				}
				label := s.tr("kept")
				if p.Selected {
					label = s.tr("toClean")
				}
				fg := muted
				if p.Selected {
					fg = declutterError
				}
				ui.Text(c, label).FontSize(12).TextColor(fg)
			}
			ui.Text(c, fmt.Sprintf("%d x %d · %s", p.Dimensions[0], p.Dimensions[1], core.FmtSize(p.Size))).FontSize(12).TextColor(muted)
			if hero {
				ui.Text(c, p.DateStr).FontSize(12).TextColor(muted)
			}
			ui.Row(c).Gap(4).Children(func() {
				s.button(c, key+"-reveal", s.tr("reveal"), func() { a.Reveal(p.Path) }, a.Reveal != nil).Padding(2, 6)
				s.button(c, key+"-open", s.tr("preview"), func() { a.Open(p.Path) }, a.Open != nil).Padding(2, 6)
			})
		})
	})
}
