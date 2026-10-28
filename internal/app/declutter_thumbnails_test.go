package app

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"fmt"
	"hash/crc32"
	"image"
	"image/color"
	"image/png"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func previewFixture(t *testing.T) (string, []byte) {
	t.Helper()
	im := image.NewRGBA(image.Rect(0, 0, 7, 5))
	im.Set(2, 2, color.RGBA{R: 255, A: 255})
	var b bytes.Buffer
	if err := png.Encode(&b, im); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(t.TempDir(), "photo.PNG")
	if err := os.WriteFile(path, b.Bytes(), 0600); err != nil {
		t.Fatal(err)
	}
	return path, b.Bytes()
}

func TestDeclutterRealThumbnailPreservesPhotoSnapshot(t *testing.T) {
	path, data := previewFixture(t)
	groups := []core.PhotoGroup{{Photos: []core.PhotoItem{{Path: path, Size: uint64(len(data)), IsBestShot: true}, {Path: path, Selected: true}}}}
	before := append([]core.PhotoItem(nil), groups[0].Photos...)
	got := loadDeclutterThumbnails(context.Background(), groups)
	if len(got) != 1 || got[path] == nil {
		t.Fatal("no actual decoded bitmap", got)
	}
	if w, h := got[path].Size(); w != 7 || h != 5 {
		t.Fatal(w, h)
	}
	if !reflect.DeepEqual(groups[0].Photos, before) {
		t.Fatal("preview changed original selection/authority")
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if got := loadDeclutterThumbnails(ctx, groups); len(got) != 0 {
		t.Fatal("cancelled work decoded a preview")
	}
}

func TestDeclutterThumbnailRefusals(t *testing.T) {
	path, data := previewFixture(t)
	for _, test := range []struct {
		name string
		data []byte
		size uint64
	}{
		{"unsupported.heic", data, uint64(len(data))},
		{"metadata.png", data, declutterPreviewBytes + 1},
		{"actual.png", make([]byte, declutterPreviewBytes+1), 1},
		{"broken.gif", []byte("invalid image"), 13},
		{"bomb.png", oversizedPreviewPNG(data), 1},
	} {
		t.Run(test.name, func(t *testing.T) {
			p := filepath.Join(filepath.Dir(path), test.name)
			if err := os.WriteFile(p, test.data, 0600); err != nil {
				t.Fatal(err)
			}
			got := loadDeclutterThumbnails(context.Background(), []core.PhotoGroup{{Photos: []core.PhotoItem{{Path: p, Size: test.size}}}})
			if len(got) != 0 {
				t.Fatal("ineligible image accepted")
			}
		})
	}
}

func oversizedPreviewPNG(data []byte) []byte {
	return previewPNGDimensions(data, 100000, 100000)
}

func previewPNGDimensions(data []byte, w, h uint32) []byte {
	b := bytes.Clone(data)
	binary.BigEndian.PutUint32(b[16:20], w)
	binary.BigEndian.PutUint32(b[20:24], h)
	binary.BigEndian.PutUint32(b[29:33], crc32.ChecksumIEEE(b[12:29]))
	return b
}

func TestDeclutterThumbnailAggregateDecodeBudget(t *testing.T) {
	path, data := previewFixture(t)
	// Valid headers with insufficient pixel data still spend the decode budget.
	bad := previewPNGDimensions(data, 4000, 4000)
	config, _, err := image.DecodeConfig(bytes.NewReader(bad))
	if err != nil || config.Width*config.Height != declutterPreviewPixels {
		t.Fatal("fixture must reach the decode gate", config, err)
	}
	var photos []core.PhotoItem
	for i := range 2 {
		p := filepath.Join(filepath.Dir(path), fmt.Sprintf("bad-%d.png", i))
		if err := os.WriteFile(p, bad, 0600); err != nil {
			t.Fatal(err)
		}
		photos = append(photos, core.PhotoItem{Path: p})
	}
	photos = append(photos, core.PhotoItem{Path: path})
	if got := loadDeclutterThumbnails(context.Background(), []core.PhotoGroup{{Photos: photos}}); len(got) != 0 {
		t.Fatal("decode work exceeded aggregate pixel budget")
	}
}

func TestDeclutterThumbnailReadCancellation(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	r := declutterPreviewReader{ctx: ctx, r: bytes.NewReader([]byte("fixture"))}
	var b [1]byte
	if n, err := r.Read(b[:]); n != 1 || err != nil {
		t.Fatal(n, err)
	}
	cancel()
	if n, err := r.Read(b[:]); n != 0 || !errors.Is(err, context.Canceled) {
		t.Fatal("reader continued after cancellation", n, err)
	}
}

func TestDeclutterThumbnailAttemptBudget(t *testing.T) {
	path, data := previewFixture(t)
	var photos []core.PhotoItem
	for i := 0; i < declutterPreviewAttempts+2; i++ {
		p := filepath.Join(filepath.Dir(path), fmt.Sprintf("%d.png", i))
		if err := os.WriteFile(p, data, 0600); err != nil {
			t.Fatal(err)
		}
		photos = append(photos, core.PhotoItem{Path: p})
	}
	if got := loadDeclutterThumbnails(context.Background(), []core.PhotoGroup{{Photos: photos}}); len(got) != declutterPreviewAttempts {
		t.Fatal("attempt limit not enforced", len(got))
	}
}

func TestDeclutterKindFilterSourceCycle(t *testing.T) {
	r, _ := testRoot(t)
	r.declutter.LargeFiles = []core.LargeFileItem{{IconType: 0}, {IconType: 1}, {IconType: 2}, {IconType: 3}, {IconType: 4}}
	a := r.declutterActions()
	for _, want := range []int{0, 1, 2, 3, -1, 0} {
		a.CycleKindFilter()
		if want == -1 {
			if r.declutter.KindFilter != nil || len(r.declutter.FilteredLargeIndices) != 5 {
				t.Fatal("cycle did not reset")
			}
		} else if r.declutter.KindFilter == nil || *r.declutter.KindFilter != want || !reflect.DeepEqual(r.declutter.FilteredLargeIndices, []int{want}) {
			t.Fatal("wrong filter", r.declutter.KindFilter, r.declutter.FilteredLargeIndices)
		}
	}
}
