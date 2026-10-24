package core_test

import (
	"archive/zip"
	"bytes"
	"context"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func TestUpdaterVersionAndState(t *testing.T) {
	for _, tc := range []struct {
		next, current string
		newer         bool
	}{
		{"0.0.10", "0.0.9", true}, {" VVv1.2 ", "1.2.0", false}, {"1.2.3-beta.1", "1.2.3", false},
		{"1.nope.1", "1.0.0", true}, {"1.18446744073709551616.0", "1.0.0", false},
		{"1.0.18446744073709551616", "1.0.0", false}, {"nightly", "1", true}, {"1", "nightly", true}, {"nightly", "nightly", false},
		{"+1.+2.3more", "1.2.3", false}, {"1.2.3.99", "1.2.3", false}, {"1.2.+3", "1.2.0", false},
	} {
		if got := core.IsVersionNewer(tc.next, tc.current); got != tc.newer {
			t.Errorf("%q > %q = %v", tc.next, tc.current, got)
		}
	}
	v := " v1.2 "
	if !core.IsSkipped("1.2", &v) || core.IsSkipped("1.2.0", &v) || core.IsSkipped("1.2", nil) {
		t.Fatal("skip normalization")
	}
	for _, p := range []string{`C:\code\target\debug\app.exe`, "/tmp/TARGET/release/app", "/tmp/target/debugger/app"} {
		if !core.LooksLikeDevBuild(p) {
			t.Fatal(p)
		}
	}
	if core.LooksLikeDevBuild("/Applications/QuickCleaner.app/Contents/MacOS/app") {
		t.Fatal("packaged app")
	}
	for k := core.UpdateIdle; k <= core.UpdateError; k++ {
		s := core.UpdateStatus{Kind: k, CurrentVersion: "1", LatestVersion: "2"}
		if got := s.WantsAttention(); got != (k >= core.UpdateAvailable) {
			t.Fatalf("attention %v", k)
		}
		if got := s.LatestVersionValue(); (got != nil) != (k >= core.UpdateAvailable && k <= core.UpdateInstalling) {
			t.Fatalf("latest %v", k)
		}
	}
	if core.FirstCheckDelayMS != 10000 || core.CheckIntervalMS != 14400000 {
		t.Fatal("cadence")
	}
}

func TestUpdaterRelease(t *testing.T) {
	r := core.GithubRelease{TagName: "v2.0.0", HTMLURL: "release", Assets: []core.GithubAsset{
		{Name: "quick-cleaner-macos-aarch64.zip", BrowserDownloadURL: "arm"},
		{Name: "quick-cleaner-macos-x86_64.zip", BrowserDownloadURL: "intel"},
		{Name: "quick-cleaner-macos-universal.zip", BrowserDownloadURL: "universal"},
	}}
	for _, target := range []core.UpdateTarget{core.MacosAarch64, core.MacosX86_64} {
		s, err := core.EvaluateRelease("1", &r, core.CandidateAssetNames(target))
		if err != nil || s.Kind != core.UpdateAvailable || s.AssetURL != "universal" || s.ChecksumURL != "universal.sha256" || s.LatestVersion != "2.0.0" {
			t.Fatalf("%+v %v", s, err)
		}
	}
	if core.SelectAsset(&r, core.MacosX86_64).BrowserDownloadURL != "intel" {
		t.Fatal("exact selection uses architecture, not universal")
	}
	r.Assets = r.Assets[:1]
	if _, err := core.EvaluateRelease("1", &r, core.CandidateAssetNames(core.MacosX86_64)); err == nil || err.Error() != `no matching asset for this platform among ["quick-cleaner-macos-universal.zip", "quick-cleaner-macos-x86_64.zip"]` {
		t.Fatal(err)
	}
	for _, mode := range []string{"draft", "pre", "same"} {
		r.Draft = mode == "draft"
		r.Prerelease = mode == "pre"
		if mode == "same" {
			r.TagName = "v1"
		}
		s, e := core.EvaluateRelease("1", &r, nil)
		if e != nil || s.Kind != core.UpdateNotAvailable {
			t.Fatalf("%s %+v %v", mode, s, e)
		}
	}
	for _, body := range []string{`{}`, `null`, `{"tag_name":"v1","html_url":"u","prerelease":false,"draft":false,"assets":null}`, `{"tag_name":"v1","tag_name":"v2","html_url":"u","prerelease":false,"draft":false,"assets":[]}`, `{"tag_name":"v1","html_url":"u","prerelease":false,"draft":false,"assets":[{"name":"x","browser_download_url":"u","size":-1}]}`} {
		if err := json.Unmarshal([]byte(body), &r); err == nil {
			t.Fatalf("accepted %s", body)
		}
	}
}

type updaterFixtureEntry struct {
	name, body string
	mode       os.FileMode
}

func updaterZipFixture(t *testing.T, entries ...updaterFixtureEntry) []byte {
	t.Helper()
	var b bytes.Buffer
	w := zip.NewWriter(&b)
	for _, e := range entries {
		h := &zip.FileHeader{Name: e.name, Method: zip.Deflate}
		if e.mode != 0 {
			h.SetMode(e.mode)
		}
		f, err := w.CreateHeader(h)
		if err != nil {
			t.Fatal(err)
		}
		if strings.HasSuffix(e.name, "/") {
			continue
		}
		if _, err = io.WriteString(f, e.body); err != nil {
			t.Fatal(err)
		}
	}
	if err := w.Close(); err != nil {
		t.Fatal(err)
	}
	return b.Bytes()
}
func updaterExtractFixture(t *testing.T, data []byte, limits core.ExtractLimits) (string, error) {
	t.Helper()
	root := t.TempDir()
	archive := filepath.Join(root, "a.zip")
	if err := os.WriteFile(archive, data, 0600); err != nil {
		t.Fatal(err)
	}
	staging := filepath.Join(root, "staging")
	if err := os.Mkdir(staging, 0700); err != nil {
		t.Fatal(err)
	}
	return staging, core.ExtractZip(archive, staging, limits)
}

func TestUpdaterArchivePrevalidation(t *testing.T) {
	for _, tc := range []struct{ name, reason string }{
		{"../escape", "path escapes destination"}, {`dir\..\escape`, "path escapes destination"}, {"/abs", "absolute path"}, {`C:\abs`, "absolute path (drive prefix)"},
		{"a//b", "empty path component"}, {"a/./b", "relative path component"}, {"a//", "empty path component"}, {"", "empty name"},
		{"a/b.", "path component ends with a dot or space"}, {"a/b ", "path component ends with a dot or space"}, {"a/CON.txt", "reserved device name"},
		{"a/LPT9", "reserved device name"}, {"a/COM1.dat", "reserved device name"}, {"a/has:stream", "illegal character in path"}, {"a/has\x00nul", "illegal character in path"},
	} {
		t.Run(fmt.Sprintf("%q", tc.name), func(t *testing.T) {
			staging, err := updaterExtractFixture(t, updaterZipFixture(t, updaterFixtureEntry{"safe", "ok", 0}, updaterFixtureEntry{tc.name, "bad", 0}), core.DefaultExtractLimits())
			if err == nil || !strings.Contains(err.Error(), tc.reason) {
				t.Fatalf("%v want %s", err, tc.reason)
			}
			entries, _ := os.ReadDir(staging)
			if len(entries) != 0 {
				t.Fatal("wrote before prevalidation")
			}
		})
	}
	for _, entries := range [][]updaterFixtureEntry{
		{{"a", "1", 0}, {"A", "2", 0}}, {{"dir/", "", 0}, {"DIR/", "", 0}}, {{"a", "1", 0}, {"a/b", "2", 0}},
	} {
		staging, err := updaterExtractFixture(t, updaterZipFixture(t, entries...), core.DefaultExtractLimits())
		if err == nil {
			t.Fatal("collision accepted")
		}
		e, _ := os.ReadDir(staging)
		if len(e) != 0 {
			t.Fatal("partial prevalidation")
		}
	}
	data := updaterZipFixture(t, updaterFixtureEntry{"a", "1234", 0}, updaterFixtureEntry{"b", "5678", 0})
	for _, limits := range []core.ExtractLimits{{1, 100}, {100, 7}, {0, 0}} {
		staging, err := updaterExtractFixture(t, data, limits)
		if err == nil {
			t.Fatal("limit accepted")
		}
		e, _ := os.ReadDir(staging)
		if len(e) != 0 {
			t.Fatal("limit wrote files")
		}
	}
	if _, err := updaterExtractFixture(t, data, core.ExtractLimits{2, 8}); err != nil {
		t.Fatal(err)
	}
	// Lie only in central-directory sizes; runtime decompression still may not
	// write an unbounded stream, even when headers understate it.
	for start := 0; ; {
		i := bytes.Index(data[start:], []byte{'P', 'K', 1, 2})
		if i < 0 {
			break
		}
		i += start
		binary.LittleEndian.PutUint32(data[i+24:i+28], 1)
		start = i + 46
	}
	staging, err := updaterExtractFixture(t, data, core.ExtractLimits{10, 2})
	if err == nil {
		t.Fatal("forged size accepted")
	}
	var actual int64
	_ = filepath.Walk(staging, func(_ string, m os.FileInfo, e error) error {
		if e == nil && m.Mode().IsRegular() {
			actual += m.Size()
		}
		return nil
	})
	if actual > 3 {
		t.Fatalf("unbounded writes %d", actual)
	}
}

func TestUpdaterArchiveUnicodeCollisionParity(t *testing.T) {
	for _, tc := range []struct {
		name, first, second string
		collision           bool
	}{
		{"final sigma", "ΟΣ", "ος", true},
		{"dotted capital I", "İ.txt", "i.txt", false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			stage, err := updaterExtractFixture(t, updaterZipFixture(t,
				updaterFixtureEntry{tc.first, "first", 0}, updaterFixtureEntry{tc.second, "second", 0}), core.DefaultExtractLimits())
			if (err != nil) != tc.collision {
				t.Fatalf("collision=%v: %v", tc.collision, err)
			}
			if tc.collision {
				entries, readErr := os.ReadDir(stage)
				if readErr != nil || len(entries) != 0 {
					t.Fatal("collision wrote files before rejection", entries, readErr)
				}
			} else {
				for path, want := range map[string]string{tc.first: "first", tc.second: "second"} {
					data, readErr := os.ReadFile(filepath.Join(stage, path))
					if readErr != nil || string(data) != want {
						t.Fatal(path, string(data), readErr)
					}
				}
			}
		})
	}
}

