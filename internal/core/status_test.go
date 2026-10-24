package core

import (
	"errors"
	"math"
	"reflect"
	"testing"
)

func statusTestPtr[T any](v T) *T { return &v }

func TestStatusFanModesAndTypedErrors(t *testing.T) {
	for n := 0; n < 256; n++ {
		if got := (FanMode{Kind: FanPercent, Percent: uint8(n)}).IsSupported(); got != (n == 60 || n == 100) {
			t.Fatalf("percent %d supported=%v", n, got)
		}
	}
	if !(FanMode{}).IsSupported() || (FanMode{Kind: 99}).IsSupported() {
		t.Fatal("automatic/unknown kind")
	}
	for _, tc := range []struct {
		kind         FanErrorKind
		detail, want string
	}{
		{FanNeedsRoot, "firmware refused", "firmware refused"},
		{FanOther, "root required", "root required"},
		{FanCanceled, "ignored", "canceled"},
		{FanNotInstalled, "", "风扇守护进程未安装"},
		{FanNeedsUpgrade, "", "风扇守护进程需要更新"},
	} {
		var err error = FanError{Kind: tc.kind, Detail: tc.detail}
		var typed FanError
		if err.Error() != tc.want || !errors.As(err, &typed) || typed.Kind != tc.kind {
			t.Fatalf("%+v", err)
		}
	}
}

func TestStatusGPULabels(t *testing.T) {
	for _, tc := range []struct {
		names []*string
		want  []string
	}{
		{nil, []string{}},
		{[]*string{statusTestPtr("AMD Radeon(TM) 610M"), statusTestPtr("NVIDIA GeForce RTX 4060 Laptop GPU")}, []string{"AMD", "NVIDIA"}},
		{[]*string{statusTestPtr("NVIDIA A"), statusTestPtr("NVIDIA B")}, []string{"GPU 1", "GPU 2"}},
		{[]*string{nil}, []string{"GPU 1"}},
		{[]*string{statusTestPtr("\u2003你好世界甲乙丙丁戊己庚 GPU"), statusTestPtr("amd gpu")}, []string{"你好世界甲乙丙丁戊己", "amd"}},
		{[]*string{statusTestPtr("abcdefghijX"), statusTestPtr("abcdefghijY"), nil}, []string{"GPU 1", "GPU 2", "GPU 3"}},
		{[]*string{statusTestPtr("   "), nil}, []string{"GPU 1", "GPU 2"}},
	} {
		gpus := make([]GpuReading, len(tc.names))
		for i, name := range tc.names {
			gpus[i].Name = name
		}
		if got := GPULabels(gpus); !reflect.DeepEqual(got, tc.want) {
			t.Fatalf("got %q want %q", got, tc.want)
		}
	}
}

func TestStatusDefaultsAndCPUTime(t *testing.T) {
	p := DefaultProcInfo()
	if p.Threads != 1 || p.StatusText != "Running" || p.PID != 0 || p.User != nil || len(p.Ports) != 0 {
		t.Fatalf("%+v", p)
	}
	var snap StatusSnapshot
	if snap.Battery != nil || snap.Thermal.CPUTemp != nil || len(snap.GPUs) != 0 || snap.MemTotal != 0 {
		t.Fatalf("%+v", snap)
	}
	for _, tc := range []struct {
		ms   uint64
		want string
	}{
		{0, "0:00.00"}, {9, "0:00.00"}, {10, "0:00.01"}, {850, "0:00.85"}, {49230, "0:49.23"},
		{852090, "14:12.09"}, {3599999, "59:59.99"}, {3600000, "1:00:00"}, {3883000, "1:04:43"},
		{math.MaxUint64, "5124095576030:25:51"},
	} {
		if got := FmtCPUTime(tc.ms); got != tc.want {
			t.Errorf("%d got %s want %s", tc.ms, got, tc.want)
		}
	}
}

