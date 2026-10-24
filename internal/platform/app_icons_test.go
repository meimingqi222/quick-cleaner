package platform

import (
	"bytes"
	"encoding/binary"
	"image"
	"image/color"
	"image/png"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"sync"
	"testing"
)

func iconTestPNG(t *testing.T, w, h int) []byte {
	t.Helper()
	im := image.NewNRGBA(image.Rect(0, 0, w, h))
	for y := 0; y < h; y++ {
		for x := 0; x < w; x++ {
			im.SetNRGBA(x, y, color.NRGBA{100, 50, 200, 128})
		}
	}
	return iconEncode(im)
}

func iconTestICNS(chunks ...[]byte) []byte {
	b := []byte("icns\x00\x00\x00\x00")
	for _, c := range chunks {
		b = append(b, c...)
	}
	binary.BigEndian.PutUint32(b[4:8], uint32(len(b)))
	return b
}
func iconTestChunk(kind string, payload []byte) []byte {
	b := append([]byte(kind), 0, 0, 0, 0)
	binary.BigEndian.PutUint32(b[4:8], uint32(len(payload)+8))
	return append(b, payload...)
}

func TestAppIconsICNSSelectionAndBounds(t *testing.T) {
	a, b, c := []byte("\x89PNG128"), []byte("\x89PNG64first"), []byte("\x89PNG64second")
	fixture := iconTestICNS(iconTestChunk("ic07", a), iconTestChunk("ic12", b), iconTestChunk("icp6", c))
	if got := iconICNS(fixture); !bytes.Equal(got, b) {
		t.Fatalf("64px first winner: %q", got)
	}
	for _, data := range [][]byte{nil, []byte("icns"), iconTestICNS(iconTestChunk("junk", b)), iconTestICNS([]byte("ic12\x00\x00\x00\x07")), iconTestICNS([]byte("ic12\xff\xff\xff\xff"))} {
		if iconICNS(data) != nil {
			t.Fatalf("malformed/unknown accepted: %x", data)
		}
	}
	validThenBad := iconTestICNS(iconTestChunk("ic07", a), []byte("ic12\xff\xff\xff\xff"))
	if !bytes.Equal(iconICNS(validThenBad), a) {
		t.Fatal("lost prior valid chunk")
	}
	declared := append([]byte(nil), fixture...)
	binary.BigEndian.PutUint32(declared[4:8], uint32(8+8+len(a)))
	if !bytes.Equal(iconICNS(declared), a) {
		t.Fatal("read beyond declared ICNS length")
	}
	for n := 0; n < len(fixture); n++ {
		_ = iconICNS(fixture[:n])
	}
}

func iconTestBPlist(value []byte) []byte {
	key := append([]byte{0x5f, 0x10, 16}, []byte("CFBundleIconFile")...)
	b := append([]byte("bplist00"), 0xd1, 1, 2)
	keyOff := len(b)
	b = append(b, key...)
	valueOff := len(b)
	b = append(b, value...)
	table := len(b)
	b = append(b, 8, byte(keyOff), byte(valueOff))
	trailer := make([]byte, 32)
	trailer[6] = 1
	trailer[7] = 1
	binary.BigEndian.PutUint64(trailer[8:16], 3)
	binary.BigEndian.PutUint64(trailer[24:32], uint64(table))
	return append(b, trailer...)
}

func TestAppIconsPlistBinaryAndXML(t *testing.T) {
	for _, tc := range []struct {
		data []byte
		want string
	}{
		{[]byte("<key>CFBundleIconFile</key>\n <string> Main </string>"), "Main"},
		{[]byte("<key>CFBundleIconFile</key><true/><string>Wrong</string>"), ""},
		{iconTestBPlist(append([]byte{0x54}, []byte("Main")...)), "Main"},
		{iconTestBPlist([]byte{0x62, 0x4e, 0x2d, 0x65, 0x87}), "中文"},
		{iconTestBPlist([]byte{0x61, 0xd8, 0x00}), ""},
		{iconTestBPlist([]byte{0x5f, 0x1f}), ""},
	} {
		if got := iconPlist(tc.data, "CFBundleIconFile"); got != tc.want {
			t.Fatalf("plist got %q want %q", got, tc.want)
		}
	}
	valid := iconTestBPlist([]byte{0x51, 'X'})
	for n := 0; n < len(valid); n++ {
		_ = iconPlist(valid[:n], "CFBundleIconFile")
	}
	for _, at := range []int{len(valid) - 26, len(valid) - 25, len(valid) - 24, len(valid) - 16, len(valid) - 8} {
		b := append([]byte(nil), valid...)
		b[at] = 255
		if iconPlist(b, "CFBundleIconFile") != "" {
			t.Fatalf("bad trailer accepted at %d", at)
		}
	}
}

