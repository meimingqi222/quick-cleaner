//go:build !windows

package platform

import "os"

// RemoveFileExclusive is a plain unlink here: POSIX has no share modes, so
// occupancy stays a target-level gate on these platforms.
func RemoveFileExclusive(path string) error { return os.Remove(path) }
