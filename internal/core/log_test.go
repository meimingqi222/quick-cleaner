package core

import (
	"bytes"
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

func TestProductLogRotatesAboveTwoMegabytes(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, logFileName)
	payload := bytes.Repeat([]byte("x"), logMaxBytes+1)
	if err := os.WriteFile(path, payload, 0o600); err != nil {
		t.Fatal(err)
	}
	file := openProductLog(dir)
	if file == nil {
		t.Fatal("log did not open")
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
	old, err := os.ReadFile(filepath.Join(dir, logOldName))
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(old, payload) {
		t.Fatalf("rotated log length = %d", len(old))
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	if info.Size() != 0 {
		t.Fatalf("new log size = %d", info.Size())
	}
}

func TestProductLogKeepsFileAtExactLimit(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, logFileName)
	payload := bytes.Repeat([]byte("y"), logMaxBytes)
	if err := os.WriteFile(path, payload, 0o600); err != nil {
		t.Fatal(err)
	}
	file := openProductLog(dir)
	if file == nil {
		t.Fatal("log did not open")
	}
	_ = file.Close()
	if _, err := os.Stat(filepath.Join(dir, logOldName)); !os.IsNotExist(err) {
		t.Fatalf("exact limit rotated: %v", err)
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	if info.Size() != logMaxBytes {
		t.Fatalf("size = %d", info.Size())
	}
}

func TestProductLogRestrictsExistingAndRotatedFiles(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Unix permissions")
	}
	dir := t.TempDir()
	path := filepath.Join(dir, logFileName)
	if err := os.WriteFile(path, bytes.Repeat([]byte("x"), logMaxBytes+1), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(path, 0644); err != nil {
		t.Fatal(err)
	}
	f := openProductLog(dir)
	if f == nil {
		t.Fatal("open failed")
	}
	f.Close()
	for _, name := range []string{logFileName, logOldName} {
		info, err := os.Stat(filepath.Join(dir, name))
		if err != nil {
			t.Fatal(err)
		}
		if info.Mode().Perm() != 0600 {
			t.Errorf("%s mode = %o, want 600", name, info.Mode().Perm())
		}
	}
}
