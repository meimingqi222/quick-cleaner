//go:build !darwin

package platform

import "context"

// NeedsResidualElevation is macOS-specific.
func NeedsResidualElevation(string) bool { return false }

// RemovePrivilegedResiduals is unsupported outside macOS and fails closed.
func RemovePrivilegedResiduals(context.Context, []string, func(string) bool) map[string]bool {
	return map[string]bool{}
}
