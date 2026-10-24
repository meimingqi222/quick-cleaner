package core

import (
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
	"golang.org/x/text/cases"
	"golang.org/x/text/language"
)

type LocalAccount struct {
	Name, Path string
	IsCurrent  bool
	SizeBytes  *uint64
}

func ListLocalAccounts() []LocalAccount {
	home, _ := platform.UserHome()
	root := "/home"
	switch runtime.GOOS {
	case "darwin":
		root = "/Users"
	case "windows":
		root = `C:\Users`
		if drive, ok := os.LookupEnv("SystemDrive"); ok {
			root = drive + `\Users`
		}
		if home != "" {
			if parent := filepath.Dir(home); parent != home {
				root = parent
			}
		}
	}
	return listLocalAccountsAt(root, home, runtime.GOOS)
}

// Listing only: a tray opening must never recursively measure user homes.
func listLocalAccountsAt(root, home, goos string) []LocalAccount {
	entries, err := os.ReadDir(root)
	if err != nil {
		return nil
	}
	var accounts []LocalAccount
	lower := cases.Lower(language.Und)
	for _, entry := range entries {
		name := entry.Name()
		if !utf8.ValidString(name) || strings.HasPrefix(name, ".") {
			continue
		}
		if goos == "windows" {
			switch lower.String(name) {
			case "default", "default user", "all users", "public":
				continue
			}
		}
		path := filepath.Join(root, name)
		info, err := os.Stat(path)
		if err != nil || !info.IsDir() {
			continue
		}
		accounts = append(accounts, LocalAccount{Name: name, Path: path, IsCurrent: path == home})
	}
	slices.SortStableFunc(accounts, func(a, b LocalAccount) int {
		if a.IsCurrent != b.IsCurrent {
			if a.IsCurrent {
				return -1
			}
			return 1
		}
		return strings.Compare(lower.String(a.Name), lower.String(b.Name))
	})
	return accounts
}
