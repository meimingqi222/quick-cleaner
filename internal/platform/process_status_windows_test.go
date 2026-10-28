package platform

import (
	"bufio"
	"context"
	"fmt"
	"os"
	"os/exec"
	"syscall"
	"testing"
	"time"
)

func TestProcessStatusWindowsChild(t *testing.T) {
	if os.Getenv("QC_STATUS_PROCESS_CHILD") != "1" {
		return
	}
	fmt.Println("ready")
	time.Sleep(60 * time.Second)
}

func TestWindowsProcessTerminationUsesCreationTime(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 8*time.Second)
	defer cancel()
	cmd := exec.CommandContext(ctx, os.Args[0], "-test.run=^TestProcessStatusWindowsChild$")
	cmd.Env = append(os.Environ(), "QC_STATUS_PROCESS_CHILD=1")
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	if err = cmd.Start(); err != nil {
		t.Fatal(err)
	}
	defer cmd.Process.Kill()
	line, err := bufio.NewReader(stdout).ReadString('\n')
	if err != nil || line != "ready\n" {
		t.Fatal(line, err)
	}
	pid := uint32(cmd.Process.Pid)
	h, err := syscall.OpenProcess(0x1000, false, pid)
	if err != nil {
		t.Fatal(err)
	}
	var created, exited, kernel, user syscall.Filetime
	err = syscall.GetProcessTimes(h, &created, &exited, &kernel, &user)
	syscall.CloseHandle(h)
	if err != nil {
		t.Fatal(err)
	}
	start := (uint64(created.HighDateTime)<<32|uint64(created.LowDateTime))/10_000_000 - 11_644_473_600
	if TerminateProcess(pid, start+1, nil) == nil {
		t.Fatal("mismatched creation time allowed termination")
	}
	if err := ForceTerminateProcess(pid, start, nil); err != nil {
		t.Fatal(err)
	}
	if err := cmd.Wait(); err == nil || cmd.ProcessState.ExitCode() != 1 || ctx.Err() != nil {
		t.Fatal("expected TerminateProcess exit 1 before deadline", err, cmd.ProcessState, ctx.Err())
	}
}
