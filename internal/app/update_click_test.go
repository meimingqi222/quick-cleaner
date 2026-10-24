package app

import (
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func stageDownloadedUpdate(t *testing.T, r *Root) {
	t.Helper()
	r.updates = view.NewUpdateController(Version, view.UpdatePreferences{}, nil)
	// The file is absent, so the platform helper returns before it can spawn.
	r.updates.PrepareDownloaded("0.0.17", filepath.Join(t.TempDir(), "missing-payload.exe"))
	r.update = r.updates.Snapshot()
}

func waitUpdateInstall(t *testing.T, r *Root) core.UpdateStatus {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for {
		status := r.updates.Snapshot().Status
		if status.Kind == core.UpdateError && status.Operation == core.UpdateInstall {
			if !strings.Contains(status.Message, "update payload missing") && !strings.Contains(status.Message, "unsupported platform for app update") {
				t.Fatalf("install did not reach the platform helper: %s", status.Message)
			}
			return status
		}
		if time.Now().After(deadline) {
			t.Fatalf("install did not reach the platform helper: %+v", status)
		}
		time.Sleep(10 * time.Millisecond)
	}
}

func TestDownloadedSidebarClickInstallsWhenIdle(t *testing.T) {
	r, _ := testRoot(t)
	stageDownloadedUpdate(t, r)
	r.onVersionClick()
	if r.confirming || r.confirmation.Open {
		t.Fatal("idle downloaded click opened a confirmation")
	}
	if r.update.Status.Kind != core.UpdateInstalling && r.update.Status.Kind != core.UpdateError {
		t.Fatalf("idle click did not install: %+v", r.update.Status)
	}
	waitUpdateInstall(t, r)
}

func TestDownloadedSidebarClickConfirmsOnlyWhileATaskIsBusy(t *testing.T) {
	r, _ := testRoot(t)
	stageDownloadedUpdate(t, r)
	r.job = &work{cancel: func() {}}
	r.onVersionClick()
	if !r.confirming || !r.confirmation.Open || r.confirmation.Kind != view.ConfirmInstallUpdate {
		t.Fatalf("busy click did not confirm: %+v", r.confirmation)
	}
	if r.updates.Snapshot().Status.Kind != core.UpdateDownloaded {
		t.Fatal("busy click installed before confirmation")
	}
	r.resolveConfirmation(r.confirmation.ID, false)
	if r.confirming || r.updates.Snapshot().Status.Kind != core.UpdateDownloaded {
		t.Fatal("declining the busy confirmation installed the update")
	}
	r.onVersionClick()
	if !r.confirming {
		t.Fatal("second busy click did not confirm")
	}
	r.resolveConfirmation(r.confirmation.ID, true)
	if r.update.Status.Kind != core.UpdateInstalling && r.update.Status.Kind != core.UpdateError {
		t.Fatalf("accepted busy click did not install: %+v", r.update.Status)
	}
	waitUpdateInstall(t, r)
}
