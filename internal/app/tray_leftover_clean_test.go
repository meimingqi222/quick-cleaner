package app

import (
	"context"
	"os"
	"path/filepath"
	"runtime"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func TestLeftoverCleanKeepsUnselectedAndManualItems(t *testing.T) {
	if runtime.GOOS != "darwin" && runtime.GOOS != "windows" {
		t.Skip("native residual verification is only available on macOS and Windows")
	}
	r, q := testRoot(t)
	gone := core.CertainResidual(core.ResidualKind{Type: core.ResidualFile, Path: filepath.Join(t.TempDir(), "gone")}, core.ResidualCacheDir)
	keepPath := filepath.Join(t.TempDir(), "keep")
	if err := os.WriteFile(keepPath, []byte("keep"), 0600); err != nil {
		t.Fatal(err)
	}
	keep := core.PossibleResidual(core.ResidualKind{Type: core.ResidualFile, Path: keepPath, Bytes: 4}, core.ResidualOther)
	manual := core.CertainResidual(core.ResidualKind{Type: core.ResidualSystemExtension}, core.ResidualOther)
	r.trashLeftovers = []trayLeftover{{id: "example", app: core.InstalledApp{ID: "example", Name: "Example"}, result: core.ResidualScanResult{Items: []core.ResidualItem{gone, keep, manual}}, selected: map[int]bool{0: true}}}
	r.residualAuthority = func(context.Context, core.InstalledApp, []core.ResidualItem) bool { return true }
	r.requestLeftoverClean("example")
	dispatchNext(t, q)
	if len(r.trashLeftovers) != 1 || len(r.trashLeftovers[0].result.Items) != 2 {
		t.Fatalf("unselected/manual residuals disappeared: %+v", r.trashLeftovers)
	}
	row := r.trashLeftovers[0]
	if row.cleaning || row.result.Items[0].Kind.Path != keepPath || row.result.Items[1].Kind.Type != core.ResidualSystemExtension {
		t.Fatalf("wrong remaining row: %+v", row)
	}
	if row.selected[0] || row.selected[1] {
		t.Fatal("cleanup preselected a previously unselected item")
	}
	if data, err := os.ReadFile(keepPath); err != nil || string(data) != "keep" {
		t.Fatalf("unselected file changed: %q %v", data, err)
	}
}
