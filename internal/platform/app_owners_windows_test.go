package platform

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// The test binary is this user's own process: it must be listed with a
// verified identity and image path, matched case-insensitively by name.
func TestRunningAppOwnersListsOwnProcessByImageName(t *testing.T) {
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	owners, err := RunningAppOwners([]string{strings.ToUpper(filepath.Base(exe))})
	if err != nil {
		t.Fatal(err)
	}
	found := false
	for _, owner := range owners {
		if owner.PID == uint32(os.Getpid()) {
			found = true
			if owner.Identity == 0 || !strings.EqualFold(owner.Executable, exe) {
				t.Fatalf("unverified owner: %+v (want exe %s)", owner, exe)
			}
		}
	}
	if !found {
		t.Fatalf("own process not listed: %+v", owners)
	}
	if owners, err := RunningAppOwners([]string{"qc-no-such-process.exe"}); err != nil || len(owners) != 0 {
		t.Fatalf("absent image name: %+v %v", owners, err)
	}
}

// Services and other sessions are never offered for closing.
func TestRunningAppOwnersOmitsSystemProcesses(t *testing.T) {
	owners, err := RunningAppOwners([]string{"lsass.exe", "services.exe", "smss.exe"})
	if err != nil {
		t.Fatal(err)
	}
	if len(owners) != 0 {
		t.Fatalf("system processes offered for closing: %+v", owners)
	}
}
