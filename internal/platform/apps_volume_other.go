//go:build !darwin

package platform

// macOnDataVolume is only meaningful on macOS; elsewhere the question does not
// arise and the probe reports "unknown", which keeps every registered path live.
func macOnDataVolume(string) (same bool, known bool) { return false, false }
