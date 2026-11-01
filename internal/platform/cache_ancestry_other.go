//go:build !windows

package platform

import (
	"errors"
	"os"

	"github.com/shirou/gopsutil/v4/process"
)

func cacheOutsideCleanerTree(pid uint32) error {
	ancestor, err := process.NewProcess(int32(pid))
	if err != nil {
		return err
	}
	for depth := 0; ; depth++ {
		if depth >= 256 || ancestor.Pid == int32(os.Getpid()) {
			return errors.New("cleaner-process-tree-or-unknown-ancestry")
		}
		parent, err := ancestor.Ppid()
		if err != nil {
			return err
		}
		if parent <= 1 {
			return nil
		}
		ancestor, err = process.NewProcess(parent)
		if err != nil {
			return err
		}
	}
}