func TestAppIconsResizeAndAlpha(t *testing.T) {
	for _, size := range []int{32, 256, 257} {
		src := iconTestPNG(t, size, 31)
		for _, target := range []int{48, 64} {
			got := iconResize(src, target, target == 64)
			cfg, e := png.DecodeConfig(bytes.NewReader(got))
			if e != nil {
				t.Fatal(e)
			}
			wantW, wantH := size, 31
			if size > 256 {
				wantW, wantH = target, target
			}
			if cfg.Width != wantW || cfg.Height != wantH {
				t.Fatalf("dimensions %v, want %d x %d", cfg, wantW, wantH)
			}
			if target == 64 && size <= 256 && !bytes.Equal(src, got) {
				t.Fatal("small PNG changed")
			}
		}
	}
	bad := []byte("\x89PNGbroken")
	if !bytes.Equal(iconMacDownscale(bad), bad) || iconResize(bad, 48, false) != nil {
		t.Fatal("invalid image fallback")
	}
	if got := iconBGRA([]byte{1, 2, 3, 0, 4, 5, 6, 0}); !bytes.Equal(got, []byte{3, 2, 1, 255, 6, 5, 4, 255}) {
		t.Fatal(got)
	}
	if got := iconBGRA([]byte{1, 2, 3, 0, 4, 5, 6, 100}); !bytes.Equal(got, []byte{3, 2, 1, 0, 6, 5, 4, 100}) {
		t.Fatal(got)
	}
	hidden := image.NewNRGBA(image.Rect(0, 0, 300, 1))
	for x := 0; x < 300; x++ {
		hidden.SetNRGBA(x, 0, color.NRGBA{R: 200})
	}
	b := iconResize(iconEncode(hidden), 48, false)
	decoded, e := png.Decode(bytes.NewReader(b))
	if e != nil {
		t.Fatal(e)
	}
	if c := color.NRGBAModel.Convert(decoded.At(0, 0)).(color.NRGBA); c.R < 199 || c.A != 0 {
		t.Fatalf("straight-alpha RGB was lost: %v", c)
	}
}

func TestAppIconsRealBundleFallbackAndParallel(t *testing.T) {
	app := filepath.Join(t.TempDir(), "Fixture.app")
	r := filepath.Join(app, "Contents", "Resources")
	if e := os.MkdirAll(r, 0700); e != nil {
		t.Fatal(e)
	}
	write := func(p string, b []byte) {
		t.Helper()
		if e := os.WriteFile(p, b, 0600); e != nil {
			t.Fatal(e)
		}
	}
	good := iconTestPNG(t, 64, 64)
	other := iconTestPNG(t, 32, 32)
	write(filepath.Join(r, "document.icns"), iconTestICNS(iconTestChunk("ic12", other)))
	write(filepath.Join(r, "Unique.icns"), iconTestICNS(iconTestChunk("ic12", good)))
	if !bytes.Equal(iconMacBundle(app), good) {
		t.Fatal("document icon won over unique app icon")
	}
	write(filepath.Join(r, "Another.icns"), iconTestICNS(iconTestChunk("ic12", other)))
	if iconMacBundle(app) != nil {
		t.Fatal("ambiguous icon guessed")
	}
	write(filepath.Join(app, "Contents", "Info.plist"), []byte("<key>CFBundleIconFile</key><string>Unique</string>"))
	if !bytes.Equal(iconMacBundle(app), good) {
		t.Fatal("plist icon not preferred")
	}
	write(filepath.Join(r, "Unique.icns"), []byte("malformed"))
	write(filepath.Join(r, "AppIcon.icns"), iconTestICNS(iconTestChunk("ic12", other)))
	if iconMacBundle(app) != nil {
		t.Fatal("invalid selected icon should not try other ICNS")
	}
	write(filepath.Join(r, "icon.png"), good)
	if !bytes.Equal(iconMacBundle(app), good) {
		t.Fatal("common PNG fallback missing")
	}
	if runtime.GOOS == "darwin" && !bytes.Equal(AppIconFromBundle(app), good) {
		t.Fatal("public bundle API failed fixture")
	}
	var wg sync.WaitGroup
	for i := 0; i < 16; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for j := 0; j < 10; j++ {
				if !bytes.Equal(iconMacBundle(app), good) {
					t.Error("parallel acquisition mismatch")
				}
			}
		}()
	}
	wg.Wait()
}

