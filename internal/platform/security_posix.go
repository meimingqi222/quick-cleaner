//go:build darwin || linux

package platform

import "os"

func IsElevated() bool { return os.Geteuid() == 0 }

// macOS uses TCC and operation-specific authorization, not a root GUI relaunch.
func RelaunchAsAdminIfNeeded() bool { return true }
