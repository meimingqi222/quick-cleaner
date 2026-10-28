package platform

import (
	"path/filepath"
	"strings"
	"sync"
	"syscall"
	"unsafe"
)

const shellFoldersKey = `Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders`

var knownFoldersOnce sync.Once
var knownFoldersCache []string
var enumFolderValue = syscall.NewLazyDLL("advapi32.dll").NewProc("RegEnumValueW")

// KnownFolders returns the actual registered folders of the trusted user.
// It deliberately avoids shell APIs, which target the elevated process account.
func KnownFolders() []string {
	knownFoldersOnce.Do(func() {
		knownFoldersCache = collectKnownFolders(UserSID, UserHome, enumFolderStrings)
	})
	return append([]string(nil), knownFoldersCache...)
}

type folderString struct{ name, value string }

func collectKnownFolders(sid, home func() (string, bool), read func(syscall.Handle, string) []folderString) []string {
	var entries []folderString
	if id, ok := sid(); ok {
		entries = read(syscall.HKEY_USERS, id+`\`+shellFoldersKey)
	}
	if len(entries) == 0 {
		entries = read(syscall.HKEY_CURRENT_USER, shellFoldersKey)
	}
	var folders []string
	for _, entry := range entries {
		for _, name := range [...]string{"Desktop", "Personal", "My Pictures", "My Video", "My Music", "{374DE290-123F-4565-9164-39C4925E467B}", "Favorites"} {
			if asciiFolderEqual(entry.name, name) {
				if path, ok := expandFolderProfile(entry.value, home); ok && filepath.IsAbs(path) {
					folders = append(folders, path)
				}
				break
			}
		}
	}
	return folders
}

func asciiFolderEqual(a, b string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range len(a) {
		x, y := a[i], b[i]
		if x >= 'A' && x <= 'Z' {
			x += 'a' - 'A'
		}
		if y >= 'A' && y <= 'Z' {
			y += 'a' - 'A'
		}
		if x != y {
			return false
		}
	}
	return true
}

func expandFolderProfile(raw string, home func() (string, bool)) (string, bool) {
	const prefix = "%USERPROFILE%"
	if len(raw) >= len(prefix) && asciiFolderEqual(raw[:len(prefix)], prefix) {
		base, ok := home()
		if !ok {
			return "", false
		}
		return filepath.Join(base, strings.TrimLeft(raw[len(prefix):], `\/`)), true
	}
	return raw, true
}

// Match the Rust enumerator: only string types, first NUL terminates text,
// one larger-data retry, and a failed item must not hide later entries.
func enumFolderStrings(root syscall.Handle, path string) []folderString {
	wide, err := syscall.UTF16PtrFromString(path)
	if err != nil {
		return nil
	}
	var key syscall.Handle
	if syscall.RegOpenKeyEx(root, wide, 0, syscall.KEY_QUERY_VALUE, &key) != nil {
		return nil
	}
	defer syscall.RegCloseKey(key)
	var out []folderString
	for index := uint32(0); ; index++ {
		name := make([]uint16, 512)
		data := make([]uint16, 4096)
		nameLen, dataLen, kind := uint32(len(name)), uint32(len(data)*2), uint32(0)
		call := func() uintptr {
			status, _, _ := enumFolderValue.Call(uintptr(key), uintptr(index), uintptr(unsafe.Pointer(&name[0])), uintptr(unsafe.Pointer(&nameLen)), 0, uintptr(unsafe.Pointer(&kind)), uintptr(unsafe.Pointer(&data[0])), uintptr(unsafe.Pointer(&dataLen)))
			return status
		}
		status := call()
		if status == 259 {
			break
		} // ERROR_NO_MORE_ITEMS
		if status == uintptr(syscall.ERROR_MORE_DATA) {
			data = make([]uint16, max((uint64(dataLen)+1)/2, 8192))
			nameLen, dataLen = uint32(len(name)), uint32(len(data)*2)
			status = call()
		}
		if status == 0 && (kind == syscall.REG_SZ || kind == syscall.REG_EXPAND_SZ) {
			out = append(out, folderString{syscall.UTF16ToString(name[:min(int(nameLen), len(name))]), syscall.UTF16ToString(data[:min(int(dataLen/2), len(data))])})
		}
	}
	return out
}
