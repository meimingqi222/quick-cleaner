package platform

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestAppActivityShortcutFixture(t *testing.T) {
	base := t.TempDir()
	for _, name := range []string{"PotPlayer-64 bit.LNK", "one/two/Visible.lnk", "one/two/three/Hidden.lnk", "Ignored.txt"} {
		path := filepath.Join(base, name)
		if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, nil, 0600); err != nil {
			t.Fatal(err)
		}
		if err := os.Chtimes(path, time.Unix(1700000000, 0), time.Unix(1600000000, 0)); err != nil {
			t.Fatal(err)
		}
	}
	got := appScanShortcutActivity(context.Background(), []string{base})
	if got["potplayer"] != 1700000000 || got["potplayer-64 bit"] != 1700000000 || got["visible"] != 1700000000 {
		t.Fatalf("shortcut atime: %v", got)
	}
	if got["hidden"] != 0 || got["ignored"] != 0 || got["bit"] != 0 {
		t.Fatalf("filter/depth: %v", got)
	}
}

func TestAppActivityCancelledReaders(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if len(ReadWindowsAppRegistrations(ctx)) != 0 {
		t.Fatal("cancelled registry read")
	}
	if len(ReadWindowsAppActivity(ctx).byToken) != 0 {
		t.Fatal("cancelled activity read")
	}
	if len(ReadWindowsShortcutActivity(ctx)) != 0 {
		t.Fatal("cancelled shortcut read")
	}
}
