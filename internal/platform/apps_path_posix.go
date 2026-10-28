//go:build darwin || linux

package platform

import (
	"bytes"
	"runtime"
	"strings"
	"sync"

	"github.com/ebitengine/purego"
	"golang.org/x/sys/unix"
)

var appPathSystem struct {
	sync.Once
	realpath func(string, []byte) *byte
}

// CanonicalAppPath uses the native canonical spelling. EvalSymlinks preserves
// input casing on case-insensitive APFS and cannot deduplicate those aliases.
func CanonicalAppPath(path string) (string, bool) {
	if path == "" || strings.ContainsRune(path, 0) {
		return "", false
	}
	appPathSystem.Do(func() {
		library := "libc.so.6"
		if runtime.GOOS == "darwin" {
			library = "/usr/lib/libSystem.B.dylib"
		}
		handle, err := purego.Dlopen(library, purego.RTLD_NOW|purego.RTLD_LOCAL)
		if err != nil {
			return
		}
		address, err := purego.Dlsym(handle, "realpath")
		if err == nil {
			purego.RegisterFunc(&appPathSystem.realpath, address)
		}
	})
	if appPathSystem.realpath == nil {
		return "", false
	}
	buffer := make([]byte, unix.PathMax)
	if appPathSystem.realpath(path, buffer) == nil {
		return "", false
	}
	end := bytes.IndexByte(buffer, 0)
	if end <= 0 {
		return "", false
	}
	return string(buffer[:end]), true
}
