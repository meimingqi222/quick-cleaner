package ui

import (
	"fmt"
	"math"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

var statusError = ui.Hex("#ba1a1a")

func statusCard(c *ui.Context, key string) ui.Element {
	return ui.Column(c.Key(key)).Grow(1).Basis(0).MinWidth(210).MinHeight(232).Padding(20).Border(1, outlineVariant.Alpha(.5)).Radius(12).Background(card).Clip().Gap(0)
}

func statusHeader(c *ui.Context, title, chip string, view View) {
	ui.Row(c).FillWidth().Gap(8).Margin(0, 0, 16, 0).Children(func() {
		icon(c, view, muted, 16)
		ui.Text(c, title).MinWidth(0).FontSize(14).FixedLineHeight(20).FontWeight(600).TextColor(text)
		ui.Spacer(c)
		if chip != "" {
			ui.Text(c, chip).MinWidth(0).MaxWidth(115).Clip().FontSize(12).FixedLineHeight(16).Padding(2, 6).Radius(6).Background(surfaceLow).TextColor(muted)
		}
	})
}

func statusNumber(c *ui.Context, value, unit string) {
	ui.Row(c).Gap(6).AlignItems(ui.Baseline).Children(func() {
		ui.Text(c, value).FontSize(20).FixedLineHeight(28).Bold().TextColor(text)
		ui.Text(c, unit).FontSize(12).FontWeight(500).TextColor(muted)
	})
}

func statusFooter(c *ui.Context, cells ...string) {
	ui.Text(c, strings.Join(cells, " · ")).FillWidth().FontSize(12).FixedLineHeight(16).TextColor(outline).Margin(12, 0, 0, 0)
}

func statusMeter(c *ui.Context, ratio float32, fg ui.Color) {
	if math.IsNaN(float64(ratio)) {
		ratio = 0
	}
	ui.Box(c).FillWidth().Height(6).Radius(999).Background(surfaceHighest).Clip().Margin(12, 0, 0, 0).Children(func() {
		ui.Box(c).WidthPercent(100 * min(float32(1), max(float32(0), ratio))).FillHeight().Background(fg).Radius(999)
	})
}

func statusHistory(c *ui.Context, history []float32, height float32) {
	if len(history) > 24 {
		history = history[len(history)-24:]
	}
	ui.Row(c).FillWidth().Height(height).Gap(2).AlignItems(ui.End).Margin(12, 0, 0, 0).Children(func() {
		for i := 0; i < 24; i++ {
			h, fg := float32(2), surfaceHighest
			if i < len(history) {
				ratio := history[i] / 100
				if math.IsNaN(float64(ratio)) {
					ratio = 0
				}
				h = height * min(float32(1), max(float32(.03), ratio))
				fg = primary.Alpha(.25 + .6*float32(i)/24)
				if i == len(history)-1 {
					fg = primary
				}
			}
			ui.Box(c).Grow(1).Basis(0).Height(h).Radius(2, 2, 0, 0).Background(fg)
		}
	})
}

func statusTemp(temp *float32) string {
	if temp == nil {
		return ""
	}
	return fmt.Sprintf("%.0f °C", *temp)
}

func (s *StatusPage) renderHardware(c *ui.Context, a StatusPageActions) {
	snap := s.Snapshot
	cards := []func(*ui.Context){s.healthCard, func(c *ui.Context) {
		statusCard(c, "cpu-card").Children(func() {
			statusHeader(c, s.tr("cpu"), statusTemp(snap.Thermal.CPUTemp), Status)
			statusNumber(c, fmt.Sprintf("%.0f", snap.CPUUsage), "%")
			statusHistory(c, s.CPUHistory, 64)
			statusFooter(c, fmt.Sprintf("%s × %d", s.tr("cores"), snap.CoreCount))
		})
	}}
	if len(snap.GPUs) != 0 {
		cards = append(cards, func(c *ui.Context) { s.gpuCard(c, a) })
	}
	cards = append(cards, s.memoryCard, s.diskCard, s.networkCard)
	if snap.Battery != nil {
		cards = append(cards, s.batteryCard)
	}
	cards = append(cards, func(c *ui.Context) { s.fanCard(c, a) })
	ui.Scroll(c.Key("status-hardware")).Grow(1).MinHeight(0).FillWidth().Padding(20).Gap(16).Children(func() {
		ui.Column(c).Gap(4).Children(func() {
			ui.Text(c, s.tr("overview")).FontSize(16).FixedLineHeight(24).Bold().TextColor(text)
			ui.Text(c, s.tr("overviewSub")).FontSize(12).FixedLineHeight(16).TextColor(muted)
		})
		for start := 0; start < len(cards); start += 4 {
			ui.Row(c.Key(start)).FillWidth().Wrap().Gap(12).Children(func() {
				for j := 0; j < 4; j++ {
					if start+j < len(cards) {
						cards[start+j](c)
					} else {
						// P2: invisible cells have identical padding/border flex metrics.
						ui.Box(c).Grow(1).Basis(0).MinWidth(210).Padding(20).Border(1, ui.RGBA(0, 0, 0, 0))
					}
				}
			})
		}
	})
}

func (s *StatusPage) healthCard(c *ui.Context) {
	snap := s.Snapshot
	avg := snap.CPUUsage
	if len(s.CPUHistory) > 0 {
		avg = 0
		for _, v := range s.CPUHistory {
			avg += v
		}
		avg /= float32(len(s.CPUHistory))
	}
	var disk *float32
	if s.DiskTotal > 0 {
		ratio := float32(s.DiskFree) / float32(s.DiskTotal)
		disk = &ratio
	}
	h := core.ComputeHealthReport(core.HealthInputs{DiskFreeRatio: disk, MemUsed: snap.MemUsed, MemTotal: snap.MemTotal, SwapUsed: snap.SwapUsed, CPUAvg: avg, UptimeSecs: snap.UptimeSecs})
	word, fg := "good", primary
	if h.Score < 60 {
		word, fg = "high", statusError
	} else if h.Score < 80 {
		word, fg = "fair", caution
	}
	statusCard(c, "health-card").Children(func() {
		statusHeader(c, s.tr("health"), snap.OSName, Overview)
		ui.Row(c).FillWidth().Gap(12).Margin(8, 0, 0, 0).Children(func() {
			ui.Column(c).Grow(1).MinWidth(0).Gap(4).Children(func() {
				ui.Row(c).Gap(8).AlignItems(ui.Baseline).Children(func() {
					ui.Text(c, s.tr(word)).FontSize(20).Bold().TextColor(fg)
					ui.Text(c, fmt.Sprintf("%d/100", h.Score)).FontSize(12).TextColor(outline)
				})
				ui.Text(c, core.FmtSize(s.DiskFree)+" "+s.tr("available")).FontSize(12).TextColor(muted)
				factor := "clear"
				if h.Worst != nil {
					factor = []string{"factorDisk", "factorSwap", "factorMemory", "factorCPU", "factorUptime"}[*h.Worst]
				}
				ui.Text(c, s.tr(factor)).FontSize(12).TextColor(outline)
			})
			ui.Box(c).Size(72, 72).Shrink(0).Radius(999).Border(7, primary).Center().Children(func() {
				ui.Column(c).Center().Gap(0).Children(func() {
					ui.Text(c, fmt.Sprint(h.Score)).FontSize(14).Bold().TextColor(fg)
					ui.Text(c, s.tr("healthShort")).FontSize(12).TextColor(outline)
				})
			})
		})
		statusFooter(c, s.tr("uptime")+" "+s.uptime(snap.UptimeSecs))
	})
}

func (s *StatusPage) gpuCard(c *ui.Context, a StatusPageActions) {
	gpus := s.Snapshot.GPUs
	selected := 0
	for i, g := range gpus {
		if g.ID == s.SelectedGPU {
			selected = i
		}
	}
	g := gpus[selected]
	chip := statusTemp(g.TempC)
	if chip == "" && g.VRAMInUse != nil {
		chip = s.tr("vram") + " " + core.FmtMem(*g.VRAMInUse)
	}
	statusCard(c, "gpu-card").Children(func() {
		statusHeader(c, "GPU", chip, Status)
		value := "—"
		if g.Utilization != nil {
			value = fmt.Sprintf("%.0f", *g.Utilization)
		}
		statusNumber(c, value, "%")
		statusHistory(c, s.GPUHistory[g.ID], 64)
		if len(gpus) > 1 {
			ui.Row(c).Gap(2).Margin(12, 0, 0, 0).Children(func() {
				labels := core.GPULabels(gpus)
				for i, gpu := range gpus {
					statusButton(c, gpu.ID, labels[i], selected == i, a.SelectGPU == nil, func() { a.SelectGPU(gpu.ID) })
				}
			})
		} else {
			var footer []string
			if g.Name != nil {
				footer = append(footer, *g.Name)
			}
			if g.RendererUtilization != nil {
				footer = append(footer, fmt.Sprintf("%s %.0f%%", s.tr("renderer"), *g.RendererUtilization))
			}
			if g.TempC != nil && g.VRAMInUse != nil {
				footer = append(footer, s.tr("vram")+" "+core.FmtMem(*g.VRAMInUse))
			}
			if len(footer) == 0 {
				footer = append(footer, s.tr("gpuUnknown"))
			}
			statusFooter(c, footer...)
		}
	})
}

func (s *StatusPage) memoryCard(c *ui.Context) {
	p := s.Snapshot
	ratio := float32(0)
	if p.MemTotal > 0 {
		ratio = float32(p.MemUsed) / float32(p.MemTotal)
	}
	statusCard(c, "memory-card").Children(func() {
		statusHeader(c, s.tr("memory"), "", Apps)
		statusNumber(c, core.FmtMem(p.MemUsed), s.tr("used"))
		statusMeter(c, ratio, primary)
		footer := []string{fmt.Sprintf("%s %.0f%%", s.tr("used"), ratio*100), s.tr("total") + " " + core.FmtMem(p.MemTotal)}
		if p.SwapTotal > 0 {
			footer = append(footer, s.tr("swap")+" "+core.FmtMem(p.SwapUsed))
		}
		statusFooter(c, footer...)
	})
}

func (s *StatusPage) diskCard(c *ui.Context) {
	used := s.DiskTotal - min(s.DiskFree, s.DiskTotal)
	ratio := float32(0)
	chip := ""
	if s.DiskTotal > 0 {
		ratio = float32(used) / float32(s.DiskTotal)
		chip = s.tr("total") + " " + core.FmtSize(s.DiskTotal)
	}
	statusCard(c, "disk-card").Children(func() {
		statusHeader(c, s.tr("disk"), chip, Disk)
		statusNumber(c, core.FmtSize(s.DiskFree), s.tr("available"))
		statusMeter(c, ratio, primary)
		statusFooter(c, s.tr("used")+" "+core.FmtSize(used), fmt.Sprintf("%.0f%%", ratio*100))
	})
}

func (s *StatusPage) networkCard(c *ui.Context) {
	statusCard(c, "network-card").Children(func() {
		statusHeader(c, s.tr("network"), "", Search)
		ui.Column(c).Gap(8).Margin(4, 0, 0, 0).Children(func() {
			ui.Text(c, "↓  "+core.FmtSize(uint64(max(0, s.Snapshot.RxBPS)))+"/s").FontSize(16).Bold().TextColor(primary)
			ui.Text(c, "↑  "+core.FmtSize(uint64(max(0, s.Snapshot.TxBPS)))+"/s").FontSize(16).TextColor(muted)
		})
		statusFooter(c, s.tr("liveRates"))
	})
}

func (s *StatusPage) batteryCard(c *ui.Context) {
	b := s.Snapshot.Battery
	fg := primary
	if !b.Charging && b.Percent < 20 {
		fg = statusError
	} else if !b.Charging && b.Percent < 40 {
		fg = caution
	}
	state := "onBattery"
	if b.FullyCharged {
		state = "charged"
	} else if b.Charging {
		state = "charging"
	} else if b.External {
		state = "plugged"
	}
	line := s.tr(state)
	if b.MinutesRemaining != nil && !b.FullyCharged {
		m := *b.MinutesRemaining
		span := s.tr("minutes", m)
		if m >= 60 {
			span = s.tr("hoursMinutes", m/60, m%60)
		}
		key := "left"
		if b.Charging {
			key = "toFull"
		}
		line += " · " + s.tr(key, span)
	}
	statusCard(c, "battery-card").Children(func() {
		statusHeader(c, s.tr("battery"), statusTemp(b.TempC), Overview)
		statusNumber(c, fmt.Sprintf("%.0f", b.Percent), "%")
		statusMeter(c, b.Percent/100, fg)
		ui.Text(c, line).FontSize(12).TextColor(muted).Margin(8, 0, 0, 0)
		var footer []string
		if b.CycleCount != nil {
			v := fmt.Sprintf("%s %d", s.tr("cycles"), *b.CycleCount)
			if b.DesignCycleCount != nil {
				v += fmt.Sprintf("/%d", *b.DesignCycleCount)
			}
			footer = append(footer, v)
		}
		if b.HealthPercent != nil {
			footer = append(footer, fmt.Sprintf("%s %.0f%%", s.tr("batteryHealth"), *b.HealthPercent))
		}
		statusFooter(c, footer...)
	})
}

func (s *StatusPage) fanCard(c *ui.Context, a StatusPageActions) {
	t := s.Snapshot.Thermal
	statusCard(c, "fan-card").Children(func() {
		statusHeader(c, s.tr("fan"), statusTemp(t.CPUTemp), Status)
		if len(t.Fans) > 0 {
			statusNumber(c, core.Commas(uint64(math.Round(float64(max(0, t.Fans[0].RPM))))), "RPM")
		} else {
			ui.Text(c, s.tr("noFan")).FontSize(16).TextColor(outline)
		}
		if len(t.Fans) > 0 && s.FanControlSupported {
			ui.Row(c).Gap(2).Margin(12, 0, 0, 0).Children(func() {
				for i, mode := range []core.FanMode{{Kind: core.FanAuto}, {Kind: core.FanPercent, Percent: 60}, {Kind: core.FanPercent, Percent: 100}} {
					if mode.Percent == 60 && t.CPUTemp == nil {
						continue
					}
					statusButton(c, fmt.Sprintf("fan-mode-%d", i), s.tr([]string{"auto", "cool", "full"}[i]), s.FanMode == mode, s.FanApplying || a.ApplyFanMode == nil, func() { a.ApplyFanMode(mode) })
				}
			})
		}
		if s.FanHelperInstalled {
			ui.Box(c).Margin(8, 0, 0, 0).Children(func() {
				statusButton(c, "remove-helper", s.tr("removeHelper"), false, s.FanApplying || a.RemoveFanHelper == nil, a.RemoveFanHelper)
			})
		}
		state := "managed"
		if s.FanMode.Kind != core.FanAuto {
			state = "manual"
		}
		statusFooter(c, fmt.Sprintf("%s %d", s.tr("fans"), len(t.Fans)), s.tr(state))
	})
}
