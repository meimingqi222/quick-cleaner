package core

import (
	"os"
	"time"
)

// TargetIdentity is a scan-time snapshot, not authorization to delete a path.
type TargetIdentity struct {
	dev, ino  uint64
	mtime     int64
	length    int64
	stable    [2]uint64
	hasStable bool
}

// IdentityFromMetadata reuses metadata without opening another handle. On
// Windows this deliberately provides only the weak timestamp/length identity.
// FileInfo has no timestamp error channel; a zero ModTime is treated as
// unavailable, including for synthetic FileInfo implementations.
func IdentityFromMetadata(info os.FileInfo) (TargetIdentity, bool) {
	if info == nil || info.ModTime().IsZero() {
		return TargetIdentity{}, false
	}
	modified := info.ModTime()
	seconds := modified.Unix()
	// Rust truncates the magnitude before negating pre-epoch timestamps.
	if modified.Before(time.Unix(0, 0)) && modified.Nanosecond() != 0 {
		seconds++
	}
	id := TargetIdentity{mtime: seconds, length: info.Size()}
	return identityMetadata(id, info)
}

// CaptureIdentity uses Lstat: the final symlink itself is the target.
// Unavailable Windows object IDs retain the metadata-only fallback.
func CaptureIdentity(path string) (TargetIdentity, bool) {
	return captureIdentityUsing(path, nil)
}

func captureIdentityUsing(path string, lstat func(string) (os.FileInfo, error)) (TargetIdentity, bool) {
	if lstat == nil {
		lstat = os.Lstat
	}
	info, err := lstat(path)
	if err != nil {
		return TargetIdentity{}, false
	}
	id, ok := IdentityFromMetadata(info)
	if ok {
		captureStable(&id, path, lstat)
	}
	return id, ok
}

// Recheck fails closed on inaccessible or missing paths.
func (id TargetIdentity) Recheck(path string) bool {
	info, err := os.Lstat(path)
	return err == nil && id.recheckMetadata(path, info)
}
