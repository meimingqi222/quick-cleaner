package platform

import (
	"strings"
	"syscall"
	"testing"
	"unsafe"
)

func TestWindowsElevationArgumentNativeRoundtrip(t *testing.T) {
	args := []string{"", `C:\Users\original user\`, `a\"b`, "中文 路径", "line\nbreak", "vertical\vtab", "--orig-user-home", `D:\profile\`}
	quoted := []string{"qc.exe"}
	for _, arg := range args {
		quoted = append(quoted, QuoteWindowsArgument(arg))
	}
	line, err := syscall.UTF16PtrFromString(strings.Join(quoted, " "))
	if err != nil {
		t.Fatal(err)
	}
	var count int32
	argv, err := syscall.CommandLineToArgv(line, &count)
	if err != nil {
		t.Fatal(err)
	}
	defer syscall.LocalFree(syscall.Handle(unsafe.Pointer(argv)))
	if count != int32(len(args)+1) {
		t.Fatalf("got %d args, want %d", count, len(args)+1)
	}
	for i, want := range args {
		if got := syscall.UTF16ToString(argv[i+1][:]); got != want {
			t.Fatalf("arg %d: got %q, want %q", i, got, want)
		}
	}
}
