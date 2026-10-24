package platform

import (
	"context"
	"os"
	"path/filepath"

	"golang.org/x/sys/unix"
)

// NeedsResidualElevation checks parent writability using the real UID, except
// that effective root can delete directly. Core must separately route authorized
// protected residuals here even when this returns false; this is not authorization.
func NeedsResidualElevation(path string) bool {
	if os.Geteuid() == 0 || path == "" || path == "/" {
		return false
	}
	return unix.Access(filepath.Dir(path), unix.W_OK) != nil
}

// RemovePrivilegedResiduals removes only paths approved by core's safety/identity
// callback, including when already root. Only Lstat NotFound proves completion.
func RemovePrivilegedResiduals(ctx context.Context, paths []string, allowed func(string) bool) map[string]bool {
	return removePrivilegedResiduals(ctx, paths, allowed, residualElevationOps{
		root: os.Geteuid() == 0, uid: os.Getuid(), run: RunCommandTreeContext,
	})
}
