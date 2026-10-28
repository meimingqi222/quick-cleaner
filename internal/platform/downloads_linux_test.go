package platform

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestDownloadsXDGRedirectAndUnknown(t *testing.T) {
	home := t.TempDir()
	config := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", config)
	for _, test := range []struct{ config, want string }{
		{`XDG_DOWNLOAD_DIR="/redirected/downloads"`, "/redirected/downloads"},
		{`XDG_DOWNLOAD_DIR="$HOME/下载"`, filepath.Join(home, "下载")},
		{`XDG_DESKTOP_DIR="/wrong"`, ""},
		{`XDG_DOWNLOAD_DIR="relative"`, ""},
		{`XDG_DOWNLOAD_DIR="$OTHER/downloads"`, ""},
		{`XDG_DOWNLOAD_DIR="$(touch /tmp/never)"`, ""},
		{strings.Repeat("x", 65537), ""},
	} {
		if err := os.WriteFile(filepath.Join(config, "user-dirs.dirs"), []byte(test.config), 0600); err != nil {
			t.Fatal(err)
		}
		path, ok := DownloadsDir()
		if path != test.want || ok != (test.want != "") {
			t.Fatalf("%q: %q %v", test.config[:min(60, len(test.config))], path, ok)
		}
	}
	t.Setenv("HOME", "relative")
	if path, ok := DownloadsDir(); ok || path != "" {
		t.Fatal("unknown home accepted", path)
	}
}
