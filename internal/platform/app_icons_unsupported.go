//go:build !windows && !darwin

package platform

// AppIconPNG returns nil when native icon acquisition is unsupported.
func AppIconPNG(path string) []byte { return nil }

// AppIconFromBundle returns nil on unsupported platforms.
func AppIconFromBundle(path string) []byte { return nil }
