package platform

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"testing"
)

func TestAppShortcutEvidenceLimits(t *testing.T) {
	root := t.TempDir()
	for _, name := range []string{"top.lnk", "one/two/three/four.LNK", "one/two/three/four/hidden.lnk", "ignored.exe"} {
		path := filepath.Join(root, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, nil, 0600); err != nil {
			t.Fatal(err)
		}
	}
	read := func(path string) (string, bool) { return "literal-target", true }
	got := readAppShortcuts(context.Background(), []string{root}, read)
	if len(got) != 2 {
		t.Fatalf("depth/extension boundary: %v", got)
	}
	ctx, cancel := context.WithCancel(context.Background())
	got = readAppShortcuts(ctx, []string{root}, func(path string) (string, bool) { cancel(); return "target", true })
	if len(got) != 0 {
		t.Fatal("cancelled discovery leaked partial inventory")
	}
	bounded := t.TempDir()
	for i := range 4001 {
		if err := os.WriteFile(filepath.Join(bounded, fmt.Sprintf("%04d.lnk", i)), nil, 0600); err != nil {
			t.Fatal(err)
		}
	}
	if got := readAppShortcuts(context.Background(), []string{bounded}, read); len(got) != 4000 {
		t.Fatalf("entry budget=%d", len(got))
	}
}
