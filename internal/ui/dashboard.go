package ui

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/assets"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

type View uint8

const (
	Overview View = iota
	Status
	Junk
	Apps
	Disk
	Declutter
	Search
)

// Dashboard is a presentation snapshot. The controller supplies formatted sizes
// and native permission facts; rendering neither scans nor infers permissions.
type Dashboard struct {
	Language                             Language
	Scanning, Cleaning, Scanned, HasJunk bool
	Cleanable, Freed, DiskSize           string
	FreedAny, AppsScanned                bool
	AppCount, Categories                 int
	MacOS, Elevated, FullDiskAccess      bool
	Version, StatusText                  string
	UpdateLabel                          string
	UpdateAttention                      bool
	StatusTab                            StatusTab
	ProcessCount                         int
	MonitoringPaused                     bool
	// CleanProgress is the live mutation snapshot. Nil means no docked bar.
	CleanProgress *core.CleanSnapshot
	Stop          func()
}

type DashboardActions struct {
	Navigate         func(View)
	Scan             func()
	SetLanguage      func(Language)
	CheckUpdates     func()
	OpenPermissions  func()
	SetStatusTab     func(StatusTab)
	ToggleMonitoring func()
}

var logo = func() *ui.Bitmap {
	bitmap, err := ui.DecodeBitmap(assets.IconPNG)
	if err != nil {
		panic(err)
	}
	return bitmap
}()

func (s Dashboard) Render(c *ui.Context, actions DashboardActions) {
	s.RenderPage(c, Overview, actions, func() { s.body(c, actions) })
}

// RenderPage keeps the product shell shared by each migrated page. Content owns
// its presentation; navigation and permission actions remain with the controller.
func (s Dashboard) RenderPage(c *ui.Context, active View, actions DashboardActions, content func()) {
	c.SetTheme(theme())
	t := dashboardStrings(s.Language)
	ui.Column(c).Fill().Gap(0).Background(background).TextColor(text).Children(func() {
		ui.Row(c).Grow(1).MinHeight(0).Gap(0).Children(func() {
			s.sidebar(c, active, actions)
			ui.Column(c).Grow(1).MinWidth(0).FillHeight().Gap(0).Background(card).Children(func() {
				s.topbar(c, active, actions)
				if s.Scanning {
					scanBar(c)
				}
				content()
			})
		})
		if s.Cleaning && s.CleanProgress != nil {
			s.progressBar(c)
		}
		status := s.StatusText
		if status == "" {
			status = t.ready
		}
		ui.Box(c).FillWidth().Padding(6, 32).BorderWidth(1, 0, 0, 0).BorderColor(outlineVariant.Alpha(.6)).Children(func() {
			ui.Text(c, status).FontSize(12).FixedLineHeight(16).TextColor(muted)
		})
	})
}

