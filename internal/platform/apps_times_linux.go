package platform

import (
	"os"
	"syscall"
	"time"
)

func AppFileTimes(info os.FileInfo) (created, accessed time.Time) {
	if data, ok := info.Sys().(*syscall.Stat_t); ok {
		return info.ModTime(), time.Unix(data.Atim.Sec, data.Atim.Nsec)
	}
	return info.ModTime(), info.ModTime()
}
