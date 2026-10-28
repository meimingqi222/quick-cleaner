package ui

import (
	"time"

	"fmt"
	"math"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

type ConfirmationKind uint8

const (
	ConfirmDelete ConfirmationKind = iota
	ConfirmReleaseCache
	ConfirmUninstall
	ConfirmEndProcess
	ConfirmInstallHelper
	ConfirmInstallUpdate
)

// ConfirmationDialog carries presentation only. ID identifies the controller's
// pending request; accept must revalidate it, never reconstruct targets from copy.
type ConfirmationDialog struct {
	Open                                 bool
	Language                             Language
	ID                                   string
	Kind                                 ConfirmationKind
	Title, Body, Detail                  core.Text
	AppData, Force, Busy, AcceptDisabled bool
}

type ConfirmationDialogActions struct {
	Cancel func(id string)
	Accept func(id string)
}

// productDialogFrame uses MyGo's documented custom modal primitive rather than
// Modal/DialogBase, whose Escape/outside dismissal differs from Rust. Root must
// render only its selected topmost modal (Open=false for suspended snapshots).
// Modal owns focus trapping/restoration, inert background and accessibility.
func productDialogFrame(c *ui.Context, key, title string, width, padding, gap float32, body func()) {
	w, h := c.Size()
	ui.Overlay(c.Key(key), func() {
		back := ui.Box(c).Absolute().Left(0).Top(0).Right(0).Bottom(0).Center().Modal()
		// The dialog fades in and grows from 96%; the overlay is new each
		// time, so an Enter transition has no previous frame to start from.
		// Keyframes play once on the first build; reduced motion ends at 1.
		enter := back.Keyframes("qc-dialog-enter", dialogEnter)
		back.Background(ui.RGBA(0, 0, 0, .4*enter))
		// Swallow Escape without changing the caller's explicit open snapshot.
		back.OverlayShortcut(0, ui.KeyEscape)
		back.Children(func() {
			scale := 0.96 + 0.04*enter
			ui.Column(c).Role(ui.RoleDialog).Label(title).Width(width).MaxWidth(max(float32(0), w-32)).MaxHeight(max(float32(0), h-32)).Padding(padding).Gap(gap).Radius(12).Background(card).Shadow(0, 10, 30, 0, ui.RGBA(0, 0, 0, .3)).Opacity(enter).Scale(scale, scale).Children(body)
		})
	})
}

// dialogEnter is the 0→1 progress of a dialog appearing.
var dialogEnter = []ui.Keyframe{{At: 0, Value: 0}, {At: 180 * time.Millisecond, Value: 1, Ease: ui.EaseOut}}

func productDialogButton(c *ui.Context, key, label string, disabled, accent, danger bool, action func()) {
	b := ui.Button(c.Key(key), label).Disabled(disabled || action == nil).FontSize(12).Padding(8, 12).Radius(8)
	if accent {
		b.Background(primary).TextColor(card)
	}
	if danger {
		b.Background(ui.Hex("#ba1a1a")).TextColor(card)
	}
	if b.Clicked() && !disabled && action != nil {
		action()
	}
}

func productDialogShield(c *ui.Context, fg ui.Color, size float32) {
	ui.Box(c).Size(size, size).Shrink(0).Border(2, fg).Radius(6, 6, 12, 12).Center().Children(func() {
		ui.Box(c).Size(size*.35, size*.35).Radius(2).Background(fg)
	})
}

func (s ConfirmationDialog) Render(c *ui.Context, a ConfirmationDialogActions) {
	if !s.Open {
		return
	}
	t := productDialogStrings(s.Language)
	label, neutral := t.delete, false
	switch s.Kind {
	case ConfirmReleaseCache:
		label = t.release
		if s.Force {
			label = t.forceRelease
		}
	case ConfirmUninstall:
		label, neutral = t.uninstall, true
	case ConfirmEndProcess:
		label = t.kill
	case ConfirmInstallHelper:
		label, neutral = t.install, true
	case ConfirmInstallUpdate:
		label, neutral = t.restart, true
	}
	productDialogFrame(c, "product-confirm", s.Title.Get(s.Language), 460, 24, 12, func() {
		ui.Row(c).Gap(12).AlignItems(ui.Center).Children(func() {
			view, fg, bg := Junk, ui.Hex("#ba1a1a"), ui.Hex("#ffdad6")
			if neutral {
				view, fg, bg = Apps, primary, primaryFixed
			}
			ui.Box(c).Size(40, 40).Shrink(0).Radius(12).Border(1, fg.Alpha(.4)).Background(bg).Center().Children(func() {
				if s.Kind == ConfirmEndProcess {
					productDialogShield(c, fg, 20)
				} else {
					icon(c, view, fg, 20)
				}
			})
			ui.Text(c, s.Title.Get(s.Language)).FontSize(18).Bold()
		})
		ui.Text(c, s.Body.Get(s.Language)).FontSize(14).TextColor(text)
		detailColor := ui.Hex("#ba1a1a")
		if neutral {
			detailColor = outline
		}
		detail := func() { ui.Text(c, s.Detail.Get(s.Language)).FontSize(12).TextColor(detailColor) }
		if s.Kind == ConfirmReleaseCache {
			ui.Scroll(c.Key("product-confirm-detail")).FillWidth().MaxHeight(260).Shrink(1).Children(detail)
		} else {
			detail()
		}
		if s.AppData {
			ui.Text(c, t.warning).FontSize(12).Padding(8, 12).Radius(6).Background(ui.Hex("#ffdad6")).TextColor(ui.Hex("#ba1a1a")).Border(1, ui.Hex("#ba1a1a"))
		}
		ui.Row(c).FillWidth().Gap(8).Justify(ui.End).Padding(8, 0, 0, 0).Children(func() {
			var cancel, accept func()
			if a.Cancel != nil {
				cancel = func() { a.Cancel(s.ID) }
			}
			if a.Accept != nil {
				accept = func() { a.Accept(s.ID) }
			}
			productDialogButton(c, "confirm-cancel", t.cancel, s.Busy, false, false, cancel)
			productDialogButton(c, "confirm-accept", label, s.Busy || s.AcceptDisabled, s.Kind == ConfirmInstallHelper || s.Kind == ConfirmInstallUpdate, s.Kind != ConfirmInstallHelper && s.Kind != ConfirmInstallUpdate, accept)
		})
	})
}

// Open is root's ordering decision, not Update.ShowDialog. Pass the immutable
// controller snapshot; no controller work is performed while building the view.
type UpdateDialog struct {
	Open     bool
	Language Language
	Update   UpdateSnapshot
}
type UpdateDialogActions struct {
	Later, SkipVersion, Download, Install, Retry func()
	OpenRelease                                  func(url string)
}

func (s UpdateDialog) Render(c *ui.Context, a UpdateDialogActions) {
	if !s.Open {
		return
	}
	t, status := productDialogStrings(s.Language), s.Update.Status
	title, body, label, enabled := t.checking, "", t.later, false
	var primaryAction func()
	switch status.Kind {
	case core.UpdateAvailable:
		title, body, label, enabled, primaryAction = "v"+status.LatestVersion, strings.TrimSpace(status.Notes), t.download, true, a.Download
		if body == "" {
			body = status.ReleaseURL
		} else {
			body = core.Truncate(body, 240)
		}
	case core.UpdateDownloading:
		p := status.Progress.Percent
		if math.IsNaN(float64(p)) || math.IsInf(float64(p), 0) {
			p = 0
		}
		title, body, label = t.downloading, fmt.Sprintf("%.0f%%", max(float32(0), min(float32(100), p))), t.downloading
	case core.UpdateVerifying:
		title, label = t.verifying, t.verifying
	case core.UpdateDownloaded:
		title, body, label, enabled, primaryAction = "v"+status.LatestVersion, t.ready, t.restart, true, a.Install
	case core.UpdateInstalling:
		title, label = t.installing, t.installing
	case core.UpdateError:
		title, body, label, enabled, primaryAction = t.failed, status.Message, t.retry, true, a.Retry
	}
	productDialogFrame(c, "product-update", t.update, 420, 20, 16, func() {
		ui.Row(c).Gap(12).AlignItems(ui.Center).Children(func() {
			ui.Image(c, logo).Size(36, 36).Shrink(0)
			ui.Column(c).Children(func() { ui.Text(c, t.update).FontSize(16).Bold(); ui.Text(c, title).FontSize(14).TextColor(muted) })
		})
		if body != "" {
			ui.Column(c).MaxHeight(120).Clip().Children(func() { ui.Text(c, body).FontSize(14).TextColor(outline) })
		}
		ui.Row(c).FillWidth().Gap(8).Children(func() {
			if status.Kind == core.UpdateAvailable {
				productDialogButton(c, "update-skip", t.skip, false, false, false, a.SkipVersion)
			}
			if status.Kind == core.UpdateAvailable || status.Kind == core.UpdateDownloading {
				var open func()
				if a.OpenRelease != nil && status.ReleaseURL != "" {
					open = func() { a.OpenRelease(status.ReleaseURL) }
				}
				productDialogButton(c, "update-release", t.releasePage, false, false, false, open)
			}
			ui.Spacer(c)
			productDialogButton(c, "update-later", t.later, false, false, false, a.Later)
		})
		productDialogButton(c, "update-primary", label, !enabled, true, false, primaryAction)
	})
}

type PermissionDialog struct {
	Open          bool
	Language      Language
	DontAsk, Busy bool
	// Status is controller-localized feedback, never inferred from a settings click.
	Status core.Text
}
type PermissionDialogActions struct {
	Later, CheckStatus, OpenSettings func()
	SetDontAsk                       func(bool)
}

func (s PermissionDialog) Render(c *ui.Context, a PermissionDialogActions) {
	if !s.Open {
		return
	}
	t := productDialogStrings(s.Language)
	productDialogFrame(c, "product-fda", t.fdaTitle, 580, 24, 16, func() {
		ui.Row(c).Gap(16).Children(func() {
			ui.Box(c).Size(48, 48).Shrink(0).Radius(12).Border(1, primary.Alpha(.4)).Background(primaryFixed).Center().Children(func() { productDialogShield(c, primary, 22) })
			ui.Column(c).Grow(1).MinWidth(0).Gap(2).Children(func() {
				ui.Text(c, t.fdaTitle).FontSize(18).Bold()
				ui.Text(c, t.fdaDesc).FontSize(12).TextColor(muted)
			})
		})
		ui.Column(c).Gap(8).Children(func() {
			for i, step := range t.steps {
				ui.Row(c).FillWidth().Padding(8).Gap(12).Radius(8).Background(surfaceLow).Children(func() {
					ui.Text(c, fmt.Sprint(i+1)).Size(22, 22).Shrink(0).Center().Radius(999).Background(primaryFixed).TextColor(primary).FontSize(12).Bold()
					ui.Column(c).Grow(1).MinWidth(0).Gap(1).Children(func() { ui.Text(c, step[0]).FontSize(12).Bold(); ui.Text(c, step[1]).FontSize(12).TextColor(muted) })
				})
			}
		})
		ui.Text(c, t.notice).FontSize(12).TextColor(outline).Padding(8, 12).Radius(8).Background(surfaceHigh)
		if status := s.Status.Get(s.Language); status != "" {
			ui.Text(c, status).FontSize(12).TextColor(muted)
		}
		ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Justify(ui.SpaceBetween).Padding(8, 0, 0, 0).BorderWidth(1, 0, 0, 0).BorderColor(outlineVariant.Alpha(.4)).Children(func() {
			dontAsk := s.DontAsk
			check := ui.Checkbox(c.Key("fda-dont-ask"), &dontAsk, t.dontAsk).FontSize(12).MinWidth(0).Shrink(1).Disabled(s.Busy || a.SetDontAsk == nil)
			if check.Changed() && !s.Busy && a.SetDontAsk != nil {
				a.SetDontAsk(dontAsk)
			}
			ui.Row(c).Shrink(0).Gap(8).Justify(ui.End).Children(func() {
				productDialogButton(c, "fda-later", t.configureLater, s.Busy, false, false, a.Later)
				productDialogButton(c, "fda-check", t.check, s.Busy, false, false, a.CheckStatus)
				productDialogButton(c, "fda-settings", t.settings, s.Busy, true, false, a.OpenSettings)
			})
		})
	})
}