func TestUpdaterArchiveSymlinks(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("upstream forbids all archive symlinks on Windows")
	}
	link := os.ModeSymlink | 0777
	good := []updaterFixtureEntry{{"QuickCleaner.app/Contents/Info.plist", "plist", 0644}, {"QuickCleaner.app/Contents/MacOS/quick-cleaner", "exe", 0755}, {"QuickCleaner.app/Versions/A/data", "ok", 0644}, {"QuickCleaner.app/Versions/Current", "A", link}, {"QuickCleaner.app/Data", "Versions/Current/data", link}}
	staging, err := updaterExtractFixture(t, updaterZipFixture(t, good...), core.DefaultExtractLimits())
	if err != nil {
		t.Fatal(err)
	}
	if b, e := os.ReadFile(filepath.Join(staging, "QuickCleaner.app", "Data")); e != nil || string(b) != "ok" {
		t.Fatalf("%s %v", b, e)
	}
	if target, e := os.Readlink(filepath.Join(staging, "QuickCleaner.app", "Data")); e != nil || target != "Versions/Current/data" {
		t.Fatal(target, e)
	}
	if p, e := core.FindPayload(staging, core.PayloadSpec{Kind: core.MacApp, Name: "QuickCleaner.app"}); e != nil || !strings.HasSuffix(p, "QuickCleaner.app") {
		t.Fatal(p, e)
	}
	for _, tc := range []struct{ target, reason string }{
		{"", "symlink target is empty"}, {"/absolute", "symlink target is an absolute path"}, {`a\b`, "symlink target has illegal characters"},
		{"../../outside", "symlink escapes destination"}, {"..", "symlink resolves to the archive root"}, {"missing", "symlink target is missing from the archive"},
		{"../other", "symlink resolves outside its top-level entry"}, {"self", "symlink chain is too deep"}, {strings.Repeat("a", 1025), "symlink target is too long"}, {string([]byte{0xff}), "symlink target is not readable UTF-8"},
	} {
		t.Run(tc.reason, func(t *testing.T) {
			stage, e := updaterExtractFixture(t, updaterZipFixture(t, updaterFixtureEntry{"app/self", tc.target, link}, updaterFixtureEntry{"other", "ok", 0}), core.DefaultExtractLimits())
			if e == nil || !strings.Contains(e.Error(), tc.reason) {
				t.Fatal(e)
			}
			entries, _ := os.ReadDir(stage)
			if len(entries) != 0 {
				t.Fatal("link rejection wrote files")
			}
		})
	}
	if _, e := updaterExtractFixture(t, updaterZipFixture(t, updaterFixtureEntry{"app/link", "dir", link}, updaterFixtureEntry{"app/link/child", "bad", 0}, updaterFixtureEntry{"app/dir/", "", os.ModeDir | 0755}), core.DefaultExtractLimits()); e == nil || !strings.Contains(e.Error(), "entry is nested under a symlink") {
		t.Fatal(e)
	}
}

