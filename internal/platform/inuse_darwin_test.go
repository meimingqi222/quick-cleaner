package platform

import (
	"os"
	"reflect"
	"testing"
)

func TestInUseLsofParsing(t *testing.T) {
	raw := []byte("p100\x00n/self\x00\np200\x00n/child\x00\np300\x00n/cache/open\x00nsocket\x00\n")
	if !inUseComplete(raw) || inUseComplete([]byte("p300\x00n/truncated")) {
		t.Fatal("completeness")
	}
	got := inUsePaths(raw, map[int]bool{100: true, 200: true})
	if !reflect.DeepEqual(got, []string{"/cache/open"}) {
		t.Fatalf("paths %q", got)
	}
	self := os.Getpid()
	excluded := inUseExcluded([]inUseProcess{{pid: self + 1, parent: self}, {pid: self + 2, parent: self + 1}})
	if !excluded[self+2] {
		t.Fatal("descendant exclusion")
	}
}
func TestInUseAppCandidates(t *testing.T) {
	procs := []inUseProcess{{command: "/applications/notion.app/contents/macos/notion"}, {command: "/applications/helper.app/x"}}
	if inUseOwningApp("/cache/notion.id.ShipIt", procs) != "notion" {
		t.Fatal("shell stripping")
	}
	if inUseOwningApp("/cache/helper", procs) != "" {
		t.Fatal("generic candidate")
	}
}
