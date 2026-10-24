package platform

import (
	"os"
	"syscall"
	"time"
)

func AppFileTimes(info os.FileInfo) (created, accessed time.Time) {
	if data, ok := info.Sys().(*syscall.Stat_t); ok {
		return time.Unix(data.Birthtimespec.Sec, data.Birthtimespec.Nsec), time.Unix(data.Atimespec.Sec, data.Atimespec.Nsec)
	}
	return info.ModTime(), info.ModTime()
}