func TestUpdaterStagingAndPayload(t *testing.T) {
	cache := t.TempDir()
	outside := t.TempDir()
	marker := filepath.Join(outside, "keep")
	_ = os.WriteFile(marker, []byte("ok"), 0600)
	_ = os.Mkdir(filepath.Join(cache, "update-staging-old"), 0700)
	_ = os.WriteFile(filepath.Join(cache, "download.zip"), []byte("keep"), 0600)
	if runtime.GOOS != "windows" {
		if err := os.Symlink(outside, filepath.Join(cache, "update-staging-link")); err != nil {
			t.Fatal(err)
		}
	}
	staging, err := core.CreateStagingDir(cache)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = os.Stat(marker); err != nil {
		t.Fatal("cleanup followed symlink", err)
	}
	if _, err = os.Stat(filepath.Join(cache, "download.zip")); err != nil {
		t.Fatal(err)
	}
	archive := filepath.Join(cache, "fixture.zip")
	_ = os.WriteFile(archive, updaterZipFixture(t, updaterFixtureEntry{"z/quick-cleaner.exe", "z", 0}, updaterFixtureEntry{"a/quick-cleaner.exe", "a", 0}), 0600)
	p, err := core.ExtractUpdatePayload(archive, staging, core.PayloadSpec{Kind: core.WindowsExe, Name: "quick-cleaner.exe"}, core.DefaultExtractLimits())
	if err != nil || p != filepath.Join(staging, "a", "quick-cleaner.exe") {
		t.Fatal(p, err)
	}
	if err = core.ExtractZip(archive, staging, core.DefaultExtractLimits()); err == nil || !strings.Contains(err.Error(), "is not empty") {
		t.Fatal(err)
	}
	root := t.TempDir()
	_ = os.MkdirAll(filepath.Join(root, "deep", "deeper"), 0700)
	_ = os.WriteFile(filepath.Join(root, "deep", "deeper", "quick-cleaner.exe"), []byte("no"), 0600)
	if _, e := core.FindPayload(root, core.PayloadSpec{Kind: core.WindowsExe, Name: "quick-cleaner.exe"}); e == nil {
		t.Fatal("searched too deeply")
	}
	if runtime.GOOS != "windows" {
		_ = os.Symlink(p, filepath.Join(root, "quick-cleaner.exe"))
		if _, e := core.FindPayload(root, core.PayloadSpec{Kind: core.WindowsExe, Name: "quick-cleaner.exe"}); e == nil {
			t.Fatal("symlink payload accepted")
		}
		linked := filepath.Join(cache, "linked")
		_ = os.Symlink(root, linked)
		if e := core.ExtractZip(archive, linked, core.DefaultExtractLimits()); e == nil || !strings.Contains(e.Error(), "is not a real directory") {
			t.Fatal(e)
		}
	}
	_ = os.Mkdir(filepath.Join(cache, "extracted"), 0700)
	_, err = core.ExtractUpdateZip(archive, cache)
	if runtime.GOOS == "linux" && (err == nil || err.Error() != "unsupported platform for app update") {
		t.Fatal(err)
	}
	if _, e := os.Stat(filepath.Join(cache, "extracted")); !os.IsNotExist(e) {
		t.Fatal("legacy extraction not retired")
	}
}

