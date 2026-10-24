package ui

import (
	"fmt"
	"reflect"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func productClick(t *testing.T, tt *ui.Tester, label string) {
	t.Helper()
	if err := tt.Click(label); err != nil {
		t.Fatal(err)
	}
}

func TestProductDialogConfirmationLanguagesAndKinds(t *testing.T) {
	for _, lang := range []Language{Chinese, English} {
		copy := productDialogStrings(lang)
		for kind, label := range map[ConfirmationKind]string{ConfirmDelete: copy.delete, ConfirmReleaseCache: copy.release, ConfirmUninstall: copy.uninstall, ConfirmEndProcess: copy.kill, ConfirmInstallHelper: copy.install, ConfirmInstallUpdate: copy.restart} {
			t.Run(fmt.Sprint(lang, "/", kind), func(t *testing.T) {
				s := ConfirmationDialog{Open: true, Language: lang, ID: "request-42", Kind: kind, Title: core.NewText("标题", "Title"), Body: core.NewText("正文", "Body"), Detail: core.NewText("详情", "Details"), AppData: true}
				original := s
				accepted, cancelled := 0, 0
				a := ConfirmationDialogActions{Accept: func(id string) {
					if id != s.ID {
						t.Fatal(id)
					}
					accepted++
				}, Cancel: func(id string) {
					if id != s.ID {
						t.Fatal(id)
					}
					cancelled++
				}}
				tt := ui.NewTester(func(c *ui.Context) { s.Render(c, a) }, 1280, 900)
				for _, text := range []string{s.Title.Get(lang), s.Body.Get(lang), s.Detail.Get(lang), copy.warning, label, copy.cancel} {
					if !tt.HasText(text) {
						t.Fatalf("missing %q", text)
					}
				}
				if accepted+cancelled != 0 {
					t.Fatal("render invoked callback")
				}
				productClick(t, tt, label)
				productClick(t, tt, copy.cancel)
				if accepted != 1 || cancelled != 1 || s != original {
					t.Fatal("callback or snapshot violation")
				}
				s.Busy = true
				tt.Frame()
				productClick(t, tt, label)
				productClick(t, tt, copy.cancel)
				if accepted != 1 || cancelled != 1 {
					t.Fatal("busy callbacks")
				}
				s.Busy = false
				s.AcceptDisabled = true
				tt.Frame()
				productClick(t, tt, label)
				if accepted != 1 {
					t.Fatal("disabled accept")
				}
			})
		}
		tt := ui.NewTester(func(c *ui.Context) {
			ConfirmationDialog{Open: true, Language: lang, Kind: ConfirmReleaseCache, Force: true}.Render(c, ConfirmationDialogActions{})
		}, 900, 700)
		if !tt.HasText(copy.forceRelease) {
			t.Fatal("force release copy")
		}
	}
}

func TestProductDialogUpdateStatesAndEvents(t *testing.T) {
	for _, lang := range []Language{Chinese, English} {
		txt := productDialogStrings(lang)
		for kind := core.UpdateIdle; kind <= core.UpdateError; kind++ {
			t.Run(fmt.Sprint(lang, "/", kind), func(t *testing.T) {
				s := UpdateDialog{Open: true, Language: lang, Update: UpdateSnapshot{Status: core.UpdateStatus{Kind: kind, LatestVersion: "2.0", Notes: " release notes ", ReleaseURL: "https://example.test/release", Message: "network failed", Progress: core.DownloadProgress{Percent: 37.6}}}}
				original := s
				var events []string
				a := UpdateDialogActions{Later: func() { events = append(events, "later") }, SkipVersion: func() { events = append(events, "skip") }, Download: func() { events = append(events, "download") }, Install: func() { events = append(events, "install") }, Retry: func() { events = append(events, "retry") }, OpenRelease: func(url string) { events = append(events, url) }}
				tt := ui.NewTester(func(c *ui.Context) { s.Render(c, a) }, 1280, 900)
				if !tt.HasText(txt.update) || len(events) != 0 {
					t.Fatal("initial render")
				}
				label, want := txt.later, ""
				switch kind {
				case core.UpdateAvailable:
					label, want = txt.download, "download"
					if !tt.HasText("release notes") {
						t.Fatal("notes")
					}
				case core.UpdateDownloading:
					label = txt.downloading
					if !tt.HasText("38%") {
						t.Fatal("progress")
					}
				case core.UpdateVerifying:
					label = txt.verifying
				case core.UpdateDownloaded:
					label, want = txt.restart, "install"
					if !tt.HasText(txt.ready) {
						t.Fatal("ready")
					}
				case core.UpdateInstalling:
					label = txt.installing
				case core.UpdateError:
					label, want = txt.retry, "retry"
					if !tt.HasText("network failed") {
						t.Fatal("error")
					}
				}
				// Idle fallback has two Later labels; only its explicit footer is active.
				if kind >= core.UpdateAvailable {
					productClick(t, tt, label)
				}
				if want != "" {
					if !reflect.DeepEqual(events, []string{want}) {
						t.Fatal(events)
					}
				} else if len(events) != 0 {
					t.Fatal("busy primary action", events)
				}
				if kind == core.UpdateAvailable {
					productClick(t, tt, txt.skip)
				} else if tt.HasText(txt.skip) {
					t.Fatal("skip outside available")
				}
				if kind == core.UpdateAvailable || kind == core.UpdateDownloading {
					productClick(t, tt, txt.releasePage)
					if events[len(events)-1] != s.Update.Status.ReleaseURL {
						t.Fatal(events)
					}
				} else if tt.HasText(txt.releasePage) {
					t.Fatal("release outside allowed states")
				}
				productClick(t, tt, txt.later)
				if events[len(events)-1] != "later" || !reflect.DeepEqual(s, original) {
					t.Fatal("later or snapshot")
				}
			})
		}
	}
}

func TestProductDialogUpdateNotesFallbackAndUnicode(t *testing.T) {
	for _, notes := range []string{" \n ", strings.Repeat("更新", 150)} {
		s := UpdateDialog{Open: true, Language: English, Update: UpdateSnapshot{Status: core.UpdateStatus{Kind: core.UpdateAvailable, Notes: notes, ReleaseURL: "https://example.test"}}}
		tt := ui.NewTester(func(c *ui.Context) { s.Render(c, UpdateDialogActions{}) }, 1000, 800)
		want := core.Truncate(strings.TrimSpace(notes), 240)
		if strings.TrimSpace(notes) == "" {
			want = s.Update.Status.ReleaseURL
		}
		if !tt.HasText(want) {
			t.Fatal("notes fallback/truncation")
		}
	}
}

func TestProductDialogPermissionEventsAndBusy(t *testing.T) {
	for _, lang := range []Language{Chinese, English} {
		for _, busy := range []bool{false, true} {
			t.Run(fmt.Sprint(lang, "/", busy), func(t *testing.T) {
				s := PermissionDialog{Open: true, Language: lang, DontAsk: true, Busy: busy, Status: core.NewText("尚未授权", "Not yet granted")}
				original := s
				var events []string
				a := PermissionDialogActions{Later: func() { events = append(events, "later") }, CheckStatus: func() { events = append(events, "check") }, OpenSettings: func() { events = append(events, "settings") }, SetDontAsk: func(v bool) { events = append(events, fmt.Sprint(v)) }}
				txt := productDialogStrings(lang)
				tt := ui.NewTester(func(c *ui.Context) { s.Render(c, a) }, 1280, 1000)
				for _, want := range []string{txt.fdaTitle, txt.fdaDesc, txt.notice, s.Status.Get(lang)} {
					if !tt.HasText(want) {
						t.Fatal(want)
					}
				}
				for _, step := range txt.steps {
					for _, want := range step {
						if !tt.HasText(want) {
							t.Fatal(want)
						}
					}
				}
				if len(events) != 0 {
					t.Fatal("render side effect")
				}
				for _, label := range []string{txt.configureLater, txt.check, txt.settings, txt.dontAsk} {
					productClick(t, tt, label)
				}
				want := []string{"later", "check", "settings", "false"}
				if busy {
					want = nil
				}
				if !reflect.DeepEqual(events, want) || s != original {
					t.Fatalf("events=%v want=%v snapshot=%v", events, want, s)
				}
			})
		}
	}
}

func TestProductDialogModalPolicyAndRootOrdering(t *testing.T) {
	for _, which := range []string{"confirm", "update", "permission"} {
		t.Run(which, func(t *testing.T) {
			open := false
			actions, background := 0, 0
			tt := ui.NewTester(func(c *ui.Context) {
				if ui.Button(c, "background").Clicked() {
					background++
					open = true
				}
				cancel := func() { actions++; open = false }
				switch which {
				case "confirm":
					ConfirmationDialog{Open: open, Language: English, Title: core.SameText("Confirm")}.Render(c, ConfirmationDialogActions{Cancel: func(string) { cancel() }, Accept: func(string) { actions++ }})
				case "update":
					UpdateDialog{Open: open, Language: English, Update: UpdateSnapshot{Status: core.UpdateStatus{Kind: core.UpdateDownloaded}}}.Render(c, UpdateDialogActions{Later: cancel, Install: func() { actions++ }})
				case "permission":
					PermissionDialog{Open: open, Language: English}.Render(c, PermissionDialogActions{Later: cancel, CheckStatus: func() { actions++ }, OpenSettings: func() { actions++ }})
				}
			}, 1280, 1000)
			productClick(t, tt, "background")
			for i := 0; i < 6; i++ {
				tt.Key(0, ui.KeyTab)
				if tt.Focused("background") {
					t.Fatal("Tab escaped the modal")
				}
			}
			tt.Key(0, ui.KeyEscape)
			tt.ClickAt(3, 3)
			if !open || actions != 0 || background != 1 {
				t.Fatal("escape/outside dismissed or leaked")
			}
			cancelLabel := "Cancel"
			if which == "update" {
				cancelLabel = "Later"
			}
			if which == "permission" {
				cancelLabel = "Configure Later"
			}
			productClick(t, tt, cancelLabel)
			if open || actions != 1 || !tt.Focused("background") {
				t.Fatal("explicit cancel or focus restore")
			}
		})
	}
	// Root suspends lower snapshots explicitly; the hidden confirmation cannot act.
	tt := ui.NewTester(func(c *ui.Context) {
		ConfirmationDialog{Open: false, Language: English}.Render(c, ConfirmationDialogActions{})
		UpdateDialog{Open: false, Language: English}.Render(c, UpdateDialogActions{})
		PermissionDialog{Open: false, Language: English}.Render(c, PermissionDialogActions{})
	}, 1000, 800)
	if len(tt.Texts()) != 0 {
		t.Fatal("closed snapshots rendered", tt.Texts())
	}
}

func TestProductDialogMissingCallbacksAndProgressBounds(t *testing.T) {
	for _, p := range []struct {
		value float32
		label string
	}{{-2, "0%"}, {150, "100%"}, {0, "0%"}} {
		tt := ui.NewTester(func(c *ui.Context) {
			UpdateDialog{Open: true, Language: English, Update: UpdateSnapshot{Status: core.UpdateStatus{Kind: core.UpdateDownloading, Progress: core.DownloadProgress{Percent: p.value}}}}.Render(c, UpdateDialogActions{})
		}, 1000, 800)
		if !tt.HasText(p.label) {
			t.Fatal(p.label, tt.Texts())
		}
		productClick(t, tt, "Later")
	}
	for _, lang := range []Language{Chinese, English} {
		txt := productDialogStrings(lang)
		tt := ui.NewTester(func(c *ui.Context) {
			ConfirmationDialog{Open: true, Language: lang}.Render(c, ConfirmationDialogActions{})
		}, 1000, 800)
		if tt.HasText(txt.warning) {
			t.Fatal("app-data warning without app-data snapshot")
		}
		productClick(t, tt, txt.cancel)
		productClick(t, tt, txt.delete)
		tt = ui.NewTester(func(c *ui.Context) { PermissionDialog{Open: true, Language: lang}.Render(c, PermissionDialogActions{}) }, 1280, 1000)
		for _, label := range []string{txt.configureLater, txt.check, txt.settings, txt.dontAsk} {
			productClick(t, tt, label)
		}
	}
}
