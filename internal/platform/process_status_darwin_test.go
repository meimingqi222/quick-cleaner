package platform

import (
	"bufio"
	"context"
	"os"
	"os/exec"
	"syscall"
	"testing"
	"time"
)

func TestProcessUniqueIDAndTerminationRequireSameIdentity(t *testing.T) {
	pid := uint32(os.Getpid())
	id := ProcessUniqueID(pid)
	if id == nil || *id == 0 {
		t.Fatal("own Darwin identity unavailable")
	}
	again := ProcessUniqueID(pid)
	if again == nil || *again != *id {
		t.Fatal("identity not stable")
	}
	wrong := *id + 1
	for _, kill := range []func(uint32, uint64, *uint64) error{TerminateProcess, ForceTerminateProcess} {
		if kill(pid, 0, nil) == nil || kill(pid, 0, &wrong) == nil {
			t.Fatal("unknown/replaced identity authorized termination")
		}
	}
}

func TestForceTerminationKillsTERMImmuneChild(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 8*time.Second)
	defer cancel()
	cmd := exec.CommandContext(ctx, "/bin/sh", "-c", "trap '' TERM; echo ready; exec sleep 60")
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
	id := ProcessUniqueID(pid)
	if id == nil {
		t.Fatal("child identity unavailable")
	}
	if err := TerminateProcess(pid, 0, id); err != nil {
		t.Fatal(err)
	}
	if err := syscall.Kill(int(pid), 0); err != nil {
		t.Fatal("TERM-immune child exited", err)
	}
	if err := ForceTerminateProcess(pid, 0, id); err != nil {
		t.Fatal(err)
	}
	if err := cmd.Wait(); err == nil {
		t.Fatal("force termination reported normal exit")
	}
	status := cmd.ProcessState.Sys().(syscall.WaitStatus)
	if !status.Signaled() || status.Signal() != syscall.SIGKILL || ctx.Err() != nil {
		t.Fatal("expected SIGKILL before deadline", status, ctx.Err())
	}
}
