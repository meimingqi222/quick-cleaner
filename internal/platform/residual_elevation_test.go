//go:build !windows

package platform

import (
	"context"
	"encoding/json"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"
)

type residualCommandReceipt struct {
	Args  []string
	Stdin string
}

func TestResidualElevationProcessFixture(t *testing.T) {
	if os.Getenv("QC_RESIDUAL_COMMAND_FIXTURE") != "1" {
		return
	}
	input, err := io.ReadAll(os.Stdin)
	if err != nil {
		os.Exit(90)
	}
	if json.NewEncoder(os.Stdout).Encode(residualCommandReceipt{os.Args[3:], string(input)}) != nil {
		os.Exit(91)
	}
	os.Exit(7)
}

// Execute an isolated observer instead of any native command. This exercises
// argv/stdin transport and nonzero exit handling through the bounded runner.
func residualFixtureRunner(t *testing.T, receipts *[]residualCommandReceipt) func(context.Context, *exec.Cmd, time.Duration) (ProcessResult, bool) {
	t.Helper()
	return func(ctx context.Context, original *exec.Cmd, timeout time.Duration) (ProcessResult, bool) {
		fixture := exec.Command(os.Args[0], append([]string{"-test.run=^TestResidualElevationProcessFixture$", "--"}, original.Args...)...)
		fixture.Env = append(os.Environ(), "QC_RESIDUAL_COMMAND_FIXTURE=1")
		fixture.Stdin = original.Stdin
		result, complete := RunCommandTreeContext(ctx, fixture, timeout)
		if !complete || result.ExitCode == nil || *result.ExitCode != 7 {
			t.Fatalf("fixture did not complete with expected nonzero exit: %+v %v", result, complete)
		}
		var receipt residualCommandReceipt
		if err := json.Unmarshal(result.Stdout, &receipt); err != nil {
			t.Fatal(err)
		}
		*receipts = append(*receipts, receipt)
		return result, complete
	}
}

func TestResidualElevationTaggedArgumentsAndStableOrdering(t *testing.T) {
	ordinary := filepath.Join(t.TempDir(), "quote'\"; $(touch NEVER)\n你好")
	daemon1 := "/Library/LaunchDaemons/z'\".plist"
	daemon2 := "/library/launchdaemons/a.plist"
	agent := "/Library/LaunchAgents/a.plist"
	userAgent := filepath.Join(t.TempDir(), "Library/LaunchAgents/a.plist")
	paths := []string{ordinary, agent, daemon1, "rejected", daemon2, userAgent}
	var receipts []residualCommandReceipt
	removePrivilegedResiduals(context.Background(), paths, func(p string) bool { return p != "rejected" }, residualElevationOps{
		run: residualFixtureRunner(t, &receipts),
	})
	want := []string{"/usr/bin/osascript", "-", "daemon:" + daemon1, "daemon:" + daemon2, "agent:" + agent, "path:" + ordinary, "path:" + userAgent}
	if len(receipts) != 1 || !reflect.DeepEqual(receipts[0].Args, want) {
		t.Fatalf("receipts = %+v, want one invocation with %q", receipts, want)
	}
	if receipts[0].Stdin != residualRemoveScript || strings.Contains(receipts[0].Stdin, ordinary) {
		t.Fatal("path became script source")
	}
	if strings.Count(residualRemoveScript, "do shell script") != 1 || !strings.Contains(residualRemoveScript, "quoted form of p") {
		t.Fatal("script must quote argv and authorize one shell only")
	}
}

func TestResidualElevationRootUsesSameAuthorizationAndBootoutOrder(t *testing.T) {
	daemon := "/Library/LaunchDaemons/a.plist"
	agent := "/Library/LaunchAgents/a.plist"
	ordinary := filepath.Join(t.TempDir(), "-quote'\";$(false)")
	var receipts []residualCommandReceipt
	removePrivilegedResiduals(context.Background(), []string{ordinary, agent, "rejected", daemon}, func(p string) bool { return p != "rejected" }, residualElevationOps{
		root: true, uid: 501, run: residualFixtureRunner(t, &receipts),
	})
	want := [][]string{
		{"/bin/launchctl", "bootout", "system", daemon},
		{"/bin/rm", "-f", daemon},
		{"/bin/launchctl", "bootout", "gui/501", agent},
		{"/bin/rm", "-f", agent},
		{"/bin/rm", "-rf", ordinary},
	}
	var got [][]string
	for _, receipt := range receipts {
		got = append(got, receipt.Args)
		if receipt.Stdin != "" {
			t.Fatal("root command unexpectedly uses script input")
		}
	}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("commands = %q, want %q", got, want)
	}
}

func TestResidualElevationCompletionRequiresLstatNotFound(t *testing.T) {
	dir := t.TempDir()
	existing := filepath.Join(dir, "existing")
	if err := os.WriteFile(existing, []byte("keep"), 0600); err != nil {
		t.Fatal(err)
	}
	absent := filepath.Join(dir, "absent")
	dangling := filepath.Join(dir, "dangling")
	loop := filepath.Join(dir, "loop")
	if err := os.Symlink(absent, dangling); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(loop, loop); err != nil {
		t.Fatal(err)
	}
	unknown := filepath.Join(loop, "child")
	if _, err := os.Lstat(unknown); err == nil || os.IsNotExist(err) {
		t.Fatalf("fixture must produce unknown rather than absence: %v", err)
	}
	for _, root := range []bool{false, true} {
		var receipts []residualCommandReceipt
		got := removePrivilegedResiduals(context.Background(), []string{existing, absent, dangling, unknown}, func(string) bool { return true }, residualElevationOps{
			root: root, run: residualFixtureRunner(t, &receipts),
		})
		if !reflect.DeepEqual(got, map[string]bool{absent: true}) {
			t.Fatalf("root=%v completion = %v, want only missing path", root, got)
		}
	}
}

func TestResidualElevationFailsClosedWithoutAuthorizationOrTransport(t *testing.T) {
	absent := filepath.Join(t.TempDir(), "missing")
	for _, root := range []bool{false, true} {
		ops := residualElevationOps{root: root, run: func(context.Context, *exec.Cmd, time.Duration) (ProcessResult, bool) {
			t.Fatal("unauthorized command executed")
			return ProcessResult{}, false
		}}
		for _, allowed := range []func(string) bool{nil, func(string) bool { return false }} {
			if got := removePrivilegedResiduals(context.Background(), []string{absent}, allowed, ops); len(got) != 0 {
				t.Fatalf("unauthorized completion: %v", got)
			}
		}
		ctx, cancel := context.WithCancel(context.Background())
		cancel()
		if got := removePrivilegedResiduals(ctx, []string{absent}, func(string) bool { return true }, ops); len(got) != 0 {
			t.Fatalf("canceled completion: %v", got)
		}
	}
	got := removePrivilegedResiduals(context.Background(), []string{absent}, func(string) bool { return true }, residualElevationOps{
		run: func(context.Context, *exec.Cmd, time.Duration) (ProcessResult, bool) { return ProcessResult{}, false },
	})
	if len(got) != 0 {
		t.Fatalf("transport failure produced receipt: %v", got)
	}
}
