package platform

import (
	"errors"
	"net"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestFanHelperNativePeerIdentity(t *testing.T) {
	// Darwin sockaddr_un paths are limited to 104 bytes; t.TempDir includes
	// this long test name beneath an already long /var/folders prefix.
	dir, err := os.MkdirTemp("", "qcfan-")
	if err != nil {
		t.Fatal(err)
	}
	defer os.RemoveAll(dir)
	path := filepath.Join(dir, "peer.sock")
	listener, err := net.ListenUnix("unix", &net.UnixAddr{Name: path, Net: "unix"})
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	if err := listener.SetDeadline(time.Now().Add(time.Second)); err != nil {
		t.Fatal(err)
	}
	client, err := net.Dial("unix", path)
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close()
	server, err := listener.AcceptUnix()
	if err != nil {
		t.Fatal(err)
	}
	defer server.Close()
	uid, err := fanPeerUID(server)
	if err != nil || uid != uint32(os.Geteuid()) {
		t.Fatalf("peer uid=%d want=%d err=%v", uid, os.Geteuid(), err)
	}
}

func TestFanHelperNativeEntriesRejectNonRootBeforeSideEffects(t *testing.T) {
	if os.Geteuid() == 0 {
		t.Skip("must not invoke helper entry points in a root test process")
	}
	for _, entry := range []func() error{RunFanHelperDaemon, RunFanHelperInstallStep} {
		var fanErr FanError
		if err := entry(); !errors.As(err, &fanErr) || fanErr.Kind != FanNeedsRoot {
			t.Fatalf("root-only entry returned %v", err)
		}
	}
}
