package platform

import (
	"os"
	"syscall"
	"time"
)

func AppFileTimes(info os.FileInfo) (created, accessed time.Time) {
	if data, ok := info.Sys().(*syscall.Win32FileAttributeData); ok {
		timestamp := func(value syscall.Filetime) time.Time {
			ticks := uint64(value.HighDateTime)<<32 | uint64(value.LowDateTime)
			// FILETIME's range exceeds UnixNano's; keep seconds separate.
			return time.Unix(int64(ticks/10000000)-11644473600, int64(ticks%10000000)*100)
		}
		return timestamp(data.CreationTime), timestamp(data.LastAccessTime)
	}
	return info.ModTime(), info.ModTime()
}
