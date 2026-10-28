package core

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestWindowsResidualPublicVerificationKeepsFrozenIdentity(t *testing.T) {
	root := t.TempDir()
	path := filepath.Join(root, "residual")
	if err := os.WriteFile(path, []byte("before"), 0600); err != nil {
		t.Fatal(err)
	}
	item := CertainResidual(ResidualKind{Type: ResidualFile, Path: path, Bytes: 6}, ResidualOther)
	if err := os.Rename(path, path+".old"); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte("after!"), 0600); err != nil {
		t.Fatal(err)
	}
	items, err := VerifyAppResiduals(context.Background(), []ResidualItem{item})
	if err != nil || len(items) != 1 {
		t.Fatalf("verify: %v %v", items, err)
	}
	if items[0].Identity != item.Identity || items[0].Identity.Recheck(path) {
		t.Fatal("verification renewed old authority")
	}
	if err := os.Remove(path); err != nil {
		t.Fatal(err)
	}
	items, err = VerifyAppResiduals(context.Background(), []ResidualItem{item})
	if err != nil || len(items) != 0 {
		t.Fatalf("absent target retained: %v %v", items, err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := ScanAppResiduals(ctx, InstalledApp{}); !errors.Is(err, context.Canceled) {
		t.Fatal("public scan cancellation")
	}
}

// 应用名带版本号（Windows 的 DisplayName 常见写法）时，短目录名仍必须匹配上。
// 少了这一步，%APPDATA%\MiniMax 这种真实残留会因为 7*2 < 15 被拒，界面显示 0 项。
func TestVersionedDisplayNameStillMatchesShortAppDataFolder(t *testing.T) {
	snapshot, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	c := newWindowsResidualContext(InstalledApp{Name: "MiniMax Code 3.0.74", Publisher: "MiniMax"}, snapshot)
	if !c.matchable() {
		t.Fatalf("versioned display name is not matchable: name=%q token=%q", c.name, c.token)
	}
	if !c.matches("MiniMax") {
		t.Fatalf("short app data folder rejected: name=%q", c.name)
	}
	if c.matches("MiniMax Agent") {
		t.Fatal("a different product matched the versioned name")
	}
}

func TestResidualShortNameDropsTrailingVersion(t *testing.T) {
	for _, tc := range []struct{ name, want string }{
		{"MiniMax Code 3.0.74", "MiniMax Code"},
		{"7-Zip 24.08", "7-Zip"},
		{"Foo v1.2", "Foo"},
		{"MiniMax Code", "MiniMax Code"},
		{"Microsoft Visual C++ 2015 Redistributable", "Microsoft Visual C++ 2015 Redistributable"},
		{"App 2", "App 2"},
	} {
		if got := windowsResidualShortName(tc.name); got != tc.want {
			t.Fatalf("short name of %q = %q, want %q", tc.name, got, tc.want)
		}
	}
}
