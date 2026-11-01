package platform

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"syscall"
	"testing"
	"time"

	"golang.org/x/sys/windows"
)

func TestWindowsProbeDoesNotMistakeHardlinksForExternalLocks(t *testing.T) {
	dir := t.TempDir()
	base := filepath.Join(dir, "base")
	if err := os.WriteFile(base, []byte("cache"), 0600); err != nil {
		t.Fatal(err)
	}
	paths := []string{base}
	for i := 0; i < 255; i++ {
		path := filepath.Join(dir, strconv.Itoa(i))
		if err := os.Link(base, path); err != nil {
			t.Fatal(err)
		}
		paths = append(paths, path)
	}
	var wg sync.WaitGroup
	for range 4 {
		wg.Go(func() {
			for range 3 {
				if locked, known := inUseProbeBatch(paths); len(locked) != 0 || !known {
					t.Errorf("unlocked hardlinks: locks=%d known=%v; want 0, true", len(locked), known)
				}
			}
		})
	}
	wg.Wait()
	p, err := windows.UTF16PtrFromString(base)
	if err != nil {
		t.Fatal(err)
	}
	h, err := windows.CreateFile(p, windows.GENERIC_READ, windows.FILE_SHARE_READ, nil, windows.OPEN_EXISTING, windows.FILE_ATTRIBUTE_NORMAL, 0)
	if err != nil {
		t.Fatal(err)
	}
	locked, known := inUseProbeBatch(paths)
	windows.CloseHandle(h)
	if len(locked) != len(paths) || !known {
		t.Fatalf("externally held hardlinks: locks=%d known=%v; want %d, true", len(locked), known, len(paths))
	}
	if locked, known := inUseProbeBatch(paths); len(locked) != 0 || !known {
		t.Fatalf("released hardlinks: locks=%d known=%v; want 0, true", len(locked), known)
	}
}

// Scan-time detection must not walk targets with exclusive opens: that delayed
// results by seconds per directory and only ever produced a badge.
func TestWindowsScanDoesNotProbeTargetFiles(t *testing.T) {
	held := filepath.Join(t.TempDir(), "held.bin")
	if err := os.WriteFile(held, []byte("keep"), 0600); err != nil {
		t.Fatal(err)
	}
	f, err := os.Open(held)
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	if busy := DetectInUse([]string{held, filepath.Dir(held)}); len(busy) != 0 {
		t.Fatalf("scan-time detection probed files: %+v", busy)
	}
}

func TestWindowsDirectoryProbeCoversLargeUnlockedTrees(t *testing.T) {
	dir := t.TempDir()
	started := time.Now()
	for i := 0; i < 4100; i++ {
		path, err := windows.UTF16PtrFromString(filepath.Join(dir, strconv.Itoa(i)))
		if err != nil {
			t.Fatal(err)
		}
		h, err := windows.CreateFile(path, windows.GENERIC_WRITE, 0, nil, windows.CREATE_NEW, windows.FILE_ATTRIBUTE_NORMAL, 0)
		if err != nil {
			t.Fatal(err)
		}
		windows.CloseHandle(h)
	}
	t.Logf("created 4100 files in %s", time.Since(started))
	started = time.Now()
	if busy, known := IsOpen(dir); busy || !known {
		t.Fatalf("unlocked 4100-entry tree = busy %v, known %v; want false, true", busy, known)
	}
	t.Logf("clear probe took %s", time.Since(started))
	locked := filepath.Join(dir, "4099")
	p, err := windows.UTF16PtrFromString(locked)
	if err != nil {
		t.Fatal(err)
	}
	h, err := windows.CreateFile(p, windows.GENERIC_READ, 0, nil, windows.OPEN_EXISTING, windows.FILE_ATTRIBUTE_NORMAL, 0)
	if err != nil {
		t.Fatal(err)
	}
	defer windows.CloseHandle(h)
	started = time.Now()
	if busy, known := IsOpen(dir); !busy || !known {
		t.Fatalf("locked tail entry = busy %v, known %v; want true, true", busy, known)
	}
	t.Logf("busy probe took %s", time.Since(started))
}

func TestOccupancyOwnersReportsTheLockingProcess(t *testing.T) {
	dir := t.TempDir()
	locked := filepath.Join(dir, "locked")
	if err := os.WriteFile(locked, []byte("retain"), 0600); err != nil {
		t.Fatal(err)
	}
	ready := filepath.Join(t.TempDir(), "ready")
	cmd := exec.Command(os.Args[0], "-test.run=TestOccupancyHolderHelper")
	cmd.Env = append(os.Environ(), "QC_OCC_HOLD="+locked, "QC_OCC_READY="+ready)
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() {
		_ = cmd.Process.Kill()
		_, _ = cmd.Process.Wait()
	}()
	deadline := time.Now().Add(10 * time.Second)
	for {
		if _, err := os.Stat(ready); err == nil {
			break
		}
		if !time.Now().Before(deadline) {
			t.Fatal("helper did not lock the file")
		}
		time.Sleep(20 * time.Millisecond)
	}
	owners, err := OccupancyOwners(dir)
	if err != nil {
		t.Fatalf("occupancy probe: %v", err)
	}
	if len(owners) == 0 {
		t.Fatal("locked file reported no owners")
	}
	found := false
	for _, owner := range owners {
		if int(owner.PID) == cmd.Process.Pid {
			found = true
			if owner.System {
				t.Fatalf("same-user helper marked as system owner: %+v", owner)
			}
			if owner.Name == "" {
				t.Fatal("owner has no display name")
			}
		}
	}
	if !found {
		t.Fatalf("locking pid %d missing from %+v", cmd.Process.Pid, owners)
	}
}

