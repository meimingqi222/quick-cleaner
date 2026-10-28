//go:build darwin

package platform

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"syscall"
	"time"

	"github.com/ebitengine/purego"
	"github.com/ebitengine/purego/objc"
)

var trashFoundation struct {
	sync.Once
	err error
}

// MoveToTrash uses Cocoa's restorable trash operation, including cross-volume
// handling and collision naming. Native errors never authorize permanent delete.
func MoveToTrash(path string) error {
	err := moveToTrashWithFileManager(path)
	if err == nil {
		return nil
	}
	p, pathError := trashPath(path)
	if pathError != nil {
		return pathError
	}
	// Pass the path as argv, never as AppleScript source. Finder owns its TCC
	// authorization prompt; a bounded failure still cannot become permanent delete.
	cmd := exec.Command("/usr/bin/osascript", "-", p)
	cmd.Stdin = strings.NewReader("on run argv\nset targetPath to POSIX file (item 1 of argv)\ntell application \"Finder\" to delete targetPath\nend run")
	result, complete := RunCommandTree(cmd, 60*time.Second)
	if complete && result.OK {
		if _, e := os.Lstat(p); os.IsNotExist(e) {
			return nil
		}
	}
	return fmt.Errorf("%w; Finder trash failed or timed out", err)
}

func moveToTrashWithFileManager(path string) error {
	p, err := trashPath(path)
	if err != nil {
		return err
	}
	trashFoundation.Do(func() {
		_, trashFoundation.err = purego.Dlopen("/System/Library/Frameworks/Foundation.framework/Foundation", purego.RTLD_NOW|purego.RTLD_GLOBAL)
	})
	if trashFoundation.err != nil {
		return trashFoundation.err
	}
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	pool := objc.ID(objc.GetClass("NSAutoreleasePool")).Send(objc.RegisterName("new"))
	defer pool.Send(objc.RegisterName("drain"))
	s := objc.ID(objc.GetClass("NSString")).Send(objc.RegisterName("alloc"))
	s = s.Send(objc.RegisterName("initWithUTF8String:"), p)
	if s == 0 {
		return errors.New("cannot encode trash path")
	}
	defer s.Send(objc.RegisterName("release"))
	u := objc.ID(objc.GetClass("NSURL")).Send(objc.RegisterName("fileURLWithPath:"), s)
	fm := objc.ID(objc.GetClass("NSFileManager")).Send(objc.RegisterName("defaultManager"))
	var nativeError objc.ID
	if u == 0 || fm == 0 {
		return errors.New("NSFileManager unavailable")
	}
	if !objc.Send[bool](fm, objc.RegisterName("trashItemAtURL:resultingItemURL:error:"), u, uintptr(0), &nativeError) {
		return fmt.Errorf("NSFileManager trash failed (NSError %x)", uintptr(nativeError))
	}
	return nil
}

func trashDirectories() []string {
	home, ok := UserHome()
	if !ok {
		return nil
	}
	return []string{filepath.Join(home, ".Trash")}
}

func EmptyTrashShell() error { return nil }
func currentUID() string     { return strconv.Itoa(os.Getuid()) }

// IsVolumeTrash reports a current-user trash directory on a mounted volume.
// Those are not ~/.Trash, so emptying the home trash does not cover them.
func IsVolumeTrash(path string) bool {
	parent := filepath.Dir(path)
	return filepath.Base(parent) == ".Trashes" && filepath.Base(path) == currentUID() && strings.HasPrefix(path, "/Volumes/")
}

func IsPerDriveTrash(string) bool { return false }
func IsSystemTrash(path string) bool {
	dirs := TrashDirectories()
	return len(dirs) == 1 && path == dirs[0]
}
func IsRecycleJunkEntry(string) bool { return true }
func ForceDeleteAccess(string) bool  { return false }
func AllocatedFileSize(info os.FileInfo) uint64 {
	if info != nil {
		if s, ok := info.Sys().(*syscall.Stat_t); ok && s.Blocks >= 0 {
			return uint64(s.Blocks) * 512
		}
	}
	return logicalFileSize(info)
}
