package core

import (
	"os/exec"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type ProcRun = platform.ProcessResult

func RunWithTimeout(program string, args []string, timeout time.Duration) (ProcRun, bool) {
	return platform.RunCommandTree(exec.Command(program, args...), timeout)
}

func RunToolWithTimeout(program string, args []string, timeout time.Duration) (ProcRun, bool) {
	cmd, ok := platform.ToolCommand(program, args)
	if !ok {
		return ProcRun{}, false
	}
	return platform.RunCommandTree(cmd, timeout)
}