func TestStatusProcTabs(t *testing.T) {
	if AllProcFilterTabs() != [4]ProcFilterTab{ProcFilterAll, ProcFilterApp, ProcFilterUser, ProcFilterSystem} {
		t.Fatal("tab order")
	}
	for _, tc := range []struct {
		p                 ProcInfo
		current           *string
		app, user, system bool
	}{
		{ProcInfo{PID: 4}, nil, false, false, true},
		{ProcInfo{PID: 5}, nil, false, false, false},
		{ProcInfo{PID: 100, IconSource: statusTestPtr("")}, nil, true, false, false},
		{ProcInfo{PID: 101, IconSource: statusTestPtr("")}, nil, true, true, false},
		{ProcInfo{PID: 999, User: statusTestPtr("DOMAIN\\Alice")}, statusTestPtr("ali"), false, true, false},
		{ProcInfo{PID: 999, User: statusTestPtr("DOMAIN\\Alice")}, statusTestPtr("bob"), false, false, false},
		{ProcInfo{PID: 999, User: statusTestPtr("İPEK")}, statusTestPtr("i\u0307pek"), false, true, false},
		{ProcInfo{PID: 999, ExePath: statusTestPtr(`C:\Program Files\X.exe`)}, nil, true, false, false},
		{ProcInfo{PID: 999, ExePath: statusTestPtr(`/APPLICATIONS/X.app/x`)}, nil, true, false, false},
		{ProcInfo{PID: 999, ExePath: statusTestPtr(`C:\WINDOWS\SYSTEM32\x.exe`), User: statusTestPtr("alice")}, nil, false, true, true},
	} {
		for _, pair := range []struct {
			tab  ProcFilterTab
			want bool
		}{{ProcFilterAll, true}, {ProcFilterApp, tc.app}, {ProcFilterUser, tc.user}, {ProcFilterSystem, tc.system}} {
			if got := MatchesProcTab(tc.p, pair.tab, tc.current); got != pair.want {
				t.Errorf("%+v tab=%d got=%v", tc.p, pair.tab, got)
			}
		}
	}
	for _, user := range []string{"SYSTEM", "NT AUTHORITY\\Local Service", "NETWORK SERVICE", "root", "_daemon", "subsystem-user"} {
		p := ProcInfo{PID: 500, User: &user}
		if MatchesProcTab(p, ProcFilterUser, nil) || !MatchesProcTab(p, ProcFilterSystem, nil) {
			t.Errorf("system user %s", user)
		}
	}
}

func TestStatusProcQueryRustCases(t *testing.T) {
	p := ProcInfo{PID: 1234, Name: "web_server", CPU: 15.5, MemBytes: 2 << 30, Ports: []uint16{80, 443, 3000}, User: statusTestPtr("meimingqi"), ExePath: statusTestPtr(`C:\App\web_server.exe`)}
	for _, q := range []string{":3000", ":80", "msedge, web_server", `"other", "web_server"`, "cpu > 10", "cpu < 20", "mem > 1 GB", "user:meimingqi", "-helper", "1234", "server", "443", " :+3000 ", "80;missing", "CPU <= 15.5%%", "cpu>=15.5", "mem<=2gib", "mem>=2 G"} {
		if !MatchProcQuery(p, q) {
			t.Errorf("expected match: %q", q)
		}
	}
	for _, q := range []string{":8080", "msedge, chrome", "cpu > 20", "mem > 3 GB", "user:SYSTEM", "-server", ":30", "cpu>15.5", "cpu<15.5", "mem<2G", "mem>2G", "01234"} {
		if MatchProcQuery(p, q) {
			t.Errorf("unexpected match: %q", q)
		}
	}
	p.MemBytes = 2 << 20
	for _, q := range []string{"mem>1", "mem > 1 MB", "mem>1m", "MEM>=2", "mem<1g", "mem <= 2048k", "mem>=2097152B", "mem>2047.999 KiB"} {
		if !MatchProcQuery(p, q) {
			t.Errorf("memory expected match: %q", q)
		}
	}
	for _, q := range []string{"mem>2", "mem>3m", "mem<1", "mem>NaN", "mem>inf", "mem>-1"} {
		if MatchProcQuery(p, q) {
			t.Errorf("memory unexpected match: %q", q)
		}
	}
}

