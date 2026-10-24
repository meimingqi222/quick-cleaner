package core

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestScanDirectory(t *testing.T) {
	root := t.TempDir()
	if err := os.MkdirAll(filepath.Join(root, "nested", "empty"), 0700); err != nil {
		t.Fatal(err)
	}
	for path, data := range map[string]string{"small": "abc", "nested/big": "1234567", "nested/other": "12"} {
		if err := os.WriteFile(filepath.Join(root, path), []byte(data), 0600); err != nil {
			t.Fatal(err)
		}
	}
	var last Usage
	result, err := ScanDirectory(context.Background(), root, func(u Usage) { last = u })
	if err != nil {
		t.Fatal(err)
	}
	if result.Bytes != 12 || result.Files != 3 || result.Skipped != 0 || last != result.Usage {
		t.Fatalf("incorrect totals: %+v, progress %+v", result, last)
	}
	if len(result.Entries) != 2 || result.Entries[0].Path != filepath.Join(root, "nested") || result.Entries[0].Bytes != 9 || result.Entries[0].Files != 2 || result.Entries[1].Bytes != 3 {
		t.Fatalf("incorrect child aggregation: %+v", result.Entries)
	}
}

func TestScanDirectorySkipsLinksAndRejectsLinkedAncestors(t *testing.T) {
	root, outside := t.TempDir(), t.TempDir()
	if err := os.WriteFile(filepath.Join(outside, "private"), []byte("not in scan"), 0600); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(root, "link")
	if err := os.Symlink(outside, link); err != nil {
		t.Skipf("symlink unavailable: %v", err)
	}
	result, err := ScanDirectory(context.Background(), root, nil)
	if err != nil || result.Bytes != 0 || result.Files != 0 || result.Skipped != 1 {
		t.Fatalf("followed a link: %+v, %v", result, err)
	}
	if _, err := ScanDirectory(context.Background(), link, nil); err == nil {
		t.Fatal("accepted a link root")
	}
	if err := os.Mkdir(filepath.Join(outside, "child"), 0700); err != nil {
		t.Fatal(err)
	}
	if _, err := ScanDirectory(context.Background(), filepath.Join(link, "child"), nil); err == nil {
		t.Fatal("accepted a linked ancestor")
	}
}

func TestScanDirectoryCancellation(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	_, err := ScanDirectory(ctx, t.TempDir(), nil)
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("want cancellation, got %v", err)
	}
}
