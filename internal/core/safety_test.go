package core

import (
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

func TestNormAndBoundary(t *testing.T) {
	for input, want := range map[string]string{`C:/Users/ÖMER/`: `c:\users\ömer`, "D:/İ/Σ/": "d:\\i\u0307\\σ", `C:/用户/文档`: `c:\用户\文档`, "/": "", `C:/a/../b`: `c:\a\..\b`} {
		if got := Norm(input); got != want {
			t.Errorf("Norm(%q)=%q want %q", input, got, want)
		}
	}
	if !atOrUnder(`c:\a\b`, `c:\a`) || atOrUnder(`c:\ab`, `c:\a`) {
		t.Fatal("component boundary")
	}
	for _, n := range []string{"$MFTMirr", "DumpStack.log.tmp", "PAGEFILE.SYS", "System Volume Information"} {
		if !IsNTFSMetaName(n) {
			t.Error(n)
		}
	}
	if IsNTFSMetaName("notes.txt") {
		t.Fatal("ordinary name")
	}
}
func TestWindowsSafetyPolicy(t *testing.T) {
	p := newSafetyPolicy("windows", `Z:\Windows`, `C:\Users\Ömer`, `C:\Users\Admin`, `C:\Users\Public`, []string{`Q:\OneDrive\桌面`})
	for _, path := range []string{`C:\`, `Z:\Windows`, `Z:\Windows\Temp`, `Z:\Windows\System32\drivers`, `D:\System Volume Information\x`, `C:\Program Files`, `C:\ProgramData`, `D:\Users\Other\AppData\LocalLow`, `C:\Users\ÖMER\Desktop`, `C:\Users\Admin\Documents`, `C:\Users\Public\Libraries`, `Q:\OneDrive\桌面`} {
		if !p.protected(path, false) {
			t.Errorf("not protected: %s", path)
		}
	}
	for _, path := range []string{`Z:\Windows\Temp\a.tmp`, `Z:\Windows\System32foo`, `C:\Program Files\Vendor`, `D:\Users\Other\AppData\Local\Temp`, `C:\Users\Other\appdata2`, `C:\stuff\appdata`, `Q:\OneDrive\桌面\child`, `C:\Users\Public\Desktop\Vendor`} {
		if p.protected(path, false) {
			t.Errorf("overprotected: %s", path)
		}
	}
	if p.protected(`Z:\Windows\Temp`, true) || !p.protected(`Z:\Windows\System32`, true) || !p.systemRoot(`C:\Users`) {
		t.Fatal("contents/root parity")
	}
}
func TestMacSafetyPolicy(t *testing.T) {
	p := newSafetyPolicy("darwin", `C:\Windows`, "/Users/Alice", "", "", nil)
	for _, path := range []string{"/System/Library", "/Library/Fonts", "/Applications/Foo.app", "/usr/local/bin", "/private/var/db/receipts/x", "/Users/Alice", "/Users/Alice/Library", "/Users/Alice/Library/Application Support"} {
		if !p.protected(path, false) {
			t.Error(path)
		}
	}
	for _, path := range []string{"/Users/Alice/Library/Caches", "/Users/Alice/Library/Application Support/Vendor", "/private/var/folders/x", "/LibraryExtra/x"} {
		if p.protected(path, false) {
			t.Error(path)
		}
	}
	for _, parent := range elevatedParents {
		path := filepath.ToSlash(parent)
		path = "/" + path[1:]
		path = replaceBackslashes(path) + "/org.vendor"
		if !p.elevated(path) {
			t.Error(path)
		}
	}
	for _, path := range []string{"/Library", "/Library/Application Support", "/Library/Application Support/vendor/child", "/Library/Extensions/a.kext", "/Library/Caches/com.apple.TCC", "/Library/Preferences/SystemConfiguration", "/Library/Application Support/CrashReporter", "/Library/Caches/Keychains"} {
		if p.elevated(path) {
			t.Error(path)
		}
	}
	root := "/Users/Alice/Library/Application Support"
	if p.underAppSupport(root) || !p.underAppSupport(root+"/Vendor") || p.underAppSupport(root+"Extra/x") {
		t.Fatal("strict app support boundary")
	}
}
func replaceBackslashes(s string) string {
	b := []byte(s)
	for i := range b {
		if b[i] == '\\' {
			b[i] = '/'
		}
	}
	return string(b)
}
func TestOwnershipAndManagedWorktrees(t *testing.T) {
	for _, path := range []string{"/Applications/Foo.app/build", "C:/Users/A/AppData/Local/build", "/Users/a/.nvm/build", "/opt/lib/node_modulesX/build", "/Users/a/.local/share/uv/tools/tool/build"} {
		if !IsDevelopmentInstallationPath(path) {
			t.Error(path)
		}
	}
	// Windows TEMP is under AppData, which is an installation boundary.
	base := buildProjectFixture(t)
	child := filepath.Join(base, "project", "build")
	if IsDevelopmentInstallationPath(child) {
		t.Fatal("ordinary checkout")
	}
	if err := os.MkdirAll(filepath.Dir(child), 0700); err != nil {
		t.Fatal(err)
	}
	marker := filepath.Join(base, "project", "pyvenv.cfg")
	if err := os.WriteFile(marker, nil, 0600); err != nil {
		t.Fatal(err)
	}
	if !IsDevelopmentInstallationPath(child) {
		t.Fatal("venv ownership")
	}
	work := filepath.Join(base, "workspace")
	target := filepath.Join(work, "subagent-worktrees", "child")
	if err := os.MkdirAll(target, 0700); err != nil {
		t.Fatal(err)
	}
	if IsManagedAgentWorktree(target) {
		t.Fatal("unmanaged")
	}
	if err := os.WriteFile(filepath.Join(work, "runtime.sqlite"), nil, 0600); err != nil {
		t.Fatal(err)
	}
	if !IsManagedAgentWorktree(target) || !IsContentsProtected(target) {
		t.Fatal("managed protection")
	}
	// A non-directory workspace makes marker lookup fail with ENOTDIR, not NotFound.
	// Windows reports ERROR_PATH_NOT_FOUND there, which is a true absence.
	if runtime.GOOS == "windows" {
		return
	}
	file := filepath.Join(base, "file")
	if err := os.WriteFile(file, nil, 0600); err != nil {
		t.Fatal(err)
	}
	if !IsManagedAgentWorktree(filepath.Join(file, "subagent-worktrees", "child")) {
		t.Fatal("unknown marker must fail closed")
	}
}
func TestElevatedRejectsSymlink(t *testing.T) {
	link := filepath.Join(t.TempDir(), "link")
	if err := os.Symlink("missing", link); err != nil {
		t.Skip(err)
	}
	p := newSafetyPolicy("darwin", "", "", "", "", nil)
	// Extend the same real policy's parent list to isolate link rejection on a disposable filesystem.
	old := elevatedParents
	elevatedParents = append(append([]string{}, old...), Norm(filepath.Dir(link)))
	defer func() { elevatedParents = old }()
	if p.elevated(filepath.ToSlash(link)) {
		t.Fatal("symlink accepted")
	}
	if !p.elevated(filepath.ToSlash(filepath.Join(filepath.Dir(link), "missing"))) {
		t.Fatal("missing lexical target should pass")
	}
}
