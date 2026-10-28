//go:build !windows

package core

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestAppCaskUninstallerRejectsRedirects(t *testing.T) {
	for _, target := range []string{"config", "app", "brew"} {
		t.Run(target, func(t *testing.T) {
			f := newCaskFixture(t)
			p := f.config
			if target == "app" {
				p = f.selected
			}
			if target == "brew" {
				p = f.brew
			}
			external := filepath.Join(t.TempDir(), "external")
			if err := os.Rename(p, external); err != nil {
				t.Fatal(err)
			}
			if err := os.Symlink(external, p); err != nil {
				t.Fatal(err)
			}
			handled, err := runAppCaskUninstaller(context.Background(), f.app, f.home, []string{f.prefix}, f.metadata, runCaskCommand)
			if err == nil {
				t.Fatalf("redirect accepted handled=%v", handled)
			}
		})
	}
}

func TestCaskCommandExactEnvironmentAndArguments(t *testing.T) {
	root := t.TempDir()
	brew := filepath.Join(root, "controlled-executable")
	home := filepath.Join(root, "original home")
	// This executable only prints its inputs; no Homebrew or uninstall is run.
	caskWrite(t, brew, []byte("#!/bin/sh\nprintf 'arg:%s\\n' \"$@\"\nprintf 'home:%s\\npath:%s\\n' \"$HOME\" \"$PATH\"\nprintf 'flags:%s:%s:%s:%s\\n' \"$HOMEBREW_NO_AUTO_UPDATE\" \"$HOMEBREW_NO_AUTOREMOVE\" \"$HOMEBREW_NO_INSTALL_FROM_API\" \"$HOMEBREW_NO_ANALYTICS\"\nprintf 'leak:%s\\n' \"${QUICKCLEANER_CASK_TEST_SECRET-unset}\"\n"))
	t.Setenv("QUICKCLEANER_CASK_TEST_SECRET", "must-not-inherit")
	r, ok := runCaskCommand(context.Background(), brew, home, "renamed@1.2")
	want := "arg:uninstall\narg:--cask\narg:--\narg:renamed@1.2\nhome:" + home + "\npath:/usr/bin:/bin:/usr/sbin:/sbin\nflags:1:1:1:1\nleak:unset\n"
	if !ok || !r.OK || string(r.Stdout) != want {
		t.Fatalf("complete=%v result=%+v stdout=%q", ok, r, string(r.Stdout))
	}
}

func TestCaskCommandCancellationKillsDescendants(t *testing.T) {
	for _, ending := range []string{"wait", "exit 0"} {
		t.Run(ending, func(t *testing.T) {
			root := t.TempDir()
			brew := filepath.Join(root, "controlled-executable")
			marker := filepath.Join(root, "survived")
			caskWrite(t, brew, []byte("#!/bin/sh\n(sleep 1; printf survived > '"+strings.ReplaceAll(marker, "'", "'\\''")+"') &\n"+ending+"\n"))
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			timer := time.AfterFunc(100*time.Millisecond, cancel)
			defer timer.Stop()
			start := time.Now()
			r, ok := runCaskCommand(ctx, brew, root, "test")
			if time.Since(start) > 2*time.Second || ok && r.OK || ctx.Err() == nil {
				t.Fatalf("cancellation not bounded: %+v %v %v", r, ok, ctx.Err())
			}
			time.Sleep(1100 * time.Millisecond)
			if _, err := os.Lstat(marker); !os.IsNotExist(err) {
				t.Fatalf("descendant survived: %v", err)
			}
		})
	}
}
