package platform

import (
	"bytes"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"
)

type inUseProcess struct {
	pid, parent int
	command     string
}

func inUseProcesses() ([]inUseProcess, bool) {
	r, ok := RunCommandTree(exec.Command("/bin/ps", "-axo", "pid=,ppid=,comm="), 3*time.Second)
	if !ok || !r.OK || len(r.Stdout) == 0 || r.Stdout[len(r.Stdout)-1] != '\n' {
		return nil, false
	}
	var procs []inUseProcess
	for _, line := range strings.Split(string(r.Stdout), "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		i := strings.IndexAny(line, " \t")
		if i < 0 {
			return nil, false
		}
		pid, e := strconv.Atoi(line[:i])
		if e != nil {
			return nil, false
		}
		line = strings.TrimSpace(line[i:])
		i = strings.IndexAny(line, " \t")
		if i < 0 {
			return nil, false
		}
		parent, e := strconv.Atoi(line[:i])
		if e != nil {
			return nil, false
		}
		procs = append(procs, inUseProcess{pid, parent, strings.ToLower(strings.TrimSpace(line[i:]))})
	}
	return procs, true
}
func inUseExcluded(procs []inUseProcess) map[int]bool {
	excluded := map[int]bool{os.Getpid(): true}
	for {
		before := len(excluded)
		for _, p := range procs {
			if excluded[p.parent] {
				excluded[p.pid] = true
			}
		}
		if before == len(excluded) {
			return excluded
		}
	}
}
func inUseComplete(raw []byte) bool {
	raw = bytes.TrimSuffix(raw, []byte{'\n'})
	return len(raw) == 0 || raw[len(raw)-1] == 0
}
func inUsePaths(raw []byte, excluded map[int]bool) []string {
	var paths []string
	skip := false
	for _, field := range bytes.Split(raw, []byte{0}) {
		field = bytes.TrimLeft(field, "\n")
		if len(field) == 0 {
			continue
		}
		if field[0] == 'p' {
			pid, _ := strconv.Atoi(string(field[1:]))
			skip = excluded[pid]
		}
		if field[0] == 'n' && !skip && len(field) > 1 && field[1] == '/' {
			paths = append(paths, string(field[1:]))
		}
	}
	return paths
}
func inUseCanonical(path string) string {
	if p, e := filepath.EvalSymlinks(path); e == nil {
		path = p
	}
	p, e := filepath.Abs(path)
	if e == nil {
		return p
	}
	return path
}
func IsOpen(path string) (bool, bool) {
	md, err := os.Lstat(path)
	if err != nil || md.Mode()&os.ModeSymlink != 0 {
		return false, false
	}
	args := []string{"-F0n"}
	timeout := 3 * time.Second
	if md.IsDir() {
		args = append(args, "+D")
		timeout = 30 * time.Second
	}
	args = append(args, path)
	r, ok := RunCommandTree(exec.Command("/usr/sbin/lsof", args...), timeout)
	if !ok || r.ExitCode == nil || (*r.ExitCode != 0 && *r.ExitCode != 1) || len(bytes.TrimSpace(r.Stderr)) != 0 || !inUseComplete(r.Stdout) {
		return false, false
	}
	procs, _ := inUseProcesses()
	target := inUseCanonical(path)
	for _, p := range inUsePaths(r.Stdout, inUseExcluded(procs)) {
		if p == target || (md.IsDir() && strings.HasPrefix(p, target+"/")) {
			return true, true
		}
	}
	return false, true
}
func inUseOwningApp(path string, procs []inUseProcess) string {
	stem := filepath.Base(path)
	stem = strings.TrimPrefix(strings.TrimSuffix(strings.TrimSuffix(stem, "-updater"), ".ShipIt"), "@")
	candidates := []string{stem}
	parts := strings.Split(stem, ".")
	if len(parts) > 1 {
		candidates = append(candidates, parts[0], parts[len(parts)-1])
	}
	sort.SliceStable(candidates, func(i, j int) bool { return len(candidates[i]) > len(candidates[j]) })
	for _, c := range candidates {
		low := strings.ToLower(c)
		if len(low) < 4 || low == "default" || low == "updater" || low == "helper" || low == "service" || low == "data" {
			continue
		}
		for _, p := range procs {
			if strings.Contains(p.command, "/"+low+".app/") {
				return c
			}
		}
	}
	return ""
}
func DetectInUse(paths []string) map[string]InUseBusyInfo {
	out := make(map[string]InUseBusyInfo)
	procs, ok := inUseProcesses()
	if !ok {
		for _, p := range paths {
			out[p] = InUseBusyInfo{Unknown: true}
		}
		return out
	}
	for _, p := range paths {
		if app := inUseOwningApp(p, procs); app != "" {
			out[p] = InUseBusyInfo{App: app}
		}
	}
	r, ok := RunCommandTree(exec.Command("/usr/sbin/lsof", "-F0n", "-w"), 45*time.Second)
	if !ok || !r.OK || !inUseComplete(r.Stdout) {
		for _, p := range paths {
			b := out[p]
			b.Unknown = true
			out[p] = b
		}
		return out
	}
	opened := inUsePaths(r.Stdout, inUseExcluded(procs))
	want := make(map[string][]string, len(paths))
	for _, p := range paths {
		target := inUseCanonical(p)
		want[target] = append(want[target], p)
	}
	for _, open := range opened {
		for current := open; ; current = filepath.Dir(current) {
			for _, p := range want[current] {
				b := out[p]
				b.Open = true
				out[p] = b
			}
			if filepath.Dir(current) == current {
				break
			}
		}
	}
	return out
}
