package core_test

import (
	"runtime"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func TestVirtualPaths(t *testing.T) {
	for _, ref := range []string{"", "com.apple.TimeMachine.2024-01-15-123456", "nginx:1.25", "ghcr.io/owner/img:1.0", "sha256:a1b2", "a//b/.././c:tag", `a\b`, " 空 格 /?%#\n"} {
		for _, pair := range []struct {
			make  func(string) string
			parse func(string) (string, bool)
		}{{core.SnapshotPath, core.SnapshotName}, {core.DockerImagePath, core.DockerRmiRef}} {
			p := pair.make(ref)
			got, ok := pair.parse(p)
			if !ok || got != ref || !core.IsVirtualPath(p) {
				t.Fatalf("roundtrip %q: %q %v", p, got, ok)
			}
		}
	}
	for _, p := range []string{"/Users/test/Library/Caches", "docker:/image/x", "docker://images/x", "TMUTIL://snapshot/x", "brew://anything", "brew://cleanup/", "brew://cleanup?x", "brew://cleanup/../cleanup"} {
		if core.IsVirtualPath(p) {
			t.Errorf("virtual: %q", p)
		}
		if _, ok := core.SnapshotName(p); ok {
			t.Errorf("snapshot: %q", p)
		}
		if _, ok := core.DockerRmiRef(p); ok {
			t.Errorf("docker: %q", p)
		}
	}
	if !core.IsVirtualPath("brew://cleanup") {
		t.Fatal("brew exact target rejected")
	}
	if _, ok := core.SnapshotName(core.DockerImagePath("x")); ok {
		t.Fatal("cross-kind parse")
	}
	if _, ok := core.DockerRmiRef(core.SnapshotPath("x")); ok {
		t.Fatal("cross-kind parse")
	}
}

func TestCheckFromCounts(t *testing.T) {
	for _, c := range []struct {
		s, n uint
		want core.Check
	}{{0, 0, core.Off}, {9, 0, core.Off}, {0, 5, core.Off}, {2, 5, core.Partial}, {5, 5, core.On}, {6, 5, core.On}} {
		if got := core.CheckFromCounts(c.s, c.n); got != c.want {
			t.Errorf("%+v: %v", c, got)
		}
	}
}

func TestFormatSizes(t *testing.T) {
	base := uint64(1000)
	if runtime.GOOS == "windows" {
		base = 1024
	}
	for _, f := range []struct {
		fn   func(uint64) string
		base uint64
	}{{core.FmtSize, base}, {core.FmtMem, 1024}} {
		b := f.base
		for _, c := range []struct {
			n    uint64
			want string
		}{{0, "0 B"}, {500, "500 B"}, {b - 1, map[bool]string{true: "1023 B", false: "999 B"}[b == 1024]}, {b, "1 KB"}, {2 * b, "2 KB"}, {b*b - 1, "1000 KB"}, {15 * b * b, "15.0 MB"}, {b * b * b, "1.00 GB"}, {2 * b * b * b, "2.00 GB"}, {b * b * b * b, "1.00 TB"}, {b * b * b * b * 3 / 2, "1.50 TB"}} {
			want := c.want
			if c.n == b*b-1 && b == 1024 {
				want = "1024 KB"
			}
			if got := f.fn(c.n); got != want {
				t.Errorf("base %d bytes %d: %q != %q", b, c.n, got, want)
			}
		}
	}
	for _, c := range []struct {
		n    uint64
		want string
	}{{16 << 30, "16.00 GB"}, {32 << 30, "32.00 GB"}, {1536, "2 KB"}, {2560, "2 KB"}, {(1 << 30) + (1 << 28), "1.25 GB"}, {(1 << 40) - 1, "1024.00 GB"}, {(1 << 20) + (1 << 18), "1.2 MB"}, {^uint64(0), "16777216.00 TB"}} {
		if got := core.FmtMem(c.n); got != c.want {
			t.Errorf("memory %d: %s", c.n, got)
		}
	}
	if runtime.GOOS != "windows" && core.FmtSize(32<<30) != "34.36 GB" {
		t.Fatal("disk must differ from memory")
	}
}

func TestCommas(t *testing.T) {
	for n, want := range map[uint64]string{0: "0", 100: "100", 999: "999", 1000: "1,000", 1000000: "1,000,000", ^uint64(0): "18,446,744,073,709,551,615"} {
		if got := core.Commas(n); got != want {
			t.Errorf("%d: %s", n, got)
		}
	}
}

func TestTruncate(t *testing.T) {
	for _, c := range []struct {
		s    string
		n    uint
		want string
	}{{"短名", 22, "短名"}, {"我的外置移动硬盘", 22, "我的外置移动硬盘"}, {"一二三四", 2, "一二…"}, {"🍎🍎🍎", 1, "🍎…"}, {"", 0, ""}, {"a", 0, "…"}, {"abc", 3, "abc"}, {"e\u0301x", 1, "e…"}} {
		if got := core.Truncate(c.s, c.n); got != c.want {
			t.Errorf("%+v: %q", c, got)
		}
	}
}
