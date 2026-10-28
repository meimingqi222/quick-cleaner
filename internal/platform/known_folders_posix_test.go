//go:build darwin || linux

package platform

import "testing"

func TestKnownFoldersPOSIX(t *testing.T) {
	if got := KnownFolders(); got != nil {
		t.Fatalf("KnownFolders() = %v, want nil", got)
	}
}
