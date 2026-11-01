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
	if !ok || now.isDir != id.isDir {
		return false
	}
	// Directories with a strong volume+file-index identity match Unix dev+ino
	// semantics: background cache/log file churn updates directory mtime without
	// replacing the directory object or introducing reparse points.
	if now.isDir && id.hasStable {
		stable, ok := platform.ObjectID(path)
		return ok && stable == id.stable
	}
	if now.mtime != id.mtime || now.length != id.length {
		return false
	}
	if !id.hasStable {
		return true
	}
	stable, ok := platform.ObjectID(path)
	return ok && stable == id.stable
}