func (s Dashboard) progressBar(c *ui.Context) {
	snap := s.CleanProgress
	if snap == nil {
		return
	}
	en := s.Language == English
	phase := "正在永久删除"
	if snap.Cancelled {
		phase = "正在停止…"
	}
	if en {
		phase = "Deleting permanently"
		if snap.Cancelled {
			phase = "Stopping…"
		}
	}
	counts := fmt.Sprintf("%s 个文件", commas(snap.Files))
	if snap.TotalFiles > 0 {
		counts = fmt.Sprintf("%s / %s 个文件", commas(snap.Files), commas(snap.TotalFiles))
	}
	bytes := core.FmtSize(snap.Bytes)
	if snap.TotalBytes > 0 {
		bytes = core.FmtSize(snap.Bytes) + " / " + core.FmtSize(snap.TotalBytes)
	}
	if en {
		counts = fmt.Sprintf("%s files", commas(snap.Files))
		if snap.TotalFiles > 0 {
			counts = fmt.Sprintf("%s / %s files", commas(snap.Files), commas(snap.TotalFiles))
		}
	}
	fg := ui.Hex("#ba1a1a")
	fill := primary
	if snap.Cancelled {
		fg, fill = outline, outline
	}
	ratio := snap.Ratio()
	ui.Row(c.Key("clean-progress")).FillWidth().Shrink(0).Padding(12, 32).Gap(0).Justify(ui.Center).Background(background).BorderWidth(1, 0, 0, 0).BorderColor(outlineVariant.Alpha(.6)).Children(func() {
		ui.Column(c).Width(720).MaxWidthPercent(100).Gap(8).Children(func() {
			ui.Row(c).FillWidth().Gap(12).Children(func() {
				ui.Text(c, phase).FontSize(14).Bold().TextColor(fg)
				ui.Spacer(c)
				ui.Text(c, counts).FontSize(12).TextColor(muted)
				if snap.Failed > 0 {
					label := fmt.Sprintf("失败 %s", commas(snap.Failed))
					if en {
						label = fmt.Sprintf("%s failed", commas(snap.Failed))
					}
					ui.Text(c, label).FontSize(12).TextColor(fg)
				}
			})
			ui.Box(c).FillWidth().Height(8).Radius(999).Clip().Background(surfaceHighest).Children(func() {
				width := max(float32(2), float32(ratio)*720)
				ui.Box(c).Height(8).Width(width).Radius(999).Background(fill)
			})
			ui.Row(c).FillWidth().Gap(12).Children(func() {
				ui.Text(c, truncateRunes(snap.Current, 72)).Grow(1).MinWidth(0).FontSize(12).TextColor(outline).SingleLine()
				ui.Text(c, bytes).Shrink(0).FontSize(12).Bold()
				stop := "停止"
				if en {
					stop = "Stop"
				}
				button := ui.ButtonBase(c.Key("cancel-clean")).Padding(4, 12).Radius(8).Border(1, outlineVariant).Disabled(snap.Cancelled || s.Stop == nil)
				if button.Hovered() && !snap.Cancelled {
					button.Background(surfaceHigh)
				}
				button.Children(func() { ui.Text(c, stop).FontSize(12) })
				if button.Clicked() && s.Stop != nil && !snap.Cancelled {
					s.Stop()
				}
			})
		})
	})
}

func commas(n uint64) string {
	s := fmt.Sprintf("%d", n)
	if len(s) <= 3 {
		return s
	}
	var b strings.Builder
	prefix := len(s) % 3
	if prefix > 0 {
		b.WriteString(s[:prefix])
	}
	for i := prefix; i < len(s); i += 3 {
		if b.Len() > 0 {
			b.WriteByte(',')
		}
		b.WriteString(s[i : i+3])
	}
	return b.String()
}