func TestAppIconsWindowsCandidatesAndImages(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "Main")
	if e := os.MkdirAll(filepath.Join(dir, "bin"), 0700); e != nil {
		t.Fatal(e)
	}
	names := []string{"Main.exe", "Main.ico", "icon.ico", "bin/Main.exe", "ZMain.exe", "Alpha.exe", "Zoo.exe", "uninstall.exe", "helper.exe", "setup.exe", "Updater.exe"}
	for _, name := range names {
		if e := os.WriteFile(filepath.Join(dir, name), []byte("fixture"), 0600); e != nil {
			t.Fatal(e)
		}
	}
	var got []string
	for _, p := range iconWindowsCandidates(dir) {
		rel, _ := filepath.Rel(dir, p)
		got = append(got, filepath.ToSlash(rel))
	}
	want := []string{"Main.exe", "Main.ico", "icon.ico", "bin/Main.exe", "ZMain.exe", "Alpha.exe", "Zoo.exe"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("got %v want %v", got, want)
	}
	path := filepath.Join(dir, "test.PNG")
	source := iconTestPNG(t, 512, 512)
	if e := os.WriteFile(path, source, 0600); e != nil {
		t.Fatal(e)
	}
	b := iconWindowsImage(path)
	cfg, e := png.DecodeConfig(bytes.NewReader(b))
	if e != nil || cfg.Width != 48 || cfg.Height != 48 {
		t.Fatalf("image resize %v %v", cfg, e)
	}
	if runtime.GOOS == "windows" {
		if AppIconPNG(path) == nil {
			t.Fatal("public image API failed fixture")
		}
		ico, err := os.ReadFile(filepath.Join("..", "..", "assets", "icon.ico"))
		if err != nil {
			t.Fatal(err)
		}
		isolated := t.TempDir()
		named := filepath.Join(isolated, filepath.Base(isolated)+".ico")
		if err := os.WriteFile(named, ico, 0600); err != nil {
			t.Fatal(err)
		}
		t.Setenv("QC_APP_ICON_FIXTURE", isolated)
		for _, input := range []string{named, isolated + `\`, `%QC_APP_ICON_FIXTURE%\`} {
			data := AppIconPNG(input)
			if _, err := png.Decode(bytes.NewReader(data)); err != nil {
				t.Fatalf("native ICO fixture %q: %v", input, err)
			}
		}
	}
	for _, tc := range []struct{ raw, want string }{{`"C:\Program Files\A\a.exe",-1`, `C:\Program Files\A\a.exe`}, {`"C:\Program Files\A\a.exe,-1"`, `C:\Program Files\A\a.exe`}, {`C:\A, B\a.exe,0`, `C:\A, B\a.exe`}, {`C:\A, B\a.exe`, `C:\A, B\a.exe`}} {
		if got := iconDisplayPath(tc.raw); got != tc.want {
			t.Fatalf("path %q != %q", got, tc.want)
		}
	}
}

func FuzzAppIconsBinaryBounds(f *testing.F) {
	f.Add(iconTestBPlist([]byte{0x51, 'X'}))
	f.Add(iconTestICNS(iconTestChunk("ic12", []byte("\x89PNG"))))
	f.Fuzz(func(t *testing.T, data []byte) { _ = iconICNS(data); _ = iconPlist(data, "CFBundleIconFile") })
}
