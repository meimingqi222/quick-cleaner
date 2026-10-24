package platform

import (
	"bytes"
	"sync"

	"github.com/ebitengine/purego"
)

var darwinCacheSystem struct {
	sync.Once
	confstr func(int32, []byte, uintptr) uintptr
}

// DarwinUserCacheDir reads _CS_DARWIN_USER_CACHE_DIR, not ~/Library/Caches.
func DarwinUserCacheDir() (string, bool) {
	darwinCacheSystem.Do(func() {
		handle, err := purego.Dlopen("/usr/lib/libSystem.B.dylib", purego.RTLD_NOW|purego.RTLD_LOCAL)
		if err != nil {
			return
		}
		address, err := purego.Dlsym(handle, "confstr")
		if err == nil {
			purego.RegisterFunc(&darwinCacheSystem.confstr, address)
		}
	})
	if darwinCacheSystem.confstr == nil {
		return "", false
	}
	const csDarwinUserCacheDir = 65538
	length := darwinCacheSystem.confstr(csDarwinUserCacheDir, nil, 0)
	if length == 0 || length > 1024*1024 {
		return "", false
	}
	buffer := make([]byte, length)
	written := darwinCacheSystem.confstr(csDarwinUserCacheDir, buffer, uintptr(len(buffer)))
	if written == 0 || written > uintptr(len(buffer)) {
		return "", false
	}
	end := bytes.IndexByte(buffer[:written], 0)
	if end <= 0 {
		return "", false
	}
	return string(buffer[:end]), true
}
