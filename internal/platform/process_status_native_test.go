//go:build darwin || windows

package platform

import (
	"context"
	"net"
	"os"
	"slices"
	"testing"
)

func TestProcessPortsMapFindsOwnTCPAndUDP(t *testing.T) {
	tcp, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer tcp.Close()
	udp, err := net.ListenPacket("udp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer udp.Close()
	ports := ProcessPortsMap()[uint32(os.Getpid())]
	for _, port := range []int{tcp.Addr().(*net.TCPAddr).Port, udp.LocalAddr().(*net.UDPAddr).Port} {
		if !slices.Contains(ports, uint16(port)) {
			t.Fatalf("own port %d missing from %v", port, ports)
		}
	}
	if !slices.IsSorted(ports) || len(slices.Compact(slices.Clone(ports))) != len(ports) {
		t.Fatal("ports must be sorted/unique", ports)
	}
	if threads := ProcessThreadsMap()[uint32(os.Getpid())]; threads == 0 {
		t.Fatal("own thread count unavailable")
	}
	if state := ProcessStatusText(context.Background(), uint32(os.Getpid())); state != "Run" && state != "Sleep" {
		t.Fatalf("own process state unavailable: %s", state)
	}
}
