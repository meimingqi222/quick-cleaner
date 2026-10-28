package ui

import (
	"archive/zip"
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

type fixtureUpdateHost struct {
	cache                        string
	packaged                     bool
	cleanups, extracts, installs atomic.Int32
	installFail                  atomic.Bool
}

func (h *fixtureUpdateHost) Packaged() bool        { return h.packaged }
func (h *fixtureUpdateHost) Cleanup()              { h.cleanups.Add(1) }
func (h *fixtureUpdateHost) Cache() (string, bool) { return h.cache, h.cache != "" }
func (h *fixtureUpdateHost) Extract(archive, cache string) (string, error) {
	h.extracts.Add(1)
	staging, err := core.CreateStagingDir(cache)
	if err != nil {
		return "", err
	}
	return core.ExtractUpdatePayload(archive, staging, core.PayloadSpec{Kind: core.WindowsExe, Name: "quick-cleaner.exe"}, core.DefaultExtractLimits())
}
func (h *fixtureUpdateHost) Install(payload string) error {
	h.installs.Add(1)
	if h.installFail.Load() {
		return fmt.Errorf("fixture install preflight refused")
	}
	data, err := os.ReadFile(payload)
	if err != nil {
		return err
	}
	if string(data) != "new application" {
		return fmt.Errorf("incorrect payload: %q", data)
	}
	return nil
}

type updateFixture struct {
	controller        *UpdateController
	host              *fixtureUpdateHost
	checks, downloads atomic.Int32
	corrupt           atomic.Bool
	checkFailure      atomic.Bool
	checkEntered      chan struct{}
	checkRelease      chan struct{}
}

func newUpdateFixture(t *testing.T, packaged bool, prefs UpdatePreferences, blockCheck bool) *updateFixture {
	t.Helper()
	f := &updateFixture{host: &fixtureUpdateHost{cache: t.TempDir(), packaged: packaged}}
	if blockCheck {
		f.checkEntered, f.checkRelease = make(chan struct{}, 1), make(chan struct{})
	}
	var buf bytes.Buffer
	w := zip.NewWriter(&buf)
	entry, err := w.Create("quick-cleaner.exe")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = entry.Write([]byte("new application")); err != nil {
		t.Fatal(err)
	}
	if err = w.Close(); err != nil {
		t.Fatal(err)
	}
	archive := buf.Bytes()
	asset := core.CandidateAssetNames(core.CurrentTarget())[0]
	var server *httptest.Server
	server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/latest":
			f.checks.Add(1)
			if f.checkFailure.Load() {
				http.Error(w, "fixture check failed", http.StatusServiceUnavailable)
				return
			}
			if f.checkRelease != nil {
				select {
				case f.checkEntered <- struct{}{}:
				default:
				}
				select {
				case <-f.checkRelease:
				case <-r.Context().Done():
					return
				}
			}
			release := core.GithubRelease{TagName: "v0.0.17", HTMLURL: "https://example.test/release", Assets: []core.GithubAsset{{Name: asset, BrowserDownloadURL: server.URL + "/asset", Size: uint64(len(archive))}}}
			_ = json.NewEncoder(w).Encode(release)
		case "/asset":
			f.downloads.Add(1)
			_, _ = w.Write(archive)
		case "/asset.sha256":
			sum := sha256.Sum256(archive)
			if f.corrupt.Load() {
				sum[0] ^= 1
			}
			_, _ = fmt.Fprintf(w, "%x  %s\n", sum, asset)
		default:
			http.NotFound(w, r)
		}
	}))
	client := core.NewUpdateHTTPClient("0.0.16")
	client.Client, client.LatestReleaseURL = server.Client(), server.URL+"/latest"
	f.controller = newUpdateController("0.0.16", prefs, func(UpdatePreferences) {}, client, f.host)
	t.Cleanup(func() { f.controller.Close(); server.Close() })
	return f
}

func awaitUpdate(t *testing.T, c *UpdateController, predicate func(UpdateSnapshot) bool) UpdateSnapshot {
	t.Helper()
	timer := time.NewTimer(8 * time.Second)
	defer timer.Stop()
	for {
		s := c.Snapshot()
		if predicate(s) {
			return s
		}
		select {
		case <-c.Changes():
		case <-timer.C:
			t.Fatalf("update state timed out: %+v", c.Snapshot())
		}
	}
}

func TestUpdateControllerDownloadsButNeverInstallsWithoutClick(t *testing.T) {
	f := newUpdateFixture(t, true, UpdatePreferences{AutoCheck: true}, true)
	c := f.controller
	c.OpenDialog()
	if c.Snapshot().ShowDialog {
		t.Fatal("idle dialog opened")
	}
	c.Check(true)
	select {
	case <-f.checkEntered:
	case <-time.After(8 * time.Second):
		t.Fatal("check did not start")
	}
	for range 5 {
		c.Check(true)
		c.Check(false)
	}
	close(f.checkRelease)
	s := awaitUpdate(t, c, func(s UpdateSnapshot) bool {
		return s.Status.Kind == core.UpdateDownloaded || s.Status.Kind == core.UpdateError
	})
	if s.Status.Kind != core.UpdateDownloaded {
		t.Fatal(s.Status.Message)
	}
	if f.checks.Load() != 1 || f.downloads.Load() != 1 || f.host.extracts.Load() != 1 || f.host.installs.Load() != 0 || s.ReadyToQuit || s.ShowDialog {
		t.Fatal("incorrect automatic behavior", s, f.checks.Load(), f.downloads.Load())
	}
	if s.Preferences.LastCheckAt == nil || s.Status.LatestVersion != "0.0.17" {
		t.Fatal(s)
	}
	c.Check(true)
	if c.Snapshot().Status.Kind != core.UpdateDownloaded {
		t.Fatal("downloaded payload replaced by check")
	}
	c.OpenDialog()
	if !c.Snapshot().ShowDialog {
		t.Fatal("downloaded dialog did not open")
	}
	if !c.Install(true, false) || f.host.installs.Load() != 0 {
		t.Fatal("busy confirmation bypassed")
	}
	c.Install(true, true)
	s = awaitUpdate(t, c, func(s UpdateSnapshot) bool { return s.ReadyToQuit || s.Status.Kind == core.UpdateError })
	if !s.ReadyToQuit || s.Status.Kind != core.UpdateInstalling || f.host.installs.Load() != 1 {
		t.Fatal(s)
	}
}

