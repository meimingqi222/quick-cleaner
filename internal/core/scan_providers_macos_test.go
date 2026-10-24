package core

import (
	"os"
	"path/filepath"
	"runtime"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func TestOldIdeVersionsKeepNewestAndUnparsed(t *testing.T) {
	products := []string{"IntelliJ IDEA", "PyCharm", "DataGrip", "RustRover"}
	names := []string{"IntelliJIdea2025.2", "IntelliJ IDEA 2026.2.1", "Daemon", "Toolbox", "PyCharmEdu", "DataGrip2025.1", "rustrover2026.1"}
	old := selectOldIdeVersions(products, names)
	if len(old) != 1 || old[0].name != "IntelliJIdea2025.2" || old[0].family != "IntelliJ IDEA" || old[0].year != 2025 || old[0].minor != 2 {
		t.Fatalf("only the superseded IDEA directory should be listed: %+v", old)
	}
	if _, _, _, ok := parseJetBrainsVersion(products, "PyCharmLatest"); ok {
		t.Fatal("a prefix hit without a numeric version parsed")
	}
}

func TestOldIdeTargetsUseRuleRoots(t *testing.T) {
	rules, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	home := t.TempDir()
	base := filepath.Join(home, "Library", "Application Support", "JetBrains")
	for _, name := range []string{"IntelliJIdea2024.1", "IntelliJIdea2026.2", "Daemon"} {
		if err := os.MkdirAll(filepath.Join(base, name), 0o700); err != nil {
			t.Fatal(err)
		}
	}
	targets := oldIdeDataTargets(rules, home)
	if len(targets) != 1 || targets[0].Category != OldIdeData || targets[0].Operation.Kind != OpTree || targets[0].Recommended || targets[0].Disposal != RecycleBin {
		t.Fatalf("old IDE target diverges from the rule: %+v", targets)
	}
	if filepath.Base(targets[0].Path) != "IntelliJIdea2024.1" {
		t.Fatal("newest or shared directory listed", targets[0].Path)
	}
}

func TestVolumeTrashOnlyMatchesCurrentUser(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("/Volumes trash fixture; macOS product path")
	}
	own := filepath.Join("/Volumes/Backup/.Trashes", platform.CurrentUID())
	if !platform.IsVolumeTrash(own) {
		t.Fatal("current user's volume trash rejected", own)
	}
	if platform.IsVolumeTrash("/Volumes/Backup/.Trashes/not-the-user") || platform.IsVolumeTrash(filepath.Join("/tmp/.Trashes", platform.CurrentUID())) {
		t.Fatal("another user or a non-volume path matched")
	}
}
