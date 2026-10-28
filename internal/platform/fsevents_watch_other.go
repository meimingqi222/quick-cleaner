//go:build !darwin

package platform

import "errors"

// WatchDirectories is a macOS FSEvents feature; other platforms report no watch.
func WatchDirectories(paths []string) (*FSDirWatch, error) {
	return nil, errors.New("directory watch is implemented for macOS only")
}
