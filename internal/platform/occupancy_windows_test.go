package platform

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"sync"
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
	p, err := windows.UTF16PtrFromString(locked)
	if err != nil {
		t.Fatal(err)
	}
	h, err := windows.CreateFile(p, windows.GENERIC_READ, 0, nil, windows.OPEN_EXISTING, windows.FILE_ATTRIBUTE_NORMAL, 0)
	if err != nil {
		t.Fatal(err)
	}
	defer windows.CloseHandle(h)
	if err := os.WriteFile(ready, []byte(fmt.Sprintf("%d", os.Getpid())), 0600); err != nil {
		t.Fatal(err)
	}
	time.Sleep(15 * time.Second)
}
