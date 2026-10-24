//go:build linux || darwin

package core

import (
	"os"
	"syscall"
)

func identityMetadata(id TargetIdentity, info os.FileInfo) (TargetIdentity, bool) {
	stat, ok := info.Sys().(*syscall.Stat_t)
	if !ok || stat == nil {
		return TargetIdentity{}, false
	}
	id.dev, id.ino = uint64(stat.Dev), stat.Ino
	return id, true
}

func captureStable(_ *TargetIdentity, _ string) {}

func (id TargetIdentity) recheckMetadata(_ string, info os.FileInfo) bool {
	stat, ok := info.Sys().(*syscall.Stat_t)
	return ok && stat != nil && uint64(stat.Dev) == id.dev && stat.Ino == id.ino
}