func TestUpdateControllerGatesAndSkippedRelease(t *testing.T) {
	dev := newUpdateFixture(t, false, UpdatePreferences{AutoCheck: true}, false)
	dev.controller.StartScheduler()
	dev.controller.Check(true)
	dev.controller.Check(false)
	if dev.controller.Snapshot().Status.Kind != core.UpdateIdle || dev.checks.Load() != 0 || dev.host.cleanups.Load() != 0 {
		t.Fatal("development build network gate")
	}
	version := "v0.0.17"
	f := newUpdateFixture(t, true, UpdatePreferences{SkippedVersion: &version}, false)
	c := f.controller
	c.StartScheduler()
	if f.host.cleanups.Load() != 1 {
		t.Fatal("disabled updates prevented backup cleanup")
	}
	c.Check(false)
	if c.Snapshot().Status.Kind != core.UpdateIdle {
		t.Fatal("disabled auto check started")
	}
	c.Check(true)
	s := awaitUpdate(t, c, func(s UpdateSnapshot) bool {
		return s.Status.Kind == core.UpdateNotAvailable || s.Status.Kind == core.UpdateError
	})
	if s.Status.Kind != core.UpdateNotAvailable || f.downloads.Load() != 0 || f.host.extracts.Load() != 0 {
		t.Fatal("skipped version downloaded", s)
	}
	*s.Preferences.SkippedVersion = "changed outside controller"
	if *c.Snapshot().Preferences.SkippedVersion != version {
		t.Fatal("snapshot aliases preferences")
	}
}

func TestUpdateControllerCorruptionRetryAndInstallFailure(t *testing.T) {
	f := newUpdateFixture(t, true, UpdatePreferences{}, false)
	f.corrupt.Store(true)
	c := f.controller
	c.Check(true)
	s := awaitUpdate(t, c, func(s UpdateSnapshot) bool { return s.Status.Kind == core.UpdateError })
	if s.Status.Operation != core.UpdateDownload || f.host.extracts.Load() != 0 || f.host.installs.Load() != 0 || s.ReadyToQuit {
		t.Fatal("corrupt download escaped integrity gate", s)
	}
	entries, err := os.ReadDir(f.host.cache)
	if err != nil {
		t.Fatal(err)
	}
	for _, e := range entries {
		if e.IsDir() {
			t.Fatal("corrupt archive extracted", e.Name())
		}
	}
	f.corrupt.Store(false)
	c.Retry()
	s = awaitUpdate(t, c, func(s UpdateSnapshot) bool {
		return s.Status.Kind == core.UpdateDownloaded || s.Status.Kind == core.UpdateError
	})
	if s.Status.Kind != core.UpdateDownloaded || f.checks.Load() != 2 || f.downloads.Load() != 2 {
		t.Fatal("retry did not recheck/redownload", s)
	}
	f.host.installFail.Store(true)
	c.Install(false, false)
	s = awaitUpdate(t, c, func(s UpdateSnapshot) bool { return s.Status.Kind == core.UpdateError })
	if s.Status.Operation != core.UpdateInstall || s.ReadyToQuit {
		t.Fatal("failed handoff quit the application", s)
	}
	c.Retry()
	awaitUpdate(t, c, func(s UpdateSnapshot) bool { return s.Status.Kind == core.UpdateDownloaded })
	c.SkipVersion()
	s = c.Snapshot()
	if s.Preferences.SkippedVersion == nil || *s.Preferences.SkippedVersion != "0.0.17" || s.Status.Kind != core.UpdateDownloaded {
		t.Fatal("skip canceled an already downloaded payload", s)
	}
	if _, err := os.Stat(filepath.Join(f.host.cache, core.CandidateAssetNames(core.CurrentTarget())[0]+".sha256")); err != nil {
		t.Fatal(err)
	}
}

func TestUpdateControllerCheckFailureStaysSilentUnlessManual(t *testing.T) {
	f := newUpdateFixture(t, true, UpdatePreferences{AutoCheck: true}, false)
	f.checkFailure.Store(true)
	c := f.controller
	c.Check(false)
	s := awaitUpdate(t, c, func(s UpdateSnapshot) bool { return s.Status.Kind == core.UpdateError })
	if s.Status.Operation != core.UpdateCheck || s.ShowDialog || s.ManualCheckError != "" || s.Preferences.LastCheckAt == nil {
		t.Fatal(s)
	}
	c.Retry()
	s = awaitUpdate(t, c, func(s UpdateSnapshot) bool { return s.Status.Kind == core.UpdateError })
	if s.ManualCheckError == "" || s.ShowDialog || f.downloads.Load() != 0 || f.host.installs.Load() != 0 {
		t.Fatal(s)
	}
}
