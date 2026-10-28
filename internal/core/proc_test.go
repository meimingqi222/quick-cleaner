package core

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
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
		if err := os.WriteFile(filepath.Join(args[1], "ready"), []byte("yes"), 0600); err != nil {
			os.Exit(20)
		}
		time.Sleep(4 * time.Second)
		_ = os.WriteFile(filepath.Join(args[1], "finished"), []byte("bad"), 0600)
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
	for _, mode := range []string{"parent", "exit-parent", "finish-parent"} {
		t.Run(mode, func(t *testing.T) {
			dir := t.TempDir()
			start := time.Now()
			r, ok := RunWithTimeout(os.Args[0], helperArgs(mode, dir), 2*time.Second)
			elapsed := time.Since(start)
			if _, err := os.Stat(filepath.Join(dir, "ready")); err != nil {
				t.Fatalf("descendant did not start: %v (result=%+v ok=%v)", err, r, ok)
			}
			if ok != (mode == "finish-parent") {
				t.Fatalf("unexpected transport result: %v", ok)
			}
			if elapsed >= 3*time.Second {
				t.Fatalf("deadline exceeded: %s", elapsed)
			}
			time.Sleep(4 * time.Second)
			if _, err := os.Stat(filepath.Join(dir, "finished")); !os.IsNotExist(err) {
				t.Fatal("descendant survived command cleanup")
			}
		})
	}
}
