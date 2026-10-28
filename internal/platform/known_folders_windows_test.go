package platform

import (
	"path/filepath"
	"reflect"
	"syscall"
	"testing"
)

func TestKnownFolderValues(t *testing.T) {
	home := func() (string, bool) { return `C:\Users\original`, true }
	sid := func() (string, bool) { return "trusted", true }
	entries := []folderString{
		{"dEsKtOp", `%uSeRpRoFiLe%\OneDrive\桌面`},
		{"Personal", `\\server\share\documents`},
		{"MY PICTURES", `D:\Pictures`},
		{"My Video", `C:relative`},
		{"My Music", `\rooted`},
		{"{374de290-123f-4565-9164-39c4925e467b}", `%USERPROFILE%/Downloads`},
		{"Favorites", `%APPDATA%\Favorites`},
		{"Other", `D:\ignored`},
	}
	calls := 0
	read := func(root syscall.Handle, path string) []folderString {
		calls++
		if root != syscall.HKEY_USERS || path != `trusted\`+shellFoldersKey {
			t.Fatal(root, path)
		}
		return entries
	}
	want := []string{`C:\Users\original\OneDrive\桌面`, `\\server\share\documents`, `D:\Pictures`, `C:\Users\original\Downloads`}
	if got := collectKnownFolders(sid, home, read); !reflect.DeepEqual(got, want) || calls != 1 {
		t.Fatal(got, calls)
	}
	missing := func() (string, bool) { return "", false }
	if got := collectKnownFolders(sid, missing, read); !reflect.DeepEqual(got, want[1:3]) {
		t.Fatal(got)
	}
	if asciiFolderEqual("Deſktop", "Desktop") {
		t.Fatal("Unicode folding is not ASCII folding")
	}
}

func TestKnownFolderHiveFallback(t *testing.T) {
	for _, nonempty := range []bool{false, true} {
		calls := 0
		got := collectKnownFolders(func() (string, bool) { return "trusted", true }, func() (string, bool) { t.Fatal("unexpected home lookup"); return "", false }, func(root syscall.Handle, path string) []folderString {
			calls++
			if calls == 1 {
				if root != syscall.HKEY_USERS {
					t.Fatal(root)
				}
				if nonempty {
					return []folderString{{"unrecognized", ""}}
				}
				return nil
			}
			if root != syscall.HKEY_CURRENT_USER || path != shellFoldersKey {
				t.Fatal(root, path)
			}
			return []folderString{{"Desktop", `D:\Redirected`}}
		})
		if nonempty {
			if calls != 1 || len(got) != 0 {
				t.Fatal(calls, got)
			}
		} else if calls != 2 || !reflect.DeepEqual(got, []string{`D:\Redirected`}) {
			t.Fatal(calls, got)
		}
	}
}

// Read-only native smoke test: never modify the user's Explorer registry keys.
func TestNativeKnownFolders(t *testing.T) {
	first := KnownFolders()
	for _, path := range first {
		if !filepath.IsAbs(path) {
			t.Fatalf("nonabsolute folder %q", path)
		}
	}
	if !reflect.DeepEqual(first, KnownFolders()) {
		t.Fatal("unstable cached folders")
	}
	if len(first) > 0 {
		first[0] = "modified"
		if KnownFolders()[0] == "modified" {
			t.Fatal("caller mutated cache")
		}
	}
}