func (s Dashboard) sidebar(c *ui.Context, active View, actions DashboardActions) {
	t := dashboardStrings(s.Language)
	ui.Column(c).Width(190).Shrink(0).FillHeight().Gap(0).Background(background).
		BorderWidth(0, 1, 0, 0).BorderColor(outlineVariant.Alpha(.5)).Children(func() {
		ui.Column(c).Padding(12).Gap(12).Children(func() {
			ui.Row(c).Padding(4, 6).Gap(10).Children(func() {
				ui.Image(c, logo).Size(26, 26).Shrink(0)
				ui.Column(c).Gap(0).Children(func() {
					ui.Text(c, "QuickCleaner").FontSize(14).FixedLineHeight(20).Bold()
					ui.Text(c, t.subtitle).FontSize(12).FixedLineHeight(16).TextColor(outline)
				})
			})
			ui.Column(c).Gap(2).Children(func() {
				for v := Overview; v <= Search; v++ {
					fg := muted
					row := ui.ButtonBase(c.Key(fmt.Sprintf("nav-%d", v))).Height(36).Padding(0, 10).Gap(10).Justify(ui.Start).Radius(8).Cursor(ui.CursorPointer)
					if v == active {
						fg = primary
						interactive(row, primaryFixed, primaryFixed, primary.Alpha(.22), true)
					} else {
						interactive(row, ui.Transparent, surfaceLow, surfaceHigh, true)
					}
					row.Children(func() {
						ui.Box(c).Size(22, 22).Center().Children(func() { icon(c, v, fg, 17) })
						weight := 500
						if v == active {
							weight = 600
						}
						ui.Text(c, t.nav[v]).FontSize(13).FixedLineHeight(18).FontWeight(weight).TextColor(fg)
					})
					if row.Clicked() && actions.Navigate != nil {
						actions.Navigate(v)
					}
				}
			})
		})
		ui.Spacer(c)
		ui.Column(c).Padding(12).Gap(10).BorderWidth(1, 0, 0, 0).BorderColor(outlineVariant.Alpha(.4)).Children(func() {
			ui.Row(c).Padding(8, 10).Gap(10).Radius(8).Background(card).Border(1, outlineVariant.Alpha(.4)).Children(func() {
				ui.Box(c).Size(28, 28).Shrink(0).Radius(6).Background(primaryFixed).Center().Children(func() { icon(c, Junk, primary, 14) })
				ui.Column(c).Gap(0).Children(func() {
					ui.Text(c, t.freed).FontSize(12).FixedLineHeight(16).TextColor(muted)
					ui.Text(c, s.Freed).FontSize(13).FixedLineHeight(17).Bold().TextColor(primary)
				})
			})
			ui.Row(c).Padding(0, 4).Justify(ui.SpaceBetween).Children(func() {
				ui.Text(c, t.language).FontSize(12).TextColor(muted).NoWrap().Shrink(0)
				ui.Row(c).Shrink(0).Padding(2).Gap(2).Radius(8).Background(card).Border(1, outlineVariant.Alpha(.5)).Children(func() {
					for _, lang := range []Language{Chinese, English} {
						pill := ui.ButtonBase(c.Key(fmt.Sprintf("lang-%d", lang))).Padding(2, 8).Radius(6).Cursor(ui.CursorPointer)
						fg, weight := muted, 400
						if lang == s.Language {
							pill.Background(primaryFixed)
							fg = primary
							weight = 700
						} else if pill.Hovered() {
							pill.Background(surfaceHigh)
						}
						pill.Children(func() {
							ui.Text(c, lang.DisplayName()).FontSize(12).FixedLineHeight(16).FontWeight(weight).TextColor(fg)
						})
						if pill.Clicked() && actions.SetLanguage != nil {
							actions.SetLanguage(lang)
						}
					}
				})
			})
			label := s.Version
			if s.UpdateLabel != "" {
				label = s.UpdateLabel
			}
			// Checking replaces the version text and must not look clickable.
			quiet := !s.UpdateAttention && (s.UpdateLabel == "" || s.UpdateLabel == s.Version)
			version := ui.ButtonBase(c.Key("version")).FillWidth().Padding(8, 12).Justify(ui.SpaceBetween).Radius(8)
			fg := muted
			if s.UpdateAttention {
				fg = primary
				version.Background(primaryFixed).Cursor(ui.CursorPointer)
			} else if quiet {
				version.Cursor(ui.CursorPointer)
				if version.Hovered() {
					version.Background(surfaceHigh)
				}
			}
			version.Children(func() {
				ui.Text(c, label).FontSize(12).FixedLineHeight(16).TextColor(fg)
				if s.UpdateAttention {
					ui.Text(c, "›").FontSize(12).FixedLineHeight(16).TextColor(fg)
				}
			})
			if (quiet || s.UpdateAttention) && version.Clicked() && actions.CheckUpdates != nil {
				actions.CheckUpdates()
			}
		})
	})
}

