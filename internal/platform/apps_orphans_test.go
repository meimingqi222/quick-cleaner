package platform

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestMacOrphanRegistrationEvidenceUsesActualCanonicalBundle(t *testing.T) {
	root := t.TempDir()
	trash := filepath.Join(root, ".Trash")
	bundle := func(relative, id string) string {
		path := filepath.Join(root, filepath.FromSlash(relative))
		if err := os.MkdirAll(filepath.Join(path, "Contents"), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(path, "Contents", "Info.plist"), []byte(id), 0600); err != nil {
			t.Fatal(err)
		}
		return path
	}
	live := bundle("hidden/Browser.app", "org.webkit.Playwright")
	removed := bundle(".Trash/Browser.app", "org.webkit.Playwright")
	wrong := bundle("other/Wrong.app", "com.other.Product")
	read := func(path string) *string {
		data, err := os.ReadFile(path)
		if err != nil {
			return nil
		}
		id := string(data)
		return &id
	}
	for _, tc := range []struct {
		name, path           string
		known, live, removed bool
	}{
		{"live", live, true, true, false}, {"trashed", removed, true, false, true}, {"stale", filepath.Join(root, "missing.app"), true, false, false}, {"mismatched", wrong, false, false, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			installed := map[string]struct{}{}
			gone, known := macSupplementOwners(context.Background(), []string{"org.webkit.playwright"}, installed, trash, func(id string) ([]string, bool) {
				if id == "org.webkit.playwright" {
					return []string{tc.path}, true
				}
				return nil, true
			}, read)
			_, isLive := installed["org.webkit.playwright"]
			_, isGone := gone["org.webkit.playwright"]
			if known != tc.known || isLive != tc.live || isGone != tc.removed {
				t.Fatalf("known=%v live=%v removed=%v", known, isLive, isGone)
			}
		})
	}
	if ids, known := macSupplementOwners(context.Background(), []string{"org.webkit.playwright"}, map[string]struct{}{}, trash, func(string) ([]string, bool) { return nil, false }, read); known || ids != nil {
		t.Fatal("unavailable registration accepted")
	}
	ids := macTrashedBundleIDs(context.Background(), root, read)
	if _, ok := ids["org.webkit.playwright"]; !ok {
		t.Fatal("exact trashed identity not found")
	}
	alias := filepath.Join(trash, "Alias.app")
	if err := os.Symlink(live, alias); err != nil {
		t.Skipf("symlink unavailable: %v", err)
	}
	installed := map[string]struct{}{}
	ids, known := macSupplementOwners(context.Background(), []string{"org.webkit.playwright"}, installed, trash, func(id string) ([]string, bool) {
		if id == "org.webkit.playwright" {
			return []string{alias}, true
		}
		return nil, true
	}, read)
	_, isLive := installed["org.webkit.playwright"]
	if !known || !isLive || len(ids) != 0 {
		t.Fatal("trash alias to live bundle treated as removed", ids, installed, known)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, known := macSupplementOwners(ctx, []string{"org.webkit.playwright"}, installed, trash, func(string) ([]string, bool) { t.Fatal("cancelled lookup executed"); return nil, true }, read); known {
		t.Fatal("cancelled round accepted")
	}
}
