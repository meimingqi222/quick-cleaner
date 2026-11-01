//go:build !windows

package platform

import "errors"

// The macOS scan-time attribution already reports app names through DetectInUse;
// residual rows fall back to that evidence instead of running a second lsof pass.
func occupancyOwners(string) ([]Occupant, error) {
	return nil, errors.New("occupancy-owners-unsupported")
}

func findLockedCacheOwners([]string) ([]LockedTargetOwner, error) {
	return nil, nil
}
