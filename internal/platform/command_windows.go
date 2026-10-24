package platform

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"syscall"
)

func commandFile(name string) bool { s, e := os.Stat(name); return e == nil && s.Mode().IsRegular() }

func resolveToolIn(name, path, extensions string) (string, bool) {
	exts := strings.Split(extensions, ";")
	for _, dir := range filepath.SplitList(path) {
		for _, ext := range exts {
			if extensions != "" && ext == "" {
				continue
			}
			p := filepath.Join(dir, name+ext)
			if commandFile(p) {
				return p, true
			}
		}
	}
	return "", false
}

func ResolveToolProgram(name string) (string, bool) {
	if filepath.IsAbs(name) {
		return name, commandFile(name)
	}
	ext := ""
	if filepath.Ext(name) == "" {
		var present bool
		ext, present = os.LookupEnv("PATHEXT")
		if !present {
			ext = ".COM;.EXE;.BAT;.CMD"
		}
	}
	return resolveToolIn(name, os.Getenv("PATH"), ext)
}

func shimCommandLine(program string, args []string) (string, bool) {
	bad := func(s string) bool { return strings.ContainsAny(s, "\"&|<>^%!\r\n\x00") }
	if bad(program) {
		return "", false
	}
	line := `"` + program + `"`
	for _, arg := range args {
		if bad(arg) {
			return "", false
		}
		if arg == "" || strings.ContainsAny(arg, " \t()") {
			arg = `"` + arg + `"`
		}
		line += " " + arg
	}
	return `cmd.exe /d /v:off /s /c "` + line + `"`, true
}

func ToolCommand(name string, args []string) (*exec.Cmd, bool) {
	p, ok := ResolveToolProgram(name)
	if !ok {
		return nil, false
	}
	ext := strings.ToLower(filepath.Ext(p))
	if ext != ".cmd" && ext != ".bat" {
		return exec.Command(p, args...), true
	}
	line, ok := shimCommandLine(p, args)
	if !ok {
		return nil, false
	}
	cmd := exec.Command("cmd.exe")
	cmd.SysProcAttr = &syscall.SysProcAttr{CmdLine: line, HideWindow: true}
	return cmd, true
}
