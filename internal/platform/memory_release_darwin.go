package platform

import (
	"os/exec"
	"time"
)

func ReleaseMemory() bool {
	result, completed := RunCommandTree(exec.Command("/usr/bin/purge"), time.Minute)
	return completed && result.OK
}
