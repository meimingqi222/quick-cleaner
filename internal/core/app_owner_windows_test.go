package core

import (
	"os"
	"os/exec"
	"path/filepath"
	"syscall"
	"testing"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
	"golang.org/x/sys/windows"
)

func TestCleanupClosesWindowlessNativeProcess(t *testing.T) {
	if os.Getenv("GITHUB_ACTIONS") == "true" {
		t.Skip("GitHub runner process model cannot construct a clean sibling fixture")
	}
	if os.Getenv("QC_OWNER_TEST_CHILD") == "1" {
		time.Sleep(30 * time.Second)
		return
	}
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	cmd := exec.Command(exe, "-test.run=^TestCleanupClosesWindowlessNativeProcess$")
	cmd.Env = append(os.Environ(), "QC_OWNER_TEST_CHILD=1")
	// The release guard intentionally refuses the cleaner's descendants.
	// Give the fixture our test runner as parent, so it is a sibling instead.
	parent, err := windows.OpenProcess(windows.PROCESS_CREATE_PROCESS|windows.PROCESS_DUP_HANDLE, false, uint32(os.Getppid()))
	if err != nil {
		t.Fatal(err)
	}
	defer windows.CloseHandle(parent)
	cmd.SysProcAttr = &syscall.SysProcAttr{ParentProcess: syscall.Handle(parent)}
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() { _ = cmd.Process.Kill(); _ = cmd.Wait() }()
	// Only this disposable child can reach either exit request.
	stubOwners(t, func(names []string) ([]platform.CacheOccupant, error) {
		owners, err := platform.RunningAppOwners(names)
		var child []platform.CacheOccupant
		for _, owner := range owners {
			if owner.PID == uint32(cmd.Process.Pid) {
				child = append(child, owner)
			}
		}
		return child, err
	}, func(owner platform.CacheOccupant) error { return platform.RequestCacheOwnerExit(owner, false) })
	forceAppExit = func(owner platform.CacheOccupant) error {
		err := platform.RequestCacheOwnerExit(owner, true)
		if err != nil {
			t.Errorf("force fixture: %v", err)
		}
		return err
	}
	name := filepath.Base(exe)
	owners, err := runningOwnerProcesses([]string{name})
	if err != nil || len(owners) != 1 {
		t.Fatalf("child not observable: %v %v", owners, err)
	}
	app := RunningApp{Name: SameText("disposable fixture"), Processes: []string{name}}
	if still := CloseRunningAppsForCleanup([]RunningApp{app}, 20*time.Millisecond, nil); len(still) != 0 {
		t.Fatalf("windowless process survived cleanup close: %v", still)
	}
	if err := cmd.Wait(); err == nil || cmd.ProcessState.ExitCode() != 1 {
		t.Fatalf("expected verified termination exit 1: state=%v err=%v", cmd.ProcessState, err)
	}
}
