package core

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"
)

func TestProcHelper(t *testing.T) {
	idx := -1
	for i, a := range os.Args {
		if a == "--qc-proc" {
			idx = i
			break
		}
	}
	if idx < 0 {
		return
	}
	args := os.Args[idx+1:]
	switch args[0] {
	case "result":
		fmt.Fprint(os.Stdout, "stdout")
		fmt.Fprint(os.Stderr, "stderr")
		os.Exit(7)
	case "large":
		for i := 0; i < 10000; i++ {
			fmt.Fprintln(os.Stdout, strings.Repeat("x", 100))
			fmt.Fprintln(os.Stderr, strings.Repeat("y", 100))
		}
	case "leaf":
		// Rename so the parent never sees a half-written PID.
		tmp := filepath.Join(args[1], "ready.tmp")
		if os.WriteFile(tmp, []byte(strconv.Itoa(os.Getpid())), 0600) != nil || os.Rename(tmp, filepath.Join(args[1], "ready")) != nil {
			os.Exit(20)
		}
		// Far longer than any check window, so a leaf that escaped cleanup can
		// never pass the test by exiting on its own.
		time.Sleep(60 * time.Second)
	case "parent", "exit-parent", "finish-parent":
		cmd := exec.Command(os.Args[0], "-test.run=^TestProcHelper$", "--", "--qc-proc", "leaf", args[1])
		if args[0] != "finish-parent" {
			cmd.Stdout, cmd.Stderr = os.Stdout, os.Stderr
		}
		if err := cmd.Start(); err != nil {
			os.Exit(21)
		}
		for {
			if _, err := os.Stat(filepath.Join(args[1], "ready")); err == nil {
				break
			}
			time.Sleep(time.Millisecond)
		}
		if args[0] == "parent" {
			time.Sleep(30 * time.Second)
		}
	}
	os.Exit(0)
}

func helperArgs(mode string, extra ...string) []string {
	return append([]string{"-test.run=^TestProcHelper$", "--", "--qc-proc", mode}, extra...)
}

func TestProcResults(t *testing.T) {
	r, ok := RunWithTimeout(os.Args[0], helperArgs("result"), 5*time.Second)
	if !ok || r.OK || r.ExitCode == nil || *r.ExitCode != 7 || string(r.Stdout) != "stdout" || string(r.Stderr) != "stderr" {
		t.Fatalf("result: %+v transport=%v", r, ok)
	}
	r, ok = RunToolWithTimeout(os.Args[0], helperArgs("large"), 10*time.Second)
	if !ok || !r.OK || r.ExitCode == nil || *r.ExitCode != 0 || len(r.Stdout) != 1010000 || len(r.Stderr) != 1010000 {
		t.Fatalf("large output: out=%d err=%d transport=%v", len(r.Stdout), len(r.Stderr), ok)
	}
	if _, ok := RunWithTimeout(filepath.Join(t.TempDir(), "missing"), nil, time.Second); ok {
		t.Fatal("missing program succeeded")
	}
	if _, ok := RunToolWithTimeout("qc-definitely-missing-command", nil, time.Second); ok {
		t.Fatal("missing tool succeeded")
	}
}

func TestProcTreeDeadline(t *testing.T) {
	t.Parallel()
	for _, mode := range []string{"parent", "exit-parent", "finish-parent"} {
		t.Run(mode, func(t *testing.T) {
			t.Parallel()
			dir := t.TempDir()
			start := time.Now()
			r, ok := RunWithTimeout(os.Args[0], helperArgs(mode, dir), 2*time.Second)
			elapsed := time.Since(start)
			pid := readLeafPID(t, dir)
			if ok != (mode == "finish-parent") {
				t.Fatalf("unexpected transport result: %v (result=%+v)", ok, r)
			}
			if elapsed >= 3*time.Second {
				t.Fatalf("deadline exceeded: %s", elapsed)
			}
			if !waitProcessGone(pid, 3*time.Second) {
				t.Fatalf("descendant %d survived command cleanup", pid)
			}
		})
	}
}

func readLeafPID(t *testing.T, dir string) int {
	t.Helper()
	data, err := os.ReadFile(filepath.Join(dir, "ready"))
	if err != nil {
		t.Fatalf("descendant did not start: %v", err)
	}
	pid, err := strconv.Atoi(string(data))
	if err != nil || pid <= 0 {
		t.Fatalf("descendant pid %q: %v", data, err)
	}
	return pid
}