func TestOccupancyHolderHelper(t *testing.T) {
	locked := os.Getenv("QC_OCC_HOLD")
	ready := os.Getenv("QC_OCC_READY")
	if locked == "" || ready == "" {
		return
	}
	var handles []windows.Handle
	defer func() {
		for _, h := range handles {
			_ = windows.CloseHandle(h)
		}
	}()
	for _, item := range strings.Split(locked, ";") {
		if item == "" {
			continue
		}
		p, err := windows.UTF16PtrFromString(item)
		if err != nil {
			t.Fatal(err)
		}
		h, err := windows.CreateFile(p, windows.GENERIC_READ, 0, nil, windows.OPEN_EXISTING, windows.FILE_ATTRIBUTE_NORMAL, 0)
		if err != nil {
			t.Fatal(err)
		}
		handles = append(handles, h)
	}
	if err := os.WriteFile(ready, []byte(fmt.Sprintf("%d", os.Getpid())), 0600); err != nil {
		t.Fatal(err)
	}
	time.Sleep(15 * time.Second)
}

func TestFindLockedCacheOwnersAttributesSiblingAppAndExcludesCleanerTree(t *testing.T) {
	base := t.TempDir()
	gpuDir := filepath.Join(base, "AppData", "Roaming", "Notion", "GPUCache")
	shaderDir := filepath.Join(base, "AppData", "Roaming", "Notion", "ShaderCache")
	selfDir := filepath.Join(base, "AppData", "Roaming", "SelfChild", "Cache")
	clearDir := filepath.Join(base, "AppData", "Roaming", "Idle", "Cache")
	for _, d := range []string{gpuDir, shaderDir, selfDir, clearDir} {
		if err := os.MkdirAll(d, 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(d, "data_0"), []byte("cache"), 0600); err != nil {
			t.Fatal(err)
		}
	}

	// Direct child is inside QuickCleaner's own process tree and must be omitted.
	childReady := filepath.Join(base, "child.ready")
	childCmd := exec.Command(os.Args[0], "-test.run=^TestOccupancyHolderHelper$")
	childCmd.Env = append(os.Environ(), "QC_OCC_HOLD="+filepath.Join(selfDir, "data_0"), "QC_OCC_READY="+childReady)
	if err := childCmd.Start(); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = childCmd.Process.Kill(); _, _ = childCmd.Process.Wait() })

	// Sibling process simulates an independent user desktop application.
	parent, err := windows.OpenProcess(windows.PROCESS_CREATE_PROCESS|windows.PROCESS_DUP_HANDLE, false, uint32(os.Getppid()))
	if err != nil {
		t.Fatal(err)
	}
	defer windows.CloseHandle(parent)
	sibReady := filepath.Join(base, "sibling.ready")
	sibHold := filepath.Join(gpuDir, "data_0") + ";" + filepath.Join(shaderDir, "data_0")
	sibCmd := exec.Command(os.Args[0], "-test.run=^TestOccupancyHolderHelper$")
	sibCmd.Env = append(os.Environ(), "QC_OCC_HOLD="+sibHold, "QC_OCC_READY="+sibReady)
	sibCmd.SysProcAttr = &syscall.SysProcAttr{ParentProcess: syscall.Handle(parent)}
	if err := sibCmd.Start(); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = sibCmd.Process.Kill(); _, _ = sibCmd.Process.Wait() })

	deadline := time.Now().Add(10 * time.Second)
	for _, r := range []string{childReady, sibReady} {
		for {
			if _, err := os.Stat(r); err == nil {
				break
			}
			if !time.Now().Before(deadline) {
				t.Fatalf("helper %s did not lock in time", r)
			}
			time.Sleep(15 * time.Millisecond)
		}
	}

	hits, err := FindLockedCacheOwners([]string{gpuDir, shaderDir, selfDir, clearDir})
	if err != nil {
		t.Fatalf("FindLockedCacheOwners: %v", err)
	}
	seenTargets := map[string]bool{}
	for _, h := range hits {
		if int(h.Owner.PID) == childCmd.Process.Pid {
			t.Fatalf("cleaner child process was offered for closure: %+v", h)
		}
		if int(h.Owner.PID) == sibCmd.Process.Pid {
			seenTargets[h.Target] = true
		}
	}
	if !seenTargets[gpuDir] || !seenTargets[shaderDir] || seenTargets[selfDir] || seenTargets[clearDir] {
		t.Fatalf("unexpected target attribution: seen=%v hits=%+v", seenTargets, hits)
	}
}