func TestUpdaterHTTPDownloadChecksumExtract(t *testing.T) {
	data := updaterZipFixture(t, updaterFixtureEntry{"quick-cleaner.exe", strings.Repeat("binary", 15000), 0755})
	var server *httptest.Server
	server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("User-Agent") != "QuickCleaner-Updater/test-version" {
			t.Error("UA", r.Header)
		}
		switch r.URL.Path {
		case "/latest":
			if r.Header.Get("Accept") != "application/vnd.github+json" {
				t.Error("Accept")
			}
			_ = json.NewEncoder(w).Encode(core.GithubRelease{TagName: "v2", HTMLURL: "release", Assets: []core.GithubAsset{{Name: "quick-cleaner-windows-x86_64.zip", BrowserDownloadURL: server.URL + "/zip", Size: uint64(len(data))}}})
		case "/zip":
			w.Header().Set("Content-Length", fmt.Sprint(len(data)))
			_, _ = w.Write(data)
		case "/zip.sha256":
			_, _ = io.WriteString(w, "\ufeff"+strings.Repeat("0", 64)+"  different-name.zip\n")
		case "/chunked":
			w.(http.Flusher).Flush()
			_, _ = w.Write(data)
		case "/truncated":
			w.Header().Set("Content-Length", "1000")
			_, _ = io.WriteString(w, "short")
		default:
			http.Error(w, "missing", 404)
		}
	}))
	defer server.Close()
	c := core.NewUpdateHTTPClient("test-version")
	c.LatestReleaseURL = server.URL + "/latest"
	c.Client = server.Client()
	r, err := c.FetchLatestRelease(context.Background(), time.Second)
	if err != nil {
		t.Fatal(err)
	}
	s, err := core.EvaluateRelease("1", &r, core.CandidateAssetNames(core.WindowsX64))
	if err != nil {
		t.Fatal(err)
	}
	dest := filepath.Join(t.TempDir(), "nested", "a.zip")
	var last uint64
	calls := 0
	err = c.DownloadToFile(context.Background(), s.AssetURL, dest, func(n uint64, total *uint64) {
		if n <= last || total == nil || *total != uint64(len(data)) {
			t.Errorf("progress %d %v", n, total)
		}
		last = n
		calls++
	})
	if err != nil || last != uint64(len(data)) || calls == 0 {
		t.Fatal(err, last, calls)
	}
	hash, err := core.SHA256File(dest)
	if err != nil {
		t.Fatal(err)
	}
	if e := core.VerifyChecksum(dest, "\ufeff\r\n"+strings.ToUpper(hash)+"  ignored-filename\r\n"); e != nil {
		t.Fatal(e)
	}
	text, err := c.FetchText(context.Background(), s.ChecksumURL, time.Second)
	if err != nil {
		t.Fatal(err)
	}
	if e := core.VerifyChecksum(dest, text); e == nil || e.Error() != fmt.Sprintf("checksum mismatch: expected %s, got %s", strings.Repeat("0", 64), hash) {
		t.Fatal(e)
	}
	if e := core.VerifyChecksum("missing", "junk"); e == nil || e.Error() != "checksum sidecar is malformed" {
		t.Fatal(e)
	}
	staging, err := core.CreateStagingDir(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	if _, e := core.ExtractUpdatePayload(dest, staging, core.PayloadSpec{Kind: core.WindowsExe, Name: "quick-cleaner.exe"}, core.DefaultExtractLimits()); e != nil {
		t.Fatal(e)
	}
	if err = c.DownloadToFile(context.Background(), server.URL+"/chunked", dest, func(_ uint64, total *uint64) {
		if total != nil {
			t.Error("unknown length")
		}
	}); err != nil {
		t.Fatal(err)
	}
	if e := c.DownloadToFile(context.Background(), server.URL+"/truncated", dest, nil); e == nil || !strings.Contains(e.Error(), "read download:") {
		t.Fatal(e)
	}
	for _, op := range []func() error{
		func() error { _, e := c.FetchText(context.Background(), server.URL+"/404", time.Second); return e },
		func() error { return c.DownloadToFile(context.Background(), server.URL+"/404", dest, nil) },
	} {
		if e := op(); e == nil {
			t.Fatal("HTTP status ignored")
		}
	}
}

