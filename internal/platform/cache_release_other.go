//go:build !windows && !darwin

package platform

import "errors"

func CacheReleaseOccupants(string) ([]CacheOccupant, error) {
	return nil, errors.New("cache-release-unsupported")
}
func RequestCacheOwnerExit(CacheOccupant, bool) error { return errors.New("cache-release-unsupported") }
