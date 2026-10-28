package platform

import (
	"bytes"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

// These tests need only a signed test executable, never root, launchd or SMC.
func testFanNativeIdentity(t *testing.T) (string, string) {
	t.Helper()
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	out, err := exec.Command("/usr/bin/codesign", "-d", "--verbose=4", exe).CombinedOutput()
	if err != nil {
		if strings.Contains(string(out), "not signed at all") {
			t.Skip("sign the test binary (codesign -s -) to test the kernel CDHash")
		}
		t.Fatal(err, string(out))
	}
	hash, ok := fanSelfCDHash()
	if !ok {
		t.Fatal("signed executable has no kernel CDHash; check csops binding and CS_OPS_CDHASH")
	}
	if !strings.Contains(string(out), "CDHash="+hash+"\n") {
		t.Fatalf("kernel/disk identity mismatch: kernel=%s codesign=%s", hash, out)
	}
	return exe, hash
}

func TestFanHelperNativeCDHashMatchesCodesign(t *testing.T) {
	testFanNativeIdentity(t)
}

func TestFanHelperNativeSignatureGate(t *testing.T) {
	exe, hash := testFanNativeIdentity(t)
	own, err := os.ReadFile(exe)
	if err != nil {
		t.Fatal(err)
	}
	other, err := os.ReadFile("/bin/ls")
	if err != nil {
		t.Fatal(err)
	}
	tampered := bytes.Clone(own)
	if len(tampered) <= 40960 {
		t.Fatal("unexpectedly small test executable")
	}
	tampered[40960] ^= 0xff
	for _, sample := range []struct {
		name string
		body []byte
		pass bool
	}{
		{"good", own, true}, {"tampered", tampered, false}, {"other", other, false}, {"unsigned", []byte("#!/bin/sh\nid\n"), false},
	} {
		t.Run(sample.name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), sample.name)
			if err := os.WriteFile(path, sample.body, 0600); err != nil {
				t.Fatal(err)
			}
			verify, err := fanVerifySnippet(path, hash)
			if err != nil {
				t.Fatal(err)
			}
			out, err := exec.Command("/bin/sh", "-c", verify+" echo PASSED").CombinedOutput()
			if passed := err == nil && strings.Contains(string(out), "PASSED"); passed != sample.pass {
				t.Fatalf("passed=%v expected=%v err=%v output=%s", passed, sample.pass, err, out)
			}
			if !sample.pass {
				if _, err := os.Stat(path); !os.IsNotExist(err) {
					t.Fatal("rejected stage was not removed")
				}
			}
		})
	}
}
