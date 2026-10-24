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
