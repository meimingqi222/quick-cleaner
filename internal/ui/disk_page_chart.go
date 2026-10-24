package ui

import (
	"fmt"
	"math"
	"path/filepath"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

var diskColors = [6]ui.Color{primaryBright, ui.Hex("#7547ab"), ui.Hex("#059669"), ui.Hex("#d97706"), ui.Hex("#10b981"), ui.Hex("#64748b")}

type diskBreakdownItem struct {
	name  string
	size  uint64
	ratio float64
	color ui.Color
	row   *DiskPageRow
}

func (s DiskPage) breakdown() (string, uint64, string, []diskBreakdownItem) {
	t := diskStrings(s.Language)
	var items []diskBreakdownItem
	if s.Tab == DiskFiles {
		var sizes [5]uint64
		var total uint64
		for _, r := range s.Rows {
			i := 4
			switch strings.ToLower(filepath.Ext(r.Node.Name)) {
			case ".mp4", ".mkv", ".avi", ".mov", ".flv", ".mp3", ".wav":
				i = 0
			case ".exe", ".dll", ".sys", ".node", ".pak", ".bin", ".msi":
				i = 1
			case ".zip", ".rar", ".7z", ".tar", ".gz", ".iso", ".vmdk", ".vhdx":
				i = 2
			case ".dat", ".db", ".log", ".txt", ".pdf", ".docx", ".xlsx", ".sqlite":
				i = 3
			}
			sizes[i] += r.Node.Size
			total += r.Node.Size
		}
		for i, n := range sizes {
			if n > 0 {
				col := i
				if i == 4 {
					col = 5
				}
				items = append(items, diskBreakdownItem{name: t.kinds[i], size: n, ratio: float64(n) / float64(max(total, 1)), color: diskColors[col]})
			}
		}
		return t.filesTitle, total, fmt.Sprintf(t.filesCount, len(s.Rows)), items
	}
	physical := s.AtRoot && s.Space != nil
	base := max(s.CurrentSize, 1)
	if !physical {
		var childTotal uint64
		for _, r := range s.Rows {
			childTotal += r.Node.Size
		}
		base = max(base, childTotal)
	}
	var topSum uint64
	for _, r := range s.Rows {
		if len(items) == 4 {
			break
		}
		ratio := float64(r.Node.Size) / float64(base)
		if physical && ratio < .01 {
			continue
		}
		topSum += r.Node.Size
		items = append(items, diskBreakdownItem{name: r.Node.Name, size: r.Node.Size, ratio: min(ratio, 1), color: diskColors[len(items)], row: &r})
	}
	size := s.CurrentSize
	summary := fmt.Sprintf(t.folderItems, len(s.Rows))
	remaining, threshold := size, uint64(1024)
	if physical {
		size = 0
		if s.Space.Total > s.Space.Free {
			size = s.Space.Total - s.Space.Free
		}
		remaining, threshold = size, 1024*1024
		summary = fmt.Sprintf(t.usedFree, math.Round(float64(size)/float64(max(s.Space.Total, 1))*100), core.FmtSize(s.Space.Free))
	}
	if remaining > topSum && remaining-topSum > threshold {
		label := fmt.Sprintf(t.other, max(0, len(s.Rows)-len(items)))
		if physical {
			label = fmt.Sprintf(t.otherUsed, max(0, len(s.Rows)-len(items)))
		}
		items = append(items, diskBreakdownItem{name: label, size: remaining - topSum, ratio: min(float64(remaining-topSum)/float64(base), 1), color: diskColors[5]})
	}
	if physical {
		if s.Space.Free > 0 {
			items = append(items, diskBreakdownItem{name: t.free, size: s.Space.Free, ratio: float64(s.Space.Free) / float64(base), color: diskColors[4]})
		}
		var sum float64
		for _, item := range items {
			sum += item.ratio
		}
		if sum > 0 {
			for i := range items {
				items[i].ratio /= sum
			}
		}
	}
	return s.CurrentName, size, summary, items
}

func (s DiskPage) lens(c *ui.Context, a DiskPageActions) {
	t := diskStrings(s.Language)
	title, size, summary, items := s.breakdown()
	dominant := primary
	if len(items) > 0 {
		dominant = items[0].color
	}
	ui.Column(c.Key("disk-lens")).Width(290).Shrink(0).FillHeight().MinHeight(0).Padding(16).Gap(12).Radius(12).Border(1, outlineVariant.Alpha(.5)).Children(func() {
		ui.Row(c).FillWidth().Height(28).Gap(8).Children(func() {
			ui.ButtonBase(c.Key("disk-volume-selector")).Padding(3, 8).Radius(8).Border(1, outlineVariant.Alpha(.7)).Disabled(len(s.Volumes) <= 1 || a.SwitchVolume == nil).Menu(func(m *ui.Menu) {
				for _, v := range s.Volumes {
					label := s.volumeLabel(v.ID)
					if usage := s.volumeUsage(v.Space); usage != "" {
						label += " — " + usage
					}
					if m.Item(label).Checked(v.ID == s.Volume).Chosen() && a.SwitchVolume != nil {
						a.SwitchVolume(v.ID)
					}
				}
			}).Children(func() { ui.Text(c, s.volumeLabel(s.Volume)+" ▾").FontSize(12).Bold().SingleLine() })
			label := fmt.Sprintf(t.scanned, core.FmtSize(s.UniqueSize))
			if s.Space != nil {
				label = fmt.Sprintf(t.total, core.FmtSize(s.Space.Total))
			}
			ui.Text(c, label).FontSize(12).TextColor(outline).Padding(2, 8).Background(surfaceHigh).Radius(6).SingleLine()
		})
		ui.Column(c).Grow(1).FillWidth().Center().Gap(12).Padding(8, 0).Children(func() {
			ui.Box(c.Key("disk-ring")).Size(176, 176).Shrink(0).Draw(func(p *ui.Painter, r ui.Rect) {
				var track ui.Path
				track.Circle(r.X+88, r.Y+88, 81)
				p.StrokePath(&track, 14, ui.Hex("#e2e8f0"))
				var ratioSum float64
				for _, item := range items {
					ratioSum += item.ratio
				}
				angle := -math.Pi / 2
				for _, item := range items {
					ratio := item.ratio / max(ratioSum, 1)
					end := angle + ratio*2*math.Pi
					a0, a1 := angle-.75/88, end+.75/88
					steps := max(4, int(math.Ceil((a1-a0)/(2*math.Pi)*64)))
					var arc ui.Path
					arc.MoveTo(r.X+88+88*float32(math.Cos(a0)), r.Y+88+88*float32(math.Sin(a0)))
					for j := 1; j <= steps; j++ {
						x := a0 + (a1-a0)*float64(j)/float64(steps)
						arc.LineTo(r.X+88+88*float32(math.Cos(x)), r.Y+88+88*float32(math.Sin(x)))
					}
					for j := steps; j >= 0; j-- {
						x := a0 + (a1-a0)*float64(j)/float64(steps)
						arc.LineTo(r.X+88+74*float32(math.Cos(x)), r.Y+88+74*float32(math.Sin(x)))
					}
					arc.Close()
					if ratio > .002 {
						p.FillPath(&arc, item.color)
					}
					angle = end
				}
			}).Children(func() {
				ui.Column(c).Absolute().Top(18).Left(18).Size(140, 140).Radius(999).Background(card).Center().Padding(8).Gap(2).Children(func() {
					ui.Text(c, title).MaxWidth(124).FontSize(12).Bold().TextColor(dominant).SingleLine().Tooltip(title)
					ui.Text(c, core.FmtSize(size)).FontSize(20).Bold()
					ui.Text(c, summary).MaxWidth(124).FontSize(12).TextColor(outline)
				})
			})
			ui.Column(c).FillWidth().Padding(0, 8).Gap(4).Children(func() {
				ui.Row(c).FillWidth().Children(func() {
					ui.Text(c, t.breakdown).Grow(1).FontSize(12).TextColor(outline)
					ui.Text(c, fmt.Sprintf(t.topCategories, len(items))).FontSize(12).TextColor(outline)
				})
				ui.Row(c).FillWidth().Height(8).Radius(999).Clip().Gap(0).Background(surfaceHigh).Children(func() {
					for _, item := range items {
						if item.ratio > .005 {
							ui.Box(c).WidthPercent(float32(min(item.ratio, 1) * 100)).Shrink(0).Height(8).Background(item.color)
						}
					}
				})
			})
		})
		ui.Column(c).FillWidth().Gap(4).Padding(8, 0, 0, 0).BorderWidth(1, 0, 0, 0).BorderColor(outlineVariant.Alpha(.4)).Children(func() {
			for i, item := range items {
				b := ui.ButtonBase(c.Key(fmt.Sprintf("disk-breakdown-%d", i))).FillWidth().Padding(8).Radius(8).Gap(8).Tooltip(item.name)
				if b.Hovered() {
					b.Background(surfaceLow)
				}
				b.Children(func() {
					ui.Box(c).Size(10, 10).Shrink(0).Radius(999).Background(item.color)
					ui.Text(c, item.name).Grow(1).MinWidth(0).FontSize(12).SingleLine()
					ui.Text(c, fmt.Sprintf("%.1f%%", item.ratio*100)).Shrink(0).FontSize(12).Bold().Padding(1, 8).Radius(999).TextColor(item.color).Background(surfaceHigh)
					ui.Text(c, core.FmtSize(item.size)).Shrink(0).FontSize(12).Bold()
				})
				if b.Clicked() && item.row != nil && item.row.CanEnter && a.Navigate != nil {
					a.Navigate(item.row.Node.Idx)
				}
			}
		})
	})
}