func (s Dashboard) topbar(c *ui.Context, active View, actions DashboardActions) {
	t := dashboardStrings(s.Language)
	ui.Row(c).FillWidth().Height(44).Shrink(0).Padding(0, 20).Gap(12).BorderWidth(0, 0, 1, 0).BorderColor(outlineVariant.Alpha(.4)).Children(func() {
		ui.Text(c, t.nav[active]).FontSize(14).Bold()
		fg, bg, border, label := outline, surfaceLow, outlineVariant, t.standard
		if s.Elevated {
			fg, bg, border, label = primary, primaryFixed, primary, t.admin
		}
		if s.MacOS {
			if s.FullDiskAccess {
				fg, bg, border, label = primary, primaryFixed, primary, t.fdaGranted
			} else {
				fg, bg, border, label = caution, cautionContainer, caution.Alpha(.5), t.fdaLimited
			}
		}
		badge := ui.ButtonBase(c.Key("permissions")).Padding(2, 8).Radius(999).Gap(4).Border(1, border).Background(bg)
		badge.Children(func() {
			shield(c, fg)
			ui.Text(c, label).FontSize(12).FixedLineHeight(16).FontWeight(500).TextColor(fg)
		})
		if badge.Clicked() && s.MacOS && !s.FullDiskAccess && actions.OpenPermissions != nil {
			actions.OpenPermissions()
		}
		if active == Status {
			ui.Row(c).Padding(2).Gap(2).Radius(8).Background(surfaceHigh).Children(func() {
				for tab, title := range []string{t.statusHardware, t.statusProcesses} {
					button := ui.ButtonBase(c.Key(fmt.Sprintf("status-tab-%d", tab))).Padding(3, 10).Gap(6).Radius(6).Cursor(ui.CursorPointer)
					fg := muted
					if s.StatusTab == StatusTab(tab) {
						button.Background(card)
						fg = primary
					}
					button.Children(func() {
						ui.Text(c, title).FontSize(12).TextColor(fg)
						if tab == int(StatusProcesses) && s.ProcessCount > 0 {
							ui.Text(c, core.Commas(uint64(s.ProcessCount))).FontSize(11).Padding(0, 5).Radius(999).Background(primaryFixed).TextColor(primary)
						}
					})
					if button.Clicked() && actions.SetStatusTab != nil {
						actions.SetStatusTab(StatusTab(tab))
					}
				}
			})
		}
		ui.Spacer(c)
		if active == Status {
			fg, label := primary, t.statusLive
			if s.MonitoringPaused {
				fg, label = caution, t.statusPaused
			}
			button := ui.ButtonBase(c.Key("status-pause-toggle")).Padding(3, 10).Gap(6).Radius(999).Border(1, fg.Alpha(.25)).Background(fg.Alpha(.08)).Cursor(ui.CursorPointer)
			button.Children(func() {
				if s.MonitoringPaused {
					ui.Text(c, "⏸").FontSize(12).TextColor(fg)
				} else {
					icon(c, Status, fg, 11)
				}
				ui.Text(c, label).FontSize(12).FontWeight(500).TextColor(fg)
			})
			if button.Clicked() && actions.ToggleMonitoring != nil {
				actions.ToggleMonitoring()
			}
			return
		}
		if s.FreedAny {
			ui.Text(c, fmt.Sprintf(t.freedPill, s.Freed)).FontSize(12).Bold().TextColor(primary).Padding(4, 12).Radius(999).Background(primaryFixed)
		}
		label = t.rescan
		if s.Scanning || s.Cleaning {
			label = t.scanning
		}
		button := ui.ButtonBase(c.Key("rescan")).Padding(4, 12).Radius(999).Border(1, outlineVariant).Disabled(s.Scanning || s.Cleaning).Cursor(ui.CursorPointer)
		fg = muted
		if button.Hovered() && !s.Scanning && !s.Cleaning {
			fg = primary
		}
		interactive(button, ui.Transparent, surfaceLow, surfaceHigh, !s.Scanning && !s.Cleaning)
		button.Children(func() { ui.Text(c, label).FontSize(12).FixedLineHeight(16).TextColor(fg) })
		if button.Clicked() && actions.Scan != nil {
			actions.Scan()
		}
	})
}

