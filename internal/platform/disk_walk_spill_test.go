//go:build darwin || linux

package platform

import (
	"context"
	"os"
	"path/filepath"
	"strconv"
	"testing"
	"time"
)

func TestMacWalkSpillRecordRoundTrip(t *testing.T) {
	spill := &macWalkSpill{}
	if err := spill.create(); err != nil {
		t.Fatal(err)
	}
	defer spill.remove()
	entries := []MacWalkEntry{
		{Parent: 0, Name: "dir", IsDir: true, Mtime: 1_700_000_000, Used: true},
		{Parent: 1, Name: "file", IsRegular: true, Size: 4096, Mtime: 1_700_000_001, Used: true},
		{Parent: 1, Name: "空 名", Size: 0, Used: true},
	}
	for _, entry := range entries {
		if err := spill.appendEntry(entry); err != nil {
			t.Fatal(err)
		}
	}
	if err := spill.finish(); err != nil {
		t.Fatal(err)
	}
	if _, err := spill.file.Seek(0, 0); err != nil {
		t.Fatal(err)
	}
	for i, want := range entries {
		got, err := spill.Read()
		if err != nil {
			t.Fatal(err)
		}
		if got.Parent != want.Parent || got.Name != want.Name || got.IsDir != want.IsDir || got.Size != want.Size || got.Mtime != want.Mtime || !got.Used {
			t.Fatalf("record %d: got %+v want %+v", i, got, want)
		}
	}
	if _, err := spill.Read(); err == nil {
		t.Fatal("spill continued past its last record")
	}
	if spill.nameBytes != uint64(2+len("dir")+2+len("file")+2+len("空 名")) {
		t.Fatal("name byte reserve", spill.nameBytes)
	}
}

func TestMacWalkSpillStartsAtThresholdAndCleansUp(t *testing.T) {
	macWalkSpillThreshold = 3
	defer func() { macWalkSpillThreshold = 400_000 }()
	enum := func(_ context.Context, path string, _ time.Duration) diskWalkBatch {
		if path == "/fixture" {
			return diskWalkBatch{items: []diskWalkItem{diskWalkTestItem("a", true), diskWalkTestItem("b", false)}}
		}
		return diskWalkBatch{items: []diskWalkItem{diskWalkTestItem("c", false)}}
	}
	result, err := diskWalkAcquire(context.Background(), VolumeID{Mount: "/fixture", Label: "Fixture"}, nil, enum, diskWalkTestState(), 1, time.Second, time.Second)
	if err != nil {
		t.Fatal(err)
	}
	defer result.Spill.Remove()
	if result.Spill == nil || result.Spill.Count() != 3 || len(result.Entries) != 1 {
		t.Fatalf("spill did not take over: %+v entries=%d", result.Spill, len(result.Entries))
	}
	path := result.Spill.Path()
	if _, err := os.Stat(path); err != nil {
		t.Fatal("spill file missing", err)
	}
	result.Spill.Remove()
	if _, err := os.Stat(path); !os.IsNotExist(err) {
		t.Fatal("spill file survived Remove", err)
	}
}

func TestMacWalkSpillReclaimsDeadProcessFilesOnly(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("TMPDIR", dir)
	deadPID := deadTestPID(t)
	dead := filepath.Join(dir, "qc-spill-"+strconv.Itoa(deadPID)+"-0.bin")
	live := filepath.Join(dir, "qc-spill-"+strconv.Itoa(os.Getpid())+"-9.bin")
	other := filepath.Join(dir, "notes.bin")
	for _, path := range []string{dead, live, other} {
		if err := os.WriteFile(path, []byte("x"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	reclaimStaleMacWalkSpill()
	if _, err := os.Stat(dead); !os.IsNotExist(err) {
		t.Fatal("dead spill survived", err)
	}
	for _, path := range []string{live, other} {
		if _, err := os.Stat(path); err != nil {
			t.Fatal("reclaim removed a live file", path, err)
		}
	}
}

func deadTestPID(t *testing.T) int {
	t.Helper()
	bin := "/usr/bin/true"
	if _, err := os.Stat(bin); err != nil {
		bin = "/bin/true"
	}
	proc, err := os.StartProcess(bin, []string{"true"}, &os.ProcAttr{})
	if err != nil {
		t.Fatal(err)
	}
	if _, err = proc.Wait(); err != nil {
		t.Fatal(err)
	}
	return proc.Pid
}
