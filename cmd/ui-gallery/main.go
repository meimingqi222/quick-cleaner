// ui-gallery is a non-destructive migration fixture, not the product entry point.
package main

import (
	"flag"
	"fmt"
	"image/png"
	"log"
	"os"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
	appui "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func main() {
	english := flag.Bool("en", false, "English fixture")
	page := flag.String("page", "overview", "overview, junk, apps, status, search, disk, declutter or tray")
	details := flag.Bool("details", false, "show page-specific detail fixtures")
	dialog := flag.String("dialog", "", "confirm, release, uninstall, permission, update, downloading, ready or error")
	processes := flag.Bool("processes", false, "show the status process tab")
	dev := flag.Bool("dev", false, "show the Apps developer-environment tab")
	files := flag.Bool("files", false, "show Disk Lens large files")
	tab := flag.String("tab", "overview", "declutter: overview, photos, duplicates, large or downloads")
	state := flag.String("state", "idle", "idle, scanning, empty, found, cleaning")
	mac := flag.Bool("macos", false, "macOS permission badge fixture")
	output := flag.String("output", "", "render PNG without opening a window")
	width := flag.Int("width", 1280, "logical client width")
	height := flag.Int("height", 880, "logical client height")
	scale := flag.Float64("scale", 1.5, "PNG scale")
	flag.Parse()
	if *width <= 0 || *height <= 0 || *scale <= 0 {
		log.Fatal("dimensions and scale must be positive")
	}
	s := appui.Dashboard{Cleanable: "0 B", Freed: "0 B", Version: "v0.0.16", MacOS: *mac}
	if *english {
		s.Language = appui.English
	}
	switch *state {
	case "idle":
	case "scanning":
		s.Scanning = true
	case "empty":
		s.Scanned, s.AppsScanned = true, true
	case "found", "cleaning":
		s.Scanned, s.HasJunk, s.AppsScanned = true, true, true
		s.Cleanable, s.Categories, s.AppCount = "1.25 GB", 3, 17
		s.Cleaning = *state == "cleaning"
	default:
		log.Fatal(fmt.Errorf("unknown fixture state %q", *state))
	}
	// Business callbacks are intentionally absent, matching the Rust page gallery.
	actions := appui.DashboardActions{SetLanguage: func(l appui.Language) { s.Language = l }}
	view := func(c *ui.Context) { s.Render(c, actions) }
	switch *page {
	case "overview":
	case "junk":
		junk := junkFixture(s, *details)
		junkActions := appui.JunkPageActions{
			ToggleExpanded:      func(id core.CategoryID) { junk.Expanded[id] = !junk.Expanded[id] },
			ToggleExclusions:    func() { junk.ShowExclusions = !junk.ShowExclusions },
			ToggleFailedDetails: func() { junk.ShowFailedDetails = !junk.ShowFailedDetails },
		}
		view = func(c *ui.Context) {
			junk.Language = s.Language
			s.RenderPage(c, appui.Junk, actions, func() { junk.RenderContent(c, junkActions) })
		}
	case "apps":
		apps, appsActions := appsFixture(s, *details)
		envs, envActions := devEnvFixture(s)
		if *dev {
			apps.Tab = appui.AppsPageDevEnvironments
		}
		appsActions.SetTab = func(tab appui.AppsPageTab) { apps.Tab = tab }
		appsActions.RenderDevEnvironments = func(c *ui.Context) { envs.RenderContent(c, envActions) }
		view = func(c *ui.Context) {
			apps.Language, envs.Language = s.Language, s.Language
			s.RenderPage(c, appui.Apps, actions, func() { apps.RenderContent(c, appsActions) })
		}
	case "status":
		status, statusActions := statusFixture(s, *processes, *details)
		actions.SetStatusTab = func(tab appui.StatusTab) { status.Tab = tab }
		actions.ToggleMonitoring = func() { s.MonitoringPaused = !s.MonitoringPaused }
		view = func(c *ui.Context) {
			status.Language = s.Language
			s.StatusTab = status.Tab
			if status.Snapshot != nil {
				s.ProcessCount = status.Snapshot.ProcessCount
			}
			s.RenderPage(c, appui.Status, actions, func() { status.RenderContent(c, statusActions) })
		}
	case "search":
		search, searchActions := searchFixture(s)
		view = func(c *ui.Context) {
			search.Language = s.Language
			s.RenderPage(c, appui.Search, actions, func() { search.RenderContent(c, searchActions) })
		}
	case "disk":
		disk, diskActions := diskFixture(s, *files, *details)
		view = func(c *ui.Context) {
			disk.Language = s.Language
			s.RenderPage(c, appui.Disk, actions, func() { disk.RenderContent(c, diskActions) })
		}
	case "declutter":
		declutter, declutterActions := declutterFixture(s)
		tabs := map[string]appui.DeclutterTab{"overview": appui.DeclutterOverview, "photos": appui.DeclutterSimilarPhotos, "duplicates": appui.DeclutterDuplicates, "large": appui.DeclutterLargeFiles, "downloads": appui.DeclutterDownloads}
		var ok bool
		if declutter.Tab, ok = tabs[*tab]; !ok {
			log.Fatalf("unknown declutter tab %q", *tab)
		}
		view = func(c *ui.Context) {
			declutter.Language = s.Language
			s.RenderPage(c, appui.Declutter, actions, func() { declutter.RenderContent(c, declutterActions) })
		}
	case "tray":
		tray := trayFixture(s, *details)
		view = func(c *ui.Context) { tray.Language = s.Language; tray.Render(c, appui.TrayPopoverActions{}) }
	default:
		log.Fatalf("unknown fixture page %q", *page)
	}
	if *dialog != "" {
		body := view
		renderDialog, err := dialogFixture(&s, *dialog)
		if err != nil {
			log.Fatal(err)
		}
		view = func(c *ui.Context) { body(c); renderDialog(c) }
	}
	if *output != "" {
		f, err := os.Create(*output)
		if err != nil {
			log.Fatal(err)
		}
		err = png.Encode(f, ui.Render(view, *width, *height, float32(*scale)))
		if closeErr := f.Close(); err == nil {
			err = closeErr
		}
		if err != nil {
			log.Fatal(err)
		}
		return
	}
	mygo.App.WhenReady(func() {
		mygo.NewWindow(mygo.WindowOptions{Title: "QuickCleaner — UI Gallery", Width: *width, Height: *height, Content: ui.View(view)})
	})
	if err := mygo.App.Run(); err != nil {
		log.Fatal(err)
	}
}