func TestStatusProcQueryParsingAndUnicode(t *testing.T) {
	p := ProcInfo{PID: 90, Name: "İSTANBUL ΟΣ foo,bar a;b - cpu>nan mem>-1", Ports: []uint16{80, 65535}, User: statusTestPtr("ΟS İPEK"), ExePath: statusTestPtr("/tmp/HELPer")}
	for _, q := range []string{"", ",;;", `""`, "\u2003", "i\u0307stanbul", "ος", `"foo,bar"`, `"a;b"`, `"foo,bar`, `f"oo,ba"r`, "user:i\u0307pek", "-missing", "cpu>NaN", "mem>-1", ":65535", ":+80", ":00080", "+80", "-"} {
		if !MatchProcQuery(p, q) {
			t.Errorf("expected match %q", q)
		}
	}
	for _, q := range []string{"istanbul", "οσ", "-helper", ":65536", ":garbage", "cpu>inf", "mem>inf", "user:system", `"foo;bar"`} {
		if MatchProcQuery(p, q) {
			t.Errorf("unexpected match %q", q)
		}
	}
	// Invalid numeric syntax falls back to the ORIGINAL literal, not false or
	// an unconditional threshold match. Ports with invalid nonempty values do not.
	for _, q := range []string{"cpu>", "cpu>NaN", "cpu>inf", "cpu>-1", "cpu>0x1p2", "cpu>1_0", "mem>NaN", "mem>inf", "mem>-1", "mem>18446744073709551616B", "mem>1e309G", "mem>0x1p2", "mem>1_0", "mem>1GıB"} {
		if !MatchProcQuery(ProcInfo{Name: q}, q) {
			t.Errorf("missing literal fallback %q", q)
		}
		if MatchProcQuery(ProcInfo{Name: "unrelated", CPU: 100, MemBytes: math.MaxUint64}, q) {
			t.Errorf("invalid numeric interpreted as filter %q", q)
		}
	}
	if MatchProcQuery(ProcInfo{Name: ":invalid"}, ":invalid") {
		t.Fatal("invalid port must not fall back to literal")
	}
	// Fractional bytes truncate before comparing; CPU thresholds round to f32.
	if !MatchProcQuery(ProcInfo{MemBytes: 1}, "mem>=1.9B") || MatchProcQuery(ProcInfo{MemBytes: 1}, "mem>1.9B") {
		t.Fatal("fractional bytes")
	}
	if !MatchProcQuery(ProcInfo{CPU: 16777216}, "cpu>=16777217") || MatchProcQuery(ProcInfo{CPU: 16777216}, "cpu>16777217") {
		t.Fatal("f32 threshold")
	}
	if !MatchProcQuery(ProcInfo{CPU: 0}, "cpu>=-0") || !MatchProcQuery(ProcInfo{MemBytes: 0}, "mem>=-0B") {
		t.Fatal("negative zero accepted by Rust")
	}
}

