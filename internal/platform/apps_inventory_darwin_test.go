package platform

import (
	"context"
	"os"
	"path/filepath"
	"syscall"
	"testing"
	"time"
)

func TestMacInventoryBundleIdentityAndAllocatedBytes(t *testing.T) {
	bundle := macFixtureBundle(t, t.TempDir(), "Native.app")
	plist := `<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.example.Native</string><key>CFBundleShortVersionString</key><string>2.3</string><key>FixtureDate</key><date>2026-01-01T00:00:00Z</date></dict></plist>`
	if err := os.WriteFile(filepath.Join(bundle, "Contents", "Info.plist"), []byte(plist), 0600); err != nil {
		t.Fatal(err)
	}
	for _, system := range []bool{false, true} {
		app := macParseApp(context.Background(), macAppRoot{bundle, system}, macAppDate{}, 4096)
		wantRoot := "HKCU"
		if system {
			wantRoot = "SystemApp"
		}
		if app.ID != "com.example.Native" || app.Version != "2.3" || app.RegistryRoot != wantRoot || app.IsSystemComponent != system || app.InstallDate == nil || app.EstimatedSize != 4096 {
			t.Fatalf("%+v", app)
		}
	}
	path := filepath.Join(bundle, "Contents", "data")
	if err := os.WriteFile(path, make([]byte, 8192), 0600); err != nil {
		t.Fatal(err)
	}
	var want uint64
	for _, p := range []string{path, filepath.Join(bundle, "Contents", "Info.plist")} {
		info, err := os.Stat(p)
		if err != nil {
			t.Fatal(err)
		}
		want += uint64(info.Sys().(*syscall.Stat_t).Blocks) * 512
	}
	if got := macBundleAllocated(context.Background(), bundle); got != want {
		t.Fatalf("allocated bytes: %d != %d", got, want)
	}
}

func TestMacInventoryCommandCancellation(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, ok := macInventoryCommand(ctx, time.Minute, "/bin/sleep", "60"); ok {
		t.Fatal("cancelled command succeeded")
	}
	ctx, cancel = context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()
	start := time.Now()
	if _, ok := macInventoryCommand(ctx, time.Minute, "/bin/sh", "-c", "sleep 60 & wait"); ok {
		t.Fatal("cancelled process tree succeeded")
	}
	if time.Since(start) > 5*time.Second {
		t.Fatal("cancellation did not terminate descendant pipes")
	}
	// Keep the public cancellation contract callable without invoking native probes.
	if apps := ReadMacAppRecords(ctx); len(apps) != 0 {
		t.Fatal(apps)
	}
	if _, known := InstalledBundleIDs(ctx); known {
		t.Fatal("cancelled inventory is known")
	}
	if _, known := BundleIsStillInstalled(ctx, "com.example.App"); known {
		t.Fatal("cancelled identity is known")
	}
}

func TestBundleIsStillInstalledFastPathAbsence(t *testing.T) {
	start := time.Now()
	installed, known := BundleIsStillInstalled(context.Background(), "com.quickcleaner.definitely-not-installed-xyz")
	duration := time.Since(start)
	if !known {
		t.Skip("spotlight index not usable in test environment")
	}
	if installed {
		t.Fatal("non-existent bundle reported as installed")
	}
	// Calling full lsregister -dump takes 6+ seconds. The fast path must finish well under 2s.
	if duration > 2*time.Second {
		t.Fatalf("BundleIsStillInstalled took %v; expected fast path under 2s without full lsregister dump", duration)
	}
}

func TestMacAppSizesUseIndexAndOnlyRunDUForMissingPaths(t *testing.T) {
	calls := 0
	run := func(_ context.Context, _ time.Duration, program string, args ...string) (ProcessResult, bool) {
		calls++
		if program != "/usr/bin/du" || len(args) != 2 || args[0] != "-sk" || args[1] != "/unindexed.app" {
			t.Fatalf("wrong fallback: %s %v", program, args)
		}
		return ProcessResult{OK: true, Stdout: []byte("12\t/unindexed.app\n")}, true
	}
	size := func(_ context.Context, path string) (uint64, bool) {
		if path == "/indexed.app" {
			return 77, true
		}
		return 0, false
	}
	got := macAppSizes(context.Background(), []string{"/indexed.app", "/unindexed.app"}, size, run)
	if calls != 1 || got["/indexed.app"] != 77 || got["/unindexed.app"] != 12*1024 {
		t.Fatalf("wrong results: %v calls=%d", got, calls)
	}
	got = macAppSizes(context.Background(), []string{"/indexed.app"}, size, run)
	if calls != 1 || got["/indexed.app"] != 77 {
		t.Fatal("covered index should avoid du")
	}
}
