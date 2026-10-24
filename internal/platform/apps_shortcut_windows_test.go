package platform

import (
	"bytes"
	"context"
	"os"
	"path/filepath"
	"strings"
	"syscall"
	"testing"
	"unsafe"

	"golang.org/x/sys/windows"
)

func TestAppShortcutNativeReadOnly(t *testing.T) {
	root := t.TempDir()
	target, path := filepath.Join(root, "literal target.exe"), filepath.Join(root, "fixture.lnk")
	if err := os.WriteFile(target, []byte("not executable"), 0600); err != nil {
		t.Fatal(err)
	}
	ran := false
	withAppShortcutReader(func(read func(string) (string, bool)) {
		ran = true
		var link *appShortcutCOM
		hr, _, _ := appShortcutCreate.Call(uintptr(unsafe.Pointer(appShortcutCLSID)), 0, 1, uintptr(unsafe.Pointer(appShortcutIID)), uintptr(unsafe.Pointer(&link)))
		if int32(hr) < 0 || link == nil {
			t.Fatalf("create HRESULT=%#x", hr)
		}
		defer link.release()
		wide, _ := windows.UTF16PtrFromString(target)
		hr, _, _ = syscall.SyscallN(link.vtable[20], uintptr(unsafe.Pointer(link)), uintptr(unsafe.Pointer(wide)))
		if int32(hr) < 0 {
			t.Fatalf("SetPath HRESULT=%#x", hr)
		}
		var persist *appShortcutCOM
		hr, _, _ = syscall.SyscallN(link.vtable[0], uintptr(unsafe.Pointer(link)), uintptr(unsafe.Pointer(appPersistIID)), uintptr(unsafe.Pointer(&persist)))
		if int32(hr) < 0 || persist == nil {
			t.Fatalf("persist HRESULT=%#x", hr)
		}
		defer persist.release()
		widePath, _ := windows.UTF16PtrFromString(path)
		hr, _, _ = syscall.SyscallN(persist.vtable[6], uintptr(unsafe.Pointer(persist)), uintptr(unsafe.Pointer(widePath)), 1)
		if int32(hr) < 0 {
			t.Fatalf("Save HRESULT=%#x", hr)
		}
		before, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		got, ok := read(path)
		if !ok || !strings.EqualFold(got, target) {
			t.Fatalf("target=%q ok=%v", got, ok)
		}
		after, err := os.ReadFile(path)
		if err != nil || !bytes.Equal(before, after) {
			t.Fatal("reading changed shortcut")
		}
		items := readAppShortcuts(context.Background(), []string{root}, read)
		if len(items) != 1 || items[0].Shortcut != path || !strings.EqualFold(items[0].Target, target) {
			t.Fatalf("evidence=%v", items)
		}
		if got, ok := CanonicalAppPath(target); !ok || !strings.EqualFold(got, target) {
			t.Fatalf("canonical=%q ok=%v", got, ok)
		}
		if _, ok := CanonicalAppPath(filepath.Join(root, "missing.exe")); ok {
			t.Fatal("missing object canonicalized")
		}
	})
	if !ran {
		t.Fatal("COM initialization failed")
	}
}
