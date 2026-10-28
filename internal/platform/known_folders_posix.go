//go:build darwin || linux

package platform

// KnownFolders is Windows-only; POSIX has no User Shell Folders registry.
func KnownFolders() []string { return nil }
