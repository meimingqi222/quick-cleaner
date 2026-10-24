package platform

import (
	"syscall"
	"testing"
)

func TestDownloadsTrustedRedirectedFolder(t *testing.T) {
	for _, test := range []struct{ value, want string }{
		{`D:\Redirected\Downloads`, `D:\Redirected\Downloads`},
		{`\\server\share\下载`, `\\server\share\下载`},
		{`%uSeRpRoFiLe%\OneDrive\Downloads`, `C:\Users\original\OneDrive\Downloads`},
		{`C:relative`, ""}, {`\rooted`, ""}, {`%APPDATA%\Downloads`, ""}, {"", ""},
	} {
		t.Run(test.value, func(t *testing.T) {
			path, ok := collectDownloadsDir(func() (string, bool) { return "original-sid", true }, func() (string, bool) { return `C:\Users\original`, true }, func(root syscall.Handle, key string) []folderString {
				if root != syscall.HKEY_USERS || key != `original-sid\`+shellFoldersKey {
					t.Fatal("read wrong user's hive", root, key)
				}
				return []folderString{{"Personal", `C:\ignored`}, {"{374de290-123f-4565-9164-39c4925e467b}", test.value}}
			})
			if path != test.want || ok != (test.want != "") {
				t.Fatal(path, ok)
			}
		})
	}
}

func TestDownloadsUnknownProfileOrHiveNeverFallsBack(t *testing.T) {
	for _, missingHome := range []bool{false, true} {
		path, ok := collectDownloadsDir(func() (string, bool) { return "original-sid", missingHome }, func() (string, bool) { return `C:\Users\original`, !missingHome }, func(syscall.Handle, string) []folderString {
			t.Fatal("unknown identity must not read admin HKCU")
			return nil
		})
		if ok || path != "" {
			t.Fatal(path, ok)
		}
	}
	calls := 0
	path, ok := collectDownloadsDir(func() (string, bool) { return "original-sid", true }, func() (string, bool) { return `C:\Users\original`, true }, func(root syscall.Handle, _ string) []folderString {
		calls++
		if root != syscall.HKEY_USERS {
			t.Fatal("missing trusted hive must not fall back to admin HKCU")
		}
		return nil
	})
	if ok || path != "" || calls != 1 {
		t.Fatal(path, ok, calls)
	}
}
