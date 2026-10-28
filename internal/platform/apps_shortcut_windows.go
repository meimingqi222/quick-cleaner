package platform

import (
	"context"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"syscall"
	"unsafe"

	ole "github.com/go-ole/go-ole"
	"golang.org/x/sys/windows"
)

var appShortcutOLE = windows.NewLazySystemDLL("ole32.dll")
var appShortcutInitialize = appShortcutOLE.NewProc("CoInitializeEx")
var appShortcutCreate = appShortcutOLE.NewProc("CoCreateInstance")
var appShortcutCLSID = ole.NewGUID("{00021401-0000-0000-C000-000000000046}")
var appShortcutIID = ole.NewGUID("{000214F9-0000-0000-C000-000000000046}")
var appPersistIID = ole.NewGUID("{0000010B-0000-0000-C000-000000000046}")

type appShortcutCOM struct{ vtable *[21]uintptr }

func (o *appShortcutCOM) release() {
	if o != nil {
		syscall.SyscallN(o.vtable[2], uintptr(unsafe.Pointer(o)))
	}
}

func withAppShortcutReader(run func(func(string) (string, bool))) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	hr, _, _ := appShortcutInitialize.Call(0, 2)
	if int32(hr) < 0 && uint32(hr) != 0x80010106 {
		return
	}
	if int32(hr) >= 0 {
		defer ole.CoUninitialize()
	}
	run(readAppShortcutTarget)
}

func ReadWindowsShortcutTarget(path string) (target string, ok bool) {
	withAppShortcutReader(func(read func(string) (string, bool)) { target, ok = read(path) })
	return
}

func readAppShortcutTarget(path string) (string, bool) {
	wide, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return "", false
	}
	attr, err := windows.GetFileAttributes(wide)
	if err != nil || attr&windows.FILE_ATTRIBUTE_REPARSE_POINT != 0 {
		return "", false
	}
	var link *appShortcutCOM
	hr, _, _ := appShortcutCreate.Call(uintptr(unsafe.Pointer(appShortcutCLSID)), 0, 1, uintptr(unsafe.Pointer(appShortcutIID)), uintptr(unsafe.Pointer(&link)))
	if int32(hr) < 0 || link == nil {
		return "", false
	}
	defer link.release()
	var persist *appShortcutCOM
	hr, _, _ = syscall.SyscallN(link.vtable[0], uintptr(unsafe.Pointer(link)), uintptr(unsafe.Pointer(appPersistIID)), uintptr(unsafe.Pointer(&persist)))
	if int32(hr) < 0 || persist == nil {
		return "", false
	}
	hr, _, _ = syscall.SyscallN(persist.vtable[5], uintptr(unsafe.Pointer(persist)), uintptr(unsafe.Pointer(wide)), 0)
	persist.release()
	if int32(hr) < 0 {
		return "", false
	}
	buf := make([]uint16, 32768)
	// Load/GetPath only: Resolve may search disks, contact a network or mutate
	// the shortcut, none of which is allowed during discovery.
	hr, _, _ = syscall.SyscallN(link.vtable[3], uintptr(unsafe.Pointer(link)), uintptr(unsafe.Pointer(&buf[0])), uintptr(len(buf)), 0, 0)
	runtime.KeepAlive(wide)
	if int32(hr) < 0 {
		return "", false
	}
	target := windows.UTF16ToString(buf)
	return target, target != ""
}

func appShortcutRoots() []string {
	var roots []string
	if home, ok := UserHome(); ok {
		roots = append(roots, filepath.Join(home, "Desktop"))
		if sid, ok := UserSID(); ok {
			for _, entry := range enumFolderStrings(syscall.HKEY_USERS, sid+`\`+shellFoldersKey) {
				if asciiFolderEqual(entry.name, "Desktop") {
					if path, ok := expandFolderProfile(entry.value, UserHome); ok && filepath.IsAbs(path) {
						roots = append(roots, path)
					}
				}
			}
		}
	}
	if roaming, ok := UserDataDir(); ok {
		roots = append(roots, filepath.Join(roaming, `Microsoft\Windows\Start Menu\Programs`))
	}
	if base, ok := os.LookupEnv("ProgramData"); ok {
		roots = append(roots, filepath.Join(base, `Microsoft\Windows\Start Menu\Programs`))
	}
	if base, ok := os.LookupEnv("PUBLIC"); ok {
		roots = append(roots, filepath.Join(base, "Desktop"))
	}
	slices.Sort(roots)
	return slices.Compact(roots)
}

func ReadWindowsAppShortcuts(ctx context.Context) []ShortcutAppEvidence {
	var out []ShortcutAppEvidence
	withAppShortcutReader(func(read func(string) (string, bool)) { out = readAppShortcuts(ctx, appShortcutRoots(), read) })
	return out
}

// CanonicalAppPath normalizes short and verbatim local drive paths. UNC paths
// are not application-discovery candidates. This grants no deletion authority.
func CanonicalAppPath(path string) (string, bool) {
	wide, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return "", false
	}
	h, err := windows.CreateFile(wide, 0, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE, nil, windows.OPEN_EXISTING, windows.FILE_FLAG_BACKUP_SEMANTICS, 0)
	if err != nil {
		return "", false
	}
	defer windows.CloseHandle(h)
	buf := make([]uint16, 32768)
	n, err := windows.GetFinalPathNameByHandle(h, &buf[0], uint32(len(buf)), 0)
	if err != nil || n == 0 || n >= uint32(len(buf)) {
		return "", false
	}
	result := strings.TrimPrefix(windows.UTF16ToString(buf[:n]), `\\?\`)
	if len(result) < 3 || result[1] != ':' || result[2] != '\\' || !((result[0] >= 'A' && result[0] <= 'Z') || (result[0] >= 'a' && result[0] <= 'z')) {
		return "", false
	}
	return result, true
}
