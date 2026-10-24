package platform

import (
	"bytes"
	"context"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"time"

	"github.com/ebitengine/purego"
	"github.com/ebitengine/purego/objc"
)

func cacheRejectMacLinks(path string) error {
	if !filepath.IsAbs(path) {
		return errors.New("cache-path-not-absolute")
	}
	for {
		info, err := os.Lstat(path)
		if err != nil {
			return err
		}
		if info.Mode()&os.ModeSymlink != 0 {
			return errors.New("cache-owner-probe-link")
		}
		parent := filepath.Dir(path)
		if parent == path {
			return nil
		}
		path = parent
	}
}

func CacheReleaseOccupants(path string) ([]CacheOccupant, error) {
	if err := cacheRejectMacLinks(path); err != nil {
		return nil, err
	}
	root, err := filepath.EvalSymlinks(path)
	if err != nil {
		return nil, err
	}
	result, ok := macInventoryCommand(context.Background(), 30*time.Second, "/usr/sbin/lsof", "-F0pn", "+D", root)
	if !ok || result.ExitCode == nil || (*result.ExitCode != 0 && *result.ExitCode != 1) || len(bytes.TrimSpace(result.Stderr)) != 0 {
		return nil, errors.New("cache-owner-probe-incomplete")
	}
	records, err := parseCacheLsof(result.Stdout, root)
	if err != nil {
		return nil, err
	}
	if len(records) > 16 {
		return nil, errors.New("too-many-cache-owners")
	}
	owners := make([]CacheOccupant, 0, len(records))
	for pid, evidence := range records {
		id := ProcessUniqueID(pid)
		if id == nil {
			return nil, errors.New("owner-identity-unknown")
		}
		name, exe, err := cacheProcessSnapshot(pid)
		if err != nil {
			return nil, err
		}
		again := ProcessUniqueID(pid)
		if again == nil || *again != *id {
			return nil, errors.New("owner-identity-changed")
		}
		owners = append(owners, CacheOccupant{PID: pid, Identity: *id, Executable: exe, Name: name, Evidence: evidence})
	}
	sort.Slice(owners, func(i, j int) bool { return owners[i].PID < owners[j].PID })
	return owners, nil
}

func RequestCacheOwnerExit(owner CacheOccupant, force bool) error {
	if owner.Identity == 0 {
		return errors.New("owner-identity-unknown")
	}
	if err := ensureSameStatusProcess(owner.PID, &owner.Identity); err != nil {
		return err
	}
	_, exe, err := cacheProcessSnapshot(owner.PID)
	if err != nil {
		return err
	}
	if exe != owner.Executable {
		return errors.New("owner-executable-changed")
	}
	if force {
		return ForceTerminateProcess(owner.PID, 0, &owner.Identity)
	}
	macWorkspace.Lock()
	defer macWorkspace.Unlock()
	macWorkspace.once.Do(func() {
		_, macWorkspace.err = purego.Dlopen("/System/Library/Frameworks/AppKit.framework/AppKit", purego.RTLD_NOW|purego.RTLD_GLOBAL)
	})
	if macWorkspace.err != nil {
		return macWorkspace.err
	}
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	pool := objc.ID(objc.GetClass("NSAutoreleasePool")).Send(objc.RegisterName("new"))
	if pool == 0 {
		return errors.New("cache-quit-pool-unavailable")
	}
	defer pool.Send(objc.RegisterName("drain"))
	app := objc.ID(objc.GetClass("NSRunningApplication")).Send(objc.RegisterName("runningApplicationWithProcessIdentifier:"), int32(owner.PID))
	// A CLI gets no implicit SIGTERM. Only a separately confirmed force can kill it.
	if app == 0 {
		return nil
	}
	if err := ensureSameStatusProcess(owner.PID, &owner.Identity); err != nil {
		return err
	}
	_ = objc.Send[bool](app, objc.RegisterName("terminate"))
	return nil
}
