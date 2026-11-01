package platform

import (
	"io"
	"os"
	"strings"
	"testing"
)

func TestMacWalkSpillReclaimLogUsesStderr(t *testing.T) {
	stdout, stdoutW, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	stderr, stderrW, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	oldOut, oldErr := os.Stdout, os.Stderr
	os.Stdout, os.Stderr = stdoutW, stderrW
	defer func() { os.Stdout, os.Stderr = oldOut, oldErr }()
	logReclaimedMacWalkSpill(2, 3<<20)
	if err := stdoutW.Close(); err != nil {
		t.Fatal(err)
	}
	if err := stderrW.Close(); err != nil {
		t.Fatal(err)
	}
	out, err := io.ReadAll(stdout)
	if err != nil {
		t.Fatal(err)
	}
	errOut, err := io.ReadAll(stderr)
	if err != nil {
		t.Fatal(err)
	}
	_ = stdout.Close()
	_ = stderr.Close()
	if string(out) != "" {
		t.Fatalf("stdout polluted by spill diagnostics: %q", out)
	}
	if !strings.Contains(string(errOut), "回收死进程溢写文件") {
		t.Fatalf("stderr missing spill diagnostics: %q", errOut)
	}
}
