package platform

import (
	"bytes"
	"context"
	"io"
	"os"
	"os/exec"
	"time"
)

// ProcessResult distinguishes normal nonzero exits from transport failures and signals.
type ProcessResult struct {
	Stdout, Stderr []byte
	ExitCode       *int
	OK             bool
}

// RunCommandTree owns cmd's output streams. The deadline includes descendant-held
// pipes, not just the direct child's lifetime. A command must not already be started.
func RunCommandTree(cmd *exec.Cmd, timeout time.Duration) (ProcessResult, bool) {
	return RunCommandTreeContext(context.Background(), cmd, timeout)
}

// Cancellation remains active until both pipes close, including when the direct
// child has exited but descendants still own its output streams.
func RunCommandTreeContext(ctx context.Context, cmd *exec.Cmd, timeout time.Duration) (ProcessResult, bool) {
	if cmd == nil || timeout <= 0 || cmd.Process != nil || ctx.Err() != nil {
		return ProcessResult{}, false
	}
	bounded, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()
	deadline, _ := bounded.Deadline()
	outR, outW, err := os.Pipe()
	if err != nil {
		return ProcessResult{}, false
	}
	defer outR.Close()
	defer outW.Close()
	errR, errW, err := os.Pipe()
	if err != nil {
		return ProcessResult{}, false
	}
	defer errR.Close()
	defer errW.Close()
	cmd.Stdout, cmd.Stderr = outW, errW
	kill, err := startCommandTree(cmd)
	if err != nil {
		return ProcessResult{}, false
	}
	defer kill()
	outW.Close()
	errW.Close()
	var out, stderr bytes.Buffer
	reads := make(chan error, 2)
	go func() { _, e := io.Copy(&out, outR); reads <- e }()
	go func() { _, e := io.Copy(&stderr, errR); reads <- e }()
	wait := make(chan error, 1)
	go func() { wait <- cmd.Wait() }()
	finished, count, valid := false, 0, true
	for !finished || count < 2 {
		select {
		case e := <-wait:
			finished = true
			if e != nil {
				if _, ok := e.(*exec.ExitError); !ok {
					valid = false
				}
			}
		case e := <-reads:
			count++
			if e != nil {
				valid = false
			}
		case <-bounded.Done():
			kill()
			outR.Close()
			errR.Close()
			if !finished {
				<-wait
			}
			for count < 2 {
				<-reads
				count++
			}
			return ProcessResult{}, false
		}
	}
	if !valid || bounded.Err() != nil || !time.Now().Before(deadline) {
		return ProcessResult{}, false
	}
	result := ProcessResult{Stdout: out.Bytes(), Stderr: stderr.Bytes(), OK: cmd.ProcessState.Success()}
	if code := cmd.ProcessState.ExitCode(); code != -1 {
		result.ExitCode = &code
	}
	return result, true
}
