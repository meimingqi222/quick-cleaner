package platform

import (
	"os"
	"syscall"
	"testing"
	"time"
)

type appTimesFileInfo struct {
	os.FileInfo
	data syscall.Win32FileAttributeData
}

func (info appTimesFileInfo) Sys() any { return &info.data }

func TestAppFileTimesPreservesFiletimeRange(t *testing.T) {
	const epoch = uint64(116444736000000000)
	info := appTimesFileInfo{data: syscall.Win32FileAttributeData{
		CreationTime:   syscall.Filetime{},
		LastAccessTime: syscall.Filetime{LowDateTime: uint32((epoch + 9) & 0xffffffff), HighDateTime: uint32((epoch + 9) >> 32)},
	}}
	created, accessed := AppFileTimes(info)
	if !created.Equal(time.Date(1601, time.January, 1, 0, 0, 0, 0, time.UTC)) || !accessed.Equal(time.Unix(0, 900)) {
		t.Fatalf("FILETIME range/100 ns precision lost: created=%s accessed=%s", created, accessed)
	}
}
