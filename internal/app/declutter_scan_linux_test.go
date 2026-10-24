package app

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
	"time"
)

// All scan roots are disposable fixtures, never the orb's real user profile.
func TestDeclutterScanPublishesRedirectAndThumbnailsAtomically(t *testing.T) {
	// The photo scanner intentionally ignores every path containing /tmp/.
	fixture, err := os.MkdirTemp(".", "declutter-fixture-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(fixture) })
	home, err := filepath.Abs(fixture)
	if err != nil {
		t.Fatal(err)
	}
	downloads, config := t.TempDir(), t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", config)
	if err := os.WriteFile(filepath.Join(config, "user-dirs.dirs"), []byte(`XDG_DOWNLOAD_DIR="`+downloads+`"`), 0600); err != nil {
		t.Fatal(err)
	}
	download := filepath.Join(downloads, "redirected.txt")
	if err := os.WriteFile(download, []byte("fixture download"), 0600); err != nil {
		t.Fatal(err)
	}
	_, data := previewFixture(t)
	data = append(data, bytes.Repeat([]byte{0}, 11000)...)
	var originals []string
	for _, name := range []string{"photo.png", "photo-copy.png"} {
		path := filepath.Join(home, name)
		if err := os.WriteFile(path, data, 0600); err != nil {
			t.Fatal(err)
		}
		originals = append(originals, path)
	}
	r, q := testRoot(t)
	r.scanDeclutter()
	var finish func()
	select {
	case finish = <-q:
	case <-time.After(5 * time.Second):
		t.Fatal("scan did not finish")
	}
	if r.declutter.Scanned || len(r.declutter.Photos) != 0 || len(r.declutter.Thumbnails) != 0 || len(r.declutterAuthority) != 0 {
		t.Fatal("worker published partial scan")
	}
	finish()
	if !r.declutter.Scanned || r.declutter.Scanning || len(r.declutter.Downloads) != 1 || r.declutter.Downloads[0].Path != download {
		t.Fatal("redirected downloads were not published", r.declutter.Downloads)
	}
	if len(r.declutter.Photos) != 1 || len(r.declutter.Photos[0].Photos) != 2 {
		t.Fatal("fixture photo group missing")
	}
	for _, path := range originals {
		bitmap := r.declutter.Thumbnails[path]
		target, ok := r.declutterAuthority[path]
		if bitmap == nil || !ok || target.Path != path || target.Identity == nil {
			t.Fatal("missing preview or original authority", path)
		}
		if w, h := bitmap.Size(); w != 7 || h != 5 {
			t.Fatal(w, h)
		}
	}
	// A cancellation after worker completion must retain the entire old snapshot.
	r.scanDeclutter()
	select {
	case finish = <-q:
	case <-time.After(5 * time.Second):
		t.Fatal("second scan did not finish")
	}
	r.Cancel()
	finish()
	if !r.declutter.Scanned || r.declutter.Scanning || len(r.declutter.Thumbnails) != 2 {
		t.Fatal("cancelled scan replaced snapshot")
	}
}
