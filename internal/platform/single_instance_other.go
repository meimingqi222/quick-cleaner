//go:build !windows

package platform

// macOS and Linux use MyGo's per-user single-instance socket from the app
// layer; there is no elevated copy to bridge there.

func InstanceRunning() bool     { return false }
func AcquireInstance() bool     { return true }
func WakeRunningInstance() bool { return false }
func OnInstanceWake(func())     {}
