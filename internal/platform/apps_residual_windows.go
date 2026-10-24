package platform

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"unicode/utf16"

	"golang.org/x/sys/windows/registry"
)

// WindowsResidualReader exposes read-only native facts, not ownership policy.
type WindowsResidualReader struct{ Context context.Context }

func residualRegistryView(root string) (registry.Key, uint32) {
	switch root {
	case "HKCU":
		return registry.CURRENT_USER, registry.READ
	case "HKLM32":
		return registry.LOCAL_MACHINE, registry.READ | registry.WOW64_32KEY
	default:
		return registry.LOCAL_MACHINE, registry.READ | registry.WOW64_64KEY
	}
}

func (r WindowsResidualReader) open(root, path string) (registry.Key, error) {
	if err := r.Context.Err(); err != nil {
		return 0, err
	}
	h, access := residualRegistryView(root)
	return registry.OpenKey(h, path, access)
}
func (r WindowsResidualReader) KeyExists(root, path string) bool {
	k, err := r.open(root, path)
	if err != nil {
		return false
	}
	k.Close()
	return true
}
func (r WindowsResidualReader) Subkeys(root, path string) []string {
	k, err := r.open(root, path)
	if err != nil {
		return nil
	}
	defer k.Close()
	names, _ := k.ReadSubKeyNames(-1)
	return names
}
func (r WindowsResidualReader) Values(root, path string) map[string]string {
	k, err := r.open(root, path)
	if err != nil {
		return nil
	}
	defer k.Close()
	names, _ := k.ReadValueNames(-1)
	out := make(map[string]string)
	for _, name := range names {
		if r.Context.Err() != nil {
			break
		}
		_, ty, err := k.GetValue(name, nil)
		if err != nil || ty != registry.SZ && ty != registry.EXPAND_SZ {
			continue
		}
		if value := appRegistryText(k, name); value != nil {
			out[name] = *value
		}
	}
	return out
}
func (r WindowsResidualReader) Value(root, path, name string) string {
	k, err := r.open(root, path)
	if err != nil {
		return ""
	}
	defer k.Close()
	if value := appRegistryText(k, name); value != nil {
		return *value
	}
	return ""
}
func (r WindowsResidualReader) Expand(raw string) string {
	text := strings.Trim(strings.TrimSpace(raw), `"`)
	if expanded, err := registry.ExpandString(text); err == nil && len(utf16.Encode([]rune(expanded))) < 1024 {
		text = expanded
	}
	return strings.ReplaceAll(strings.Map(func(c rune) rune {
		if c >= 'A' && c <= 'Z' {
			return c + ('a' - 'A')
		}
		return c
	}, text), "/", `\`)
}
func (r WindowsResidualReader) ShortcutTargets(path, executable string) bool {
	target, ok := ReadWindowsShortcutTarget(path)
	if !ok {
		return false
	}
	// Official uninstall may already have removed the target. Resolve the
	// existing ancestor, not a new executable or an inferred owning directory.
	var missing []string
	for range 256 {
		if canonical, ok := CanonicalAppPath(target); ok {
			for n := len(missing) - 1; n >= 0; n-- {
				canonical = filepath.Join(canonical, missing[n])
			}
			return strings.EqualFold(canonical, executable)
		}
		if _, err := os.Lstat(target); !os.IsNotExist(err) {
			return false
		}
		name, parent := filepath.Base(target), filepath.Dir(target)
		if name == "." || name == ".." || parent == target {
			return false
		}
		missing = append(missing, name)
		target = parent
	}
	return false
}

// Roots use trusted desktop-user directories, never the elevated process HOME.
func (r WindowsResidualReader) Roots() (data, shortcuts []string, local, system string) {
	if home, ok := UserHome(); ok {
		shortcuts = append(shortcuts, filepath.Join(home, "Desktop"))
	}
	if roaming, ok := UserDataDir(); ok {
		data = append(data, roaming)
		programs := filepath.Join(roaming, `Microsoft\Windows\Start Menu\Programs`)
		shortcuts = append(shortcuts, programs, filepath.Join(programs, "Startup"))
	}
	if p, ok := UserCacheDir(); ok {
		local = p
		data = append(data, p, filepath.Join(p, "Programs"), filepath.Join(p, `VirtualStore\Program Files`), filepath.Join(p, `VirtualStore\Program Files (x86)`))
	}
	for _, variable := range []string{"ProgramData", "ProgramFiles", "ProgramFiles(x86)"} {
		if p := os.Getenv(variable); p != "" {
			data = append(data, p)
		}
	}
	if p := os.Getenv("ProgramData"); p != "" {
		programs := filepath.Join(p, `Microsoft\Windows\Start Menu\Programs`)
		shortcuts = append(shortcuts, programs, filepath.Join(programs, "Startup"))
	}
	if p := os.Getenv("PUBLIC"); p != "" {
		shortcuts = append(shortcuts, filepath.Join(p, "Desktop"))
	}
	system = os.Getenv("SystemRoot")
	if system == "" {
		system = `C:\Windows`
	}
	return
}
