package main

import (
	"bytes"
	"encoding/binary"
	"os"
	"path/filepath"
	"testing"
	"unicode/utf16"
)

func TestResourcesMatchRustBuild(t *testing.T) {
	icon, err := os.ReadFile(filepath.Join("..", "..", "assets", "icon.ico"))
	if err != nil {
		t.Fatal(err)
	}
	images, group, err := iconResources(icon)
	if err != nil {
		t.Fatal(err)
	}
	n := int(binary.LittleEndian.Uint16(group[4:]))
	if n == 0 || n != len(images) || len(group) != 6+14*n {
		t.Fatalf("group lists %d images, file has %d", n, len(images))
	}
	for i := range n {
		if id := binary.LittleEndian.Uint16(group[6+14*i+12:]); images[id] == nil {
			t.Fatalf("group entry %d names missing image %d", i, id)
		}
	}
	syso, err := build(icon, "1.2.3", "amd64")
	if err != nil {
		t.Fatal(err)
	}
	if binary.LittleEndian.Uint16(syso) != 0x8664 {
		t.Fatal("not an amd64 COFF object")
	}
	for _, want := range []string{`level="asInvoker"`, "<longPathAware", "PerMonitorV2", "Microsoft.Windows.Common-Controls"} {
		if !bytes.Contains(syso, []byte(want)) {
			t.Fatalf("manifest lacks %s", want)
		}
	}
	for _, want := range []string{"quick-cleaner.exe", "QuickCleaner", "1.2.3"} {
		if !bytes.Contains(syso, utf16le(want)) {
			t.Fatalf("version info lacks %q", want)
		}
	}
	// MyGo's window class loads icon resource 1; the group must carry that id.
	if !bytes.Contains(syso, group) {
		t.Fatal("icon group missing from the object")
	}
}

func TestVersionComesFromTheProductConstant(t *testing.T) {
	path := filepath.Join(t.TempDir(), "root.go")
	if err := os.WriteFile(path, []byte("package app\r\n\r\nconst Version = \"0.1.2\"\r\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if v, err := appVersion(path); err != nil || v != "0.1.2" {
		t.Fatalf("version %q %v", v, err)
	}
	if fileVersion("0.1.2") != [4]uint16{0, 1, 2, 0} {
		t.Fatal("fixed file version")
	}
}

func utf16le(s string) []byte {
	var b []byte
	for _, u := range utf16.Encode([]rune(s)) {
		b = binary.LittleEndian.AppendUint16(b, u)
	}
	return b
}