func TestStatusProcessComparator(t *testing.T) {
	for _, tc := range []struct {
		a, b ProcInfo
		want int
	}{
		{ProcInfo{CPU: 2, MemBytes: 1}, ProcInfo{CPU: 1, MemBytes: 999}, -1},
		{ProcInfo{CPU: 2, MemBytes: 1}, ProcInfo{CPU: 2, MemBytes: 999}, 1},
		{ProcInfo{CPU: float32(math.NaN()), MemBytes: 1000}, ProcInfo{CPU: 20, MemBytes: 999}, -1},
		{ProcInfo{CPU: float32(math.Inf(1))}, ProcInfo{CPU: 1000}, -1},
		{ProcInfo{PID: 4, CPU: 2, MemBytes: 3}, ProcInfo{PID: 99, CPU: 2, MemBytes: 3}, 0},
	} {
		if got := CompareStatusProcesses(tc.a, tc.b); got != tc.want {
			t.Errorf("got %d want %d", got, tc.want)
		}
		if got := CompareStatusProcesses(tc.b, tc.a); got != -tc.want {
			t.Errorf("reverse got %d want %d", got, -tc.want)
		}
	}
}

func TestStatusHealthReportRustCasesAndBoundaries(t *testing.T) {
	healthy := HealthInputs{DiskFreeRatio: statusTestPtr(float32(.5)), MemUsed: 8 << 30, MemTotal: 32 << 30, CPUAvg: 10, UptimeSecs: 3600}
	for _, tc := range []struct {
		name   string
		change func(*HealthInputs)
		score  uint32
		worst  *HealthFactor
	}{
		{"healthy", func(*HealthInputs) {}, 100, nil},
		{"unknown disk", func(i *HealthInputs) { i.DiskFreeRatio = nil }, 100, nil},
		{"full disk", func(i *HealthInputs) { i.DiskFreeRatio = statusTestPtr(float32(0)) }, 55, statusTestPtr(HealthDisk)},
		{"almost full disk", func(i *HealthInputs) { i.DiskFreeRatio = statusTestPtr(float32(.02)) }, 59, statusTestPtr(HealthDisk)},
		{"cached memory", func(i *HealthInputs) { i.MemUsed = 25 << 30 }, 100, nil},
		{"swap", func(i *HealthInputs) { i.SwapUsed = 6 << 30 }, 89, statusTestPtr(HealthSwap)},
		{"cpu full", func(i *HealthInputs) { i.CPUAvg = 95 }, 80, statusTestPtr(HealthCPU)},
		{"all saturated", func(i *HealthInputs) {
			i.DiskFreeRatio = statusTestPtr(float32(0))
			i.MemUsed = i.MemTotal
			i.SwapUsed = i.MemTotal
			i.CPUAvg = 100
			i.UptimeSecs = 400 * 86400
		}, 0, statusTestPtr(HealthDisk)},
		{"no memory total", func(i *HealthInputs) { i.MemTotal = 0; i.MemUsed = math.MaxUint64; i.SwapUsed = math.MaxUint64 }, 100, nil},
		{"nonfinite", func(i *HealthInputs) {
			i.CPUAvg = float32(math.Inf(1))
			i.DiskFreeRatio = statusTestPtr(float32(math.NaN()))
		}, 100, nil},
		{"below mention", func(i *HealthInputs) { i.CPUAvg = 56.749996 }, 97, nil},
		{"mention inclusive", func(i *HealthInputs) { i.CPUAvg = 56.75 }, 97, statusTestPtr(HealthCPU)},
		{"round halves upward", func(i *HealthInputs) { i.CPUAvg = 51.125 }, 100, nil},
		{"round just over half", func(i *HealthInputs) { i.CPUAvg = 51.126 }, 99, nil},
		{"memory uptime tie last", func(i *HealthInputs) { i.MemUsed = i.MemTotal; i.UptimeSecs = 30 * 86400 }, 80, statusTestPtr(HealthUptime)},
		{"swap cpu tie last", func(i *HealthInputs) { i.SwapUsed = i.MemTotal; i.CPUAvg = 83.75 }, 70, statusTestPtr(HealthCPU)},
	} {
		t.Run(tc.name, func(t *testing.T) {
			in := healthy
			tc.change(&in)
			got := ComputeHealthReport(in)
			if got.Score != tc.score || !reflect.DeepEqual(got.Worst, tc.worst) {
				t.Fatalf("got %+v worst=%v want %d/%v", got, got.Worst, tc.score, tc.worst)
			}
		})
	}
	for pct := 1; pct < 100; pct++ {
		a, b := healthy, healthy
		a.DiskFreeRatio = statusTestPtr(float32(pct) / 100)
		b.DiskFreeRatio = statusTestPtr(float32(pct+1) / 100)
		if d := int(ComputeHealthReport(a).Score) - int(ComputeHealthReport(b).Score); d > 3 || d < -3 {
			t.Fatalf("discontinuous at %d: %d", pct, d)
		}
	}
}

