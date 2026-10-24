package platform

import (
	"context"
	"encoding/binary"
	"unicode/utf16"

	"golang.org/x/sys/windows/registry"
)

// ReadWindowsAppRegistrations reads the official ARP views without deduplication,
// discovered apps, rules, filesystem enrichment or uninstall execution.
func ReadWindowsAppRegistrations(ctx context.Context) []AppRecord {
	var out []AppRecord
	for _, view := range []struct {
		key         registry.Key
		access      uint32
		label, path string
	}{
		{registry.LOCAL_MACHINE, registry.READ | registry.WOW64_64KEY, "HKLM", `SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall`},
		{registry.LOCAL_MACHINE, registry.READ | registry.WOW64_32KEY, "HKLM32", `SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall`},
		{registry.CURRENT_USER, registry.READ, "HKCU", `Software\Microsoft\Windows\CurrentVersion\Uninstall`},
	} {
		if ctx.Err() != nil {
			break
		}
		key, err := registry.OpenKey(view.key, view.path, view.access)
		if err != nil {
			continue
		}
		names, _ := key.ReadSubKeyNames(-1)
		key.Close()
		for _, name := range names {
			if ctx.Err() != nil {
				break
			}
			subpath := view.path + `\` + name
			sub, err := registry.OpenKey(view.key, subpath, view.access)
			if err != nil {
				continue
			}
			values := appRegistration{
				text: func(n string) *string { return appRegistryText(sub, n) },
				dword: func(n string) uint32 {
					data := appRegistryBytes(sub, n)
					if data == nil || len(data) > 4 {
						return 0
					}
					var buf [4]byte
					copy(buf[:], data)
					return binary.LittleEndian.Uint32(buf[:])
				},
			}
			app, ok := parseAppRegistration(values, name, view.label, subpath)
			sub.Close()
			if ok {
				out = append(out, app)
			}
		}
	}
	return out
}

// Rust's registry reader decodes raw UTF-16 regardless of registry value type
// and does not expand REG_EXPAND_SZ. Preserve even long uninstall commands.
func appRegistryText(key registry.Key, name string) *string {
	data := appRegistryBytes(key, name)
	if data == nil {
		return nil
	}
	words := make([]uint16, 0, len(data)/2)
	for len(data) >= 2 {
		c := binary.LittleEndian.Uint16(data)
		if c == 0 {
			break
		}
		words = append(words, c)
		data = data[2:]
	}
	s := string(utf16.Decode(words))
	return &s
}

func appRegistryBytes(key registry.Key, name string) []byte {
	n, _, err := key.GetValue(name, nil)
	if err != nil {
		return nil
	}
	data := make([]byte, n)
	n, _, err = key.GetValue(name, data)
	if err != nil {
		return nil
	}
	return data[:n]
}
