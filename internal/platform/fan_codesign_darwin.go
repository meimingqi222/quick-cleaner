package platform

import (
	"encoding/hex"
	"os"
	"sync"

	"github.com/ebitengine/purego"
)

var fanCodeSigning struct {
	sync.Once
	csops func(int32, uint32, *byte, uintptr) int32
	err   error
}

// Only the kernel's CS_OPS_CDHASH (5, NOT CS_CDHASH_LEN=20) identifies
// the loaded image. SecCodeCopySelf and disk hashes follow a replaceable path.
func fanSelfCDHash() (string, bool) {
	fanCodeSigning.Do(func() {
		var handle, symbol uintptr
		handle, fanCodeSigning.err = purego.Dlopen("/usr/lib/libSystem.B.dylib", purego.RTLD_NOW|purego.RTLD_LOCAL)
		if fanCodeSigning.err != nil {
			return
		}
		symbol, fanCodeSigning.err = purego.Dlsym(handle, "csops")
		if fanCodeSigning.err != nil {
			return
		}
		purego.RegisterFunc(&fanCodeSigning.csops, symbol)
	})
	if fanCodeSigning.err != nil {
		return "", false
	}
	var hash [20]byte
	if fanCodeSigning.csops(int32(os.Getpid()), 5, &hash[0], uintptr(len(hash))) != 0 || hash == [20]byte{} {
		return "", false
	}
	return hex.EncodeToString(hash[:]), true
}