func TestStatusHealthReportRustF32Oracle(t *testing.T) {
	// Expected values were produced by compiling the unmodified health domain
	// extracted from src/core/status.rs, not a float64 Go reference model.
	for _, tc := range []struct {
		free              float32
		used, total, swap uint64
		cpu               float32
		up                uint64
		score             uint32
		worst             HealthFactor
	}{
		{.175, 16777217, 20000003, 1000001, 51.125, 691201, 81, HealthDisk},
		{.17222223, 28999800422, 34359738368, 3899980042, 66.99999, 1000001, 68, HealthDisk},
		{.21944444, 8589934593, 10737418241, 107374184, 63.75, 1468800, 84, HealthCPU},
		{.24, math.MaxUint64, math.MaxUint64 - 1, 1000000000000000000, 59.998, 2999999, 71, HealthUptime},
	} {
		got := ComputeHealthReport(HealthInputs{DiskFreeRatio: &tc.free, MemUsed: tc.used, MemTotal: tc.total, SwapUsed: tc.swap, CPUAvg: tc.cpu, UptimeSecs: tc.up})
		if got.Score != tc.score || got.Worst == nil || *got.Worst != tc.worst {
			t.Fatalf("%+v got %+v", tc, got)
		}
	}
}

func TestStatusDisplayNamesAndShortOS(t *testing.T) {
	for _, tc := range []struct {
		exe, app string
		helper   *string
		want     string
	}{
		{"Telegram", "Telegram", nil, "Telegram"}, {"stable", "Warp", nil, "Warp stable"}, {"quick-cleaner", "QuickCleaner", nil, "QuickCleaner"},
		{"Microsoft Edge Helper", "Microsoft Edge", statusTestPtr("Microsoft Edge Helper (Renderer)"), "Microsoft Edge Helper (Renderer)"},
		{"Termius Helper (GPU)", "Termius", statusTestPtr("Termius Helper (GPU)"), "Termius Helper (GPU)"},
		{"Helper (GPU)", "Termius", statusTestPtr("Helper (GPU)"), "Termius Helper (GPU)"},
		{"x", "", statusTestPtr("h"), "x"}, {"stable", "Warp", statusTestPtr(""), "Warp stable"},
		{"İX", "i\u0307x", nil, "i\u0307x İX"}, // Filtering precedes lowercase expansion, so the combining dot in app is discarded.
		{"ΟΣ", "οσ", nil, "οσ"}, {"०३", "०३", nil, "०३"},
	} {
		if got := ComposeProcessName(tc.exe, tc.app, tc.helper); got != tc.want {
			t.Errorf("%+v got %q", tc, got)
		}
	}
	for _, tc := range []struct{ in, want string }{
		{"Windows 11 Home China", "Windows 11"}, {"macOS 15.6 Sequoia", "macOS 15.6"}, {"Ubuntu 24.04 LTS", "Ubuntu 24.04"},
		{"Mac OS X 10.6 Snow Leopard", "Mac OS X 10.6"}, {"Windows", "Windows"}, {"  Mac\u2003OS  X 10.6  ", "Mac OS X 10.6"}, {"OS １２ vendor", "OS １２ vendor"}, {"", ""},
	} {
		if got := ShortOSName(tc.in); got != tc.want {
			t.Errorf("%q got %q", tc.in, got)
		}
	}
}
