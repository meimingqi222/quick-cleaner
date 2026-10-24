package core

import (
	"os"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func identityMetadata(id TargetIdentity, _ os.FileInfo) (TargetIdentity, bool) {
	return id, true
}

func captureStable(id *TargetIdentity, path string, lstat func(string) (os.FileInfo, error)) {
	if lstat == nil {
		id.stable, id.hasStable = platform.ObjectID(path)
		return
	}
	id.stable, id.hasStable = platform.ObjectIDUsing(path, lstat)
}

func (id TargetIdentity) recheckMetadata(path string, info os.FileInfo) bool {
	now, ok := IdentityFromMetadata(info)
	if !ok || now.mtime != id.mtime || now.length != id.length {
		return false
	}
	if !id.hasStable {
		return true
	}
	stable, ok := platform.ObjectID(path)
	return ok && stable == id.stable
}
