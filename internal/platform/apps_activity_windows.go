package platform

import (
	"context"
	"encoding/binary"
	"io/fs"
	"os"
	"path/filepath"
	"strings"

	"golang.org/x/sys/windows"
	"golang.org/x/sys/windows/registry"
)

// ReadWindowsAppActivity combines UserAssist, BAM/DAM, Prefetch and RecentApps.
// Missing or inaccessible sources are skipped, never replaced with today's time.
func ReadWindowsAppActivity(ctx context.Context) AppActivityIndex {
	var index AppActivityIndex
	if ctx.Err() != nil {
		return index
	}
	const ua = `Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist`
	appActivitySubkeys(ctx, registry.CURRENT_USER, ua, func(path string) {
		appActivityValues(ctx, registry.CURRENT_USER, path+`\Count`, func(name string, data []byte) {
			if ts := appRunBlobUnix(data); ts > 0 {
				index.Insert(appUserAssistName(name), ts)
			}
		})
	})
	dos := make(map[string]string)
	for letter := 'A'; letter <= 'Z' && ctx.Err() == nil; letter++ {
		name, _ := windows.UTF16PtrFromString(string(letter) + ":")
		buf := make([]uint16, 1024)
		if n, err := windows.QueryDosDevice(name, &buf[0], uint32(len(buf))); err == nil && n > 0 {
			dos[strings.ToLower(windows.UTF16ToString(buf))] = strings.ToLower(string(letter) + ":")
		}
	}
	for _, root := range []string{`SYSTEM\CurrentControlSet\Services\bam\State\UserSettings`, `SYSTEM\CurrentControlSet\Services\bam\UserSettings`, `SYSTEM\CurrentControlSet\Services\dam\State\UserSettings`} {
		appActivitySubkeys(ctx, registry.LOCAL_MACHINE, root, func(path string) {
			appActivityValues(ctx, registry.LOCAL_MACHINE, path, func(name string, data []byte) {
				if len(data) < 8 || !strings.ContainsAny(name, `\/`) {
					return
				}
				if ts := appFiletimeUnix(binary.LittleEndian.Uint64(data)); ts > 0 {
					index.Insert(appNormalizeNTPath(name, dos), ts)
				}
			})
		})
	}
	root, ok := os.LookupEnv("SystemRoot")
	if !ok {
		root = `C:\Windows`
	}
	dir := filepath.Join(root, "Prefetch")
	entries, _ := os.ReadDir(dir)
	for _, entry := range entries {
		if ctx.Err() != nil {
			break
		}
		exe := appPrefetchExe(entry.Name())
		if exe == "" {
			continue
		}
		if info, err := os.Stat(filepath.Join(dir, entry.Name())); err == nil && info.ModTime().Unix() >= 0 {
			index.Insert(exe, uint64(info.ModTime().Unix()))
		}
	}
	const recent = `Software\Microsoft\Windows\CurrentVersion\Search\RecentApps`
	appActivitySubkeys(ctx, registry.CURRENT_USER, recent, func(path string) {
		key, err := registry.OpenKey(registry.CURRENT_USER, path, registry.READ)
		if err != nil {
			return
		}
		defer key.Close()
		id, data := appRegistryText(key, "AppId"), appRegistryBytes(key, "LastAccessedTime")
		if id != nil && len(data) >= 8 {
			if ts := appFiletimeUnix(binary.LittleEndian.Uint64(data)); ts > 0 {
				index.Insert(strings.ToLower(*id), ts)
			}
		}
	})
	return index
}

func appActivitySubkeys(ctx context.Context, root registry.Key, path string, visit func(string)) {
	if ctx.Err() != nil {
		return
	}
	key, err := registry.OpenKey(root, path, registry.READ)
	if err != nil {
		return
	}
	names, _ := key.ReadSubKeyNames(-1)
	key.Close()
	for _, name := range names {
		if ctx.Err() != nil {
			return
		}
		visit(path + `\` + name)
	}
}

func appActivityValues(ctx context.Context, root registry.Key, path string, visit func(string, []byte)) {
	if ctx.Err() != nil {
		return
	}
	key, err := registry.OpenKey(root, path, registry.READ)
	if err != nil {
		return
	}
	defer key.Close()
	names, _ := key.ReadValueNames(-1)
	for _, name := range names {
		if ctx.Err() != nil {
			return
		}
		if data := appRegistryBytes(key, name); data != nil {
			visit(name, data)
		}
	}
}

// ReadWindowsShortcutActivity preserves the legacy shortcut atime (mtime only
// when unavailable) evidence. NTFS may disable atime updates: this is not proof
// of inactivity and must not authorize deletion or automatic selection.
func ReadWindowsShortcutActivity(ctx context.Context) map[string]uint64 {
	dirs := []string{`C:\Users\Public\Desktop`, `C:\ProgramData\Microsoft\Windows\Start Menu\Programs`}
	if home, ok := UserHome(); ok {
		dirs = append(dirs, filepath.Join(home, "Desktop"))
	}
	if roaming, ok := UserDataDir(); ok {
		dirs = append(dirs, filepath.Join(roaming, `Microsoft\Windows\Start Menu\Programs`))
	}
	return appScanShortcutActivity(ctx, dirs)
}

func appScanShortcutActivity(ctx context.Context, dirs []string) map[string]uint64 {
	out := make(appTimestampMap)
	for _, base := range dirs {
		if ctx.Err() != nil {
			break
		}
		_ = filepath.WalkDir(base, func(path string, entry fs.DirEntry, err error) error {
			if ctx.Err() != nil {
				return ctx.Err()
			}
			if err != nil {
				return nil
			}
			rel, err := filepath.Rel(base, path)
			if err != nil {
				return nil
			}
			depth := 0
			if rel != "." {
				depth = strings.Count(rel, string(filepath.Separator)) + 1
			}
			if entry.IsDir() {
				if depth >= 3 {
					return filepath.SkipDir
				}
				return nil
			}
			if !entry.Type().IsRegular() || !strings.EqualFold(filepath.Ext(path), ".lnk") {
				return nil
			}
			info, err := entry.Info()
			if err != nil {
				return nil
			}
			_, accessed := AppFileTimes(info)
			t := accessed.Unix()
			if t < 0 {
				return nil
			}
			stem := strings.ToLower(strings.TrimSuffix(entry.Name(), filepath.Ext(path)))
			out[stem] = max(out[stem], uint64(t))
			for _, token := range ExtractAppTokens(stem) {
				out[token] = max(out[token], uint64(t))
			}
			return nil
		})
	}
	return out
}
