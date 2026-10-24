package platform

import (
	"os/exec"
	"strings"
	"testing"
	"time"
)

func TestDarwinUserCacheDirMatchesSystem(t *testing.T) {
	path, known := DarwinUserCacheDir()
	if !known || !strings.HasPrefix(path, "/") {
		t.Fatalf("native confstr returned %q known=%v", path, known)
	}
	run, ok := RunCommandTree(exec.Command("/usr/bin/getconf", "DARWIN_USER_CACHE_DIR"), 3*time.Second)
	if !ok || !run.OK || strings.TrimSpace(string(run.Stdout)) != path {
		t.Fatalf("confstr=%q getconf=%q ok=%v", path, run.Stdout, ok)
	}
}
