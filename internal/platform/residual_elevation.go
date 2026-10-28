package platform

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"
)

// Paths are data in argv, never AppleScript or shell source. One privileged
// shell invocation means one authorization prompt for the entire batch.
const residualRemoveScript = `on run argv
	set cmd to ""
	repeat with rawArg in argv
		set arg to rawArg as text
		if arg starts with "daemon:" then
			set p to text 8 thru -1 of arg
			set cmd to cmd & "/bin/launchctl bootout system " & quoted form of p & " >/dev/null 2>&1; /bin/rm -f " & quoted form of p & " >/dev/null 2>&1; "
		else if arg starts with "agent:" then
			set p to text 7 thru -1 of arg
			set cmd to cmd & "/bin/launchctl bootout gui/$(/usr/bin/id -u) " & quoted form of p & " >/dev/null 2>&1; /bin/rm -f " & quoted form of p & " >/dev/null 2>&1; "
		else
			set p to text 6 thru -1 of arg
			set cmd to cmd & "/bin/rm -rf " & quoted form of p & " >/dev/null 2>&1; "
		end if
	end repeat
	do shell script cmd & "exit 0" with administrator privileges
end run`

type residualAction uint8

const (
	residualDaemon residualAction = iota
	residualAgent
	residualRemove
)

func residualActionForPath(path string) residualAction {
	switch strings.ToLower(filepath.Dir(path)) {
	case "/library/launchdaemons":
		return residualDaemon
	case "/library/launchagents":
		return residualAgent
	default:
		return residualRemove
	}
}

type residualElevationOps struct {
	root bool
	uid  int
	run  func(context.Context, *exec.Cmd, time.Duration) (ProcessResult, bool)
}

// Authorization and object identity belong to core's callback, not this adapter.
// Injecting only the process boundary lets fixtures exercise the real planner and
// Lstat completion checks without invoking launchd or administrator authorization.
func removePrivilegedResiduals(ctx context.Context, paths []string, allowed func(string) bool, ops residualElevationOps) map[string]bool {
	removed := make(map[string]bool)
	if allowed == nil || ctx.Err() != nil {
		return removed
	}
	type target struct {
		path   string
		action residualAction
	}
	var planned []target
	for _, path := range paths {
		if allowed(path) {
			planned = append(planned, target{path, residualActionForPath(path)})
		}
	}
	sort.SliceStable(planned, func(i, j int) bool { return planned[i].action < planned[j].action })
	if len(planned) == 0 {
		return removed
	}
	if ops.root {
		for _, p := range planned {
			if ctx.Err() != nil {
				return removed
			}
			switch p.action {
			case residualDaemon:
				ops.run(ctx, exec.Command("/bin/launchctl", "bootout", "system", p.path), time.Minute)
				ops.run(ctx, exec.Command("/bin/rm", "-f", p.path), time.Minute)
			case residualAgent:
				ops.run(ctx, exec.Command("/bin/launchctl", "bootout", "gui/"+strconv.Itoa(ops.uid), p.path), time.Minute)
				ops.run(ctx, exec.Command("/bin/rm", "-f", p.path), time.Minute)
			default:
				ops.run(ctx, exec.Command("/bin/rm", "-rf", p.path), time.Minute)
			}
		}
	} else {
		args := []string{"-"}
		for _, p := range planned {
			tag := "path:"
			if p.action == residualDaemon {
				tag = "daemon:"
			} else if p.action == residualAgent {
				tag = "agent:"
			}
			args = append(args, tag+p.path)
		}
		cmd := exec.Command("/usr/bin/osascript", args...)
		cmd.Stdin = strings.NewReader(residualRemoveScript)
		// Nonzero exit is not proof of failure or success; a transport failure
		// (including cancellation/timeout) cannot produce a completion receipt.
		if _, complete := ops.run(ctx, cmd, 5*time.Minute); !complete {
			return removed
		}
	}
	for _, p := range planned {
		if _, err := os.Lstat(p.path); os.IsNotExist(err) {
			removed[p.path] = true
		}
	}
	return removed
}
