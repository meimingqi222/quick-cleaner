//go:build !darwin

package platform

import "context"

// BundleIsStillInstalled is a macOS LaunchServices probe. Other platforms
// answer unknown so callers stay on their conservative path.
func BundleIsStillInstalled(context.Context, string) (installed bool, known bool) {
	return false, false
}
