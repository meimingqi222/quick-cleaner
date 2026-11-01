//go:build !windows

package platform

// RunningAppOwners has no declared owners outside Windows yet; the browser
// owner lists only cover Windows User Data roots.
func RunningAppOwners([]string) ([]CacheOccupant, error) { return nil, nil }
