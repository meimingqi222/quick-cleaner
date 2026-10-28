//go:build !windows

package platform

import (
	"os"
	"os/exec"
	"path/filepath"
)

func ResolveToolProgram(name string) (string, bool) {
	if filepath.IsAbs(name) {
		return name, commandFile(name)
	}
	for _, dir := range filepath.SplitList(os.Getenv("PATH")) {
		p := filepath.Join(dir, name)
		if commandFile(p) {
			return p, true
		}
	}
	return "", false
}

func commandFile(name string) bool { s, e := os.Stat(name); return e == nil && s.Mode().IsRegular() }

func ToolCommand(name string, args []string) (*exec.Cmd, bool) {
	p, ok := ResolveToolProgram(name)
	if !ok {
		return nil, false
	}
	return exec.Command(p, args...), true
}