func (s Dashboard) body(c *ui.Context, actions DashboardActions) {
	t := dashboardStrings(s.Language)
	big, sub, blurb := "Smart Scan", t.start, t.idleBlurb
	switch {
	case s.Scanning:
		big, sub, blurb = t.scanning, t.scanningSub, t.scanningBlurb
	case s.Cleaning:
		big, sub = t.cleaning, t.cleaningSub
	case s.Scanned && s.HasJunk:
		big, sub = s.Cleanable, t.found
	case s.Scanned:
		big, sub = t.clean, t.noJunk
	}
	if !s.Scanning && s.Scanned {
		if s.HasJunk {
			blurb = dashboardFoundBlurb(s.Language, s.Categories, s.Cleanable)
		} else {
			blurb = t.cleanBlurb
		}
	}
	ui.Column(c).Grow(1).MinHeight(0).FillWidth().Justify(ui.Center).Children(func() {
		ui.Scroll(c).FillWidth().Padding(20).Gap(20).AlignItems(ui.Center).Children(func() {
			ui.Box(c).Size(230, 230).Shrink(0).Center().Children(func() {
				ui.Box(c).Absolute().Top(0).Left(0).Size(230, 230).Radius(999).Border(6, surfaceHighest).PassThrough()
				if s.Scanning || s.Cleaning {
					orbitDot(c, 230, 112, 12, primaryBright)
				}
				circle := ui.ButtonBase(c.Key("smart-scan")).Size(174, 174).Shrink(0).Radius(999).Gradient(primaryBright, primary, 150).
					Shadow(0, 20, 25, -5, ui.RGBA(0, 0, 0, .1)).Shadow(0, 8, 10, -6, ui.RGBA(0, 0, 0, .1)).Cursor(ui.CursorPointer)
				// The gradient stays; the circle lifts on hover and dips when
				// pressed, moved in paint-only frames.
				circleScale := float32(1)
				if !s.Scanning && !s.Cleaning {
					if circle.Pressed() {
						circleScale = pressScale
					} else if circle.Hovered() {
						circleScale = 1.03
					}
				}
				circle.AnimatePose(ui.Pose{Scale: circleScale}, ui.Snappy)
				circle.Children(func() {
					ui.Column(c).Fill().Center().Gap(6).Children(func() {
						icon(c, Overview, card, 28)
						ui.Text(c, big).MaxWidth(150).FontSize(20).FixedLineHeight(28).Bold().TextAlign(ui.Center).TextColor(card)
						ui.Text(c, sub).MaxWidth(130).FontSize(12).FixedLineHeight(16).FontWeight(500).TextAlign(ui.Center).TextColor(card.Alpha(.85))
					})
				})
				if circle.Clicked() && !s.Scanning && !s.Cleaning {
					if s.Scanned {
						if actions.Navigate != nil {
							actions.Navigate(Junk)
						}
					} else if actions.Scan != nil {
						actions.Scan()
					}
				}
			})
			ui.Text(c, blurb).MaxWidth(520).FontSize(14).FixedLineHeight(20).TextAlign(ui.Center).TextColor(muted)
			if s.Scanned && s.HasJunk && !s.Cleaning {
				button := ui.ButtonBase(c.Key("review")).Padding(12, 20).Radius(8).Background(primary).Cursor(ui.CursorPointer)
				button.Children(func() { ui.Text(c, t.review).FontSize(14).FontWeight(600).TextColor(card) })
				if button.Clicked() && actions.Navigate != nil {
					actions.Navigate(Junk)
				}
			}
			ui.Row(c).FillWidth().MaxWidth(860).Gap(12).Children(func() {
				for i, v := range []View{Junk, Apps, Disk, Declutter} {
					sub := t.quick[i]
					if v == Junk && s.Scanned {
						sub = s.Cleanable
					}
					if v == Apps && s.AppsScanned {
						sub = fmt.Sprintf(t.appsFound, s.AppCount)
					}
					if v == Disk && s.DiskSize != "" {
						sub = s.DiskSize
					}
					quick := ui.ButtonBase(c.Key(fmt.Sprintf("quick-%d", v))).Grow(1).Basis(0).MinWidth(0).Padding(12, 14).Gap(12).Justify(ui.Start).Radius(12).
						Border(1, outlineVariant.Alpha(.55)).Shadow(0, 1, 2, 0, ui.RGBA(0, 0, 0, .05)).Cursor(ui.CursorPointer)
					interactive(quick, card, surfaceLow, surfaceHigh, true)
					quick.Children(func() {
						ui.Box(c).Size(30, 30).Shrink(0).Radius(12).Background(primaryFixed).Border(1, primary.Alpha(.4)).Center().Children(func() { icon(c, v, primary, 15) })
						ui.Column(c).MinWidth(0).Gap(0).Children(func() {
							ui.Text(c, t.nav[v]).FontSize(14).FixedLineHeight(20).Bold()
							ui.Text(c, sub).FontSize(12).FixedLineHeight(16).TextColor(outline)
						})
					})
					if quick.Clicked() && actions.Navigate != nil {
						actions.Navigate(v)
					}
				}
			})
		})
	})
}
