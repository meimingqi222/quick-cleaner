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
	t.Run("long path", func(t *testing.T) { shortcutRoundTrips(t, func(path string) string { return path }) })
	t.Run("8.3 short path", func(t *testing.T) { shortcutRoundTrips(t, shortPathName) })
}

// shortcutRoundTrips 用 store 决定写进 .lnk 的目标形式，再断言原生读取器还原出同一个
// 对象且没有改写 .lnk。
func shortcutRoundTrips(t *testing.T, store func(string) string) {
	t.Helper()
	root := t.TempDir()
	target, path := filepath.Join(root, "literal target.exe"), filepath.Join(root, "fixture.lnk")
	if err := os.WriteFile(target, []byte("not executable"), 0600); err != nil {
		t.Fatal(err)
	}
	ran := false
	withAppShortcutReader(func(read func(string) (string, bool)) {
		ran = true
		saveShortcut(t, path, store(target))
		before, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		got, ok := read(path)
		if !ok || !sameAppPath(t, got, target) {
			t.Fatalf("target=%q ok=%v", got, ok)
		}
		after, err := os.ReadFile(path)
		if err != nil || !bytes.Equal(before, after) {
			t.Fatal("reading changed shortcut")
		}
		items := readAppShortcuts(context.Background(), []string{root}, read)
		if len(items) != 1 || !strings.EqualFold(items[0].Shortcut, path) || !sameAppPath(t, items[0].Target, target) {
			t.Fatalf("evidence=%v", items)
		}
		canonical, ok := CanonicalAppPath(target)
		if !ok || !sameAppPath(t, canonical, target) {
			t.Fatalf("canonical=%q ok=%v", canonical, ok)
		}
		if _, ok := CanonicalAppPath(filepath.Join(root, "missing.exe")); ok {
			t.Fatal("missing object canonicalized")
		}
	})
	if !ran {
		t.Fatal("COM initialization failed")
	}
}

// sameAppPath 比较两个路径是否指向同一个对象。8.3 短名启用的卷上，t.TempDir() 和写进
// .lnk 的路径是 MEIMIN~1 形式，而 COM GetPath 与 CanonicalAppPath 都返回长名，直接比
// 字符串会假失败。canonical 只在对象存在时成立，读不到就退回字面比较。
func sameAppPath(t *testing.T, got, want string) bool {
	t.Helper()
	canonicalGot, gotOK := CanonicalAppPath(got)
	canonicalWant, wantOK := CanonicalAppPath(want)
	if gotOK && wantOK {
		return strings.EqualFold(canonicalGot, canonicalWant)
	}
	return strings.EqualFold(got, want)
}

// shortPathName returns the 8.3 form of path; a volume that generates no short
// names returns the long name unchanged, so the caller needs no branch.
func shortPathName(path string) string {
	wide, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return path
	}
	buf := make([]uint16, windows.MAX_PATH)
	n, err := windows.GetShortPathName(wide, &buf[0], uint32(len(buf)))
	if err != nil || n == 0 || n >= uint32(len(buf)) {
		return path
	}
	return windows.UTF16ToString(buf[:n])
}

// saveShortcut 用 IShellLink + IPersistFile 写出 .lnk，目标按原样存入，不做解析。
func saveShortcut(t *testing.T, path, target string) {
	t.Helper()
	var link *appShortcutCOM
	hr, _, _ := appShortcutCreate.Call(uintptr(unsafe.Pointer(appShortcutCLSID)), 0, 1, uintptr(unsafe.Pointer(appShortcutIID)), uintptr(unsafe.Pointer(&link)))
	if int32(hr) < 0 || link == nil {
		t.Fatalf("create HRESULT=%#x", hr)
	}
	defer link.release()
	wideTarget, _ := windows.UTF16PtrFromString(target)
	hr, _, _ = syscall.SyscallN(link.vtable[20], uintptr(unsafe.Pointer(link)), uintptr(unsafe.Pointer(wideTarget)))
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
}