func TestUpdaterHTTPCancellationAndTextLimit(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/large":
			_, _ = io.CopyN(w, strings.NewReader(strings.Repeat("a", 10*1024*1024+1)), 10*1024*1024+1)
		case "/invalid":
			_, _ = w.Write([]byte{'a', 0xff, 'b'})
		default:
			w.(http.Flusher).Flush()
			<-r.Context().Done()
		}
	}))
	defer server.Close()
	c := core.NewUpdateHTTPClient("test")
	c.LatestReleaseURL = server.URL + "/wait"
	if _, e := c.FetchLatestRelease(context.Background(), 20*time.Millisecond); !errors.Is(e, context.DeadlineExceeded) {
		t.Fatal(e)
	}
	if _, e := c.FetchText(context.Background(), server.URL+"/wait", 20*time.Millisecond); !errors.Is(e, context.DeadlineExceeded) {
		t.Fatal(e)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	if e := c.DownloadToFile(ctx, server.URL+"/wait", filepath.Join(t.TempDir(), "partial"), nil); !errors.Is(e, context.DeadlineExceeded) {
		t.Fatal(e)
	}
	if _, e := c.FetchText(context.Background(), server.URL+"/large", time.Second); e == nil || e.Error() != "read body: response too big for into_string" {
		t.Fatal(e)
	}
	if text, e := c.FetchText(context.Background(), server.URL+"/invalid", time.Second); e != nil || text != "a\ufffdb" {
		t.Fatal(text, e)
	}
}
