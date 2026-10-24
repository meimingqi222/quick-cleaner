package core

import (
	"os"
	"path/filepath"
	"reflect"
	"testing"
)

func TestLocalAccountListingKeepsCurrentFirstWithoutMeasuring(t *testing.T) {
	root := t.TempDir()
	for _, name := range []string{"zCurrent", "bob", "Alice", "Shared", ".hidden", "Default", "Default User", "All Users", "Public"} {
		if err := os.Mkdir(filepath.Join(root, name), 0700); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(filepath.Join(root, "not-an-account"), []byte("file"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "zCurrent", "large-file"), []byte("do not measure"), 0600); err != nil {
		t.Fatal(err)
	}
	for _, tt := range []struct {
		os   string
		want []string
	}{
		{"windows", []string{"zCurrent", "Alice", "bob", "Shared"}},
		{"darwin", []string{"zCurrent", "Alice", "All Users", "bob", "Default", "Default User", "Public", "Shared"}},
	} {
		accounts := listLocalAccountsAt(root, filepath.Join(root, "zCurrent"), tt.os)
		var names []string
		for i, account := range accounts {
			names = append(names, account.Name)
			if account.SizeBytes != nil || account.IsCurrent != (i == 0) || account.Path != filepath.Join(root, account.Name) {
				t.Fatalf("account %+v", account)
			}
		}
		if !reflect.DeepEqual(names, tt.want) {
			t.Fatalf("%s: %v want %v", tt.os, names, tt.want)
		}
	}
	if got := listLocalAccountsAt(filepath.Join(root, "missing"), "", "windows"); len(got) != 0 {
		t.Fatal(got)
	}
}
