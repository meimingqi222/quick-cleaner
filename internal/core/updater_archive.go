package core

import (
	"archive/zip"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"golang.org/x/text/cases"
	"golang.org/x/text/language"
)

type ExtractLimits struct {
	MaxEntries    int
	MaxTotalBytes uint64
}

func DefaultExtractLimits() ExtractLimits { return ExtractLimits{10_000, 1024 * 1024 * 1024} }

type PayloadKind uint8

const (
	WindowsExe PayloadKind = iota
	MacApp
)

type PayloadSpec struct {
	Kind PayloadKind
	Name string
}

// CreateStagingDir removes only old update-staging-* siblings, never following
// symlinks, then exclusively creates a new staging directory.
func CreateStagingDir(parent string) (string, error) {
	if err := os.MkdirAll(parent, 0777); err != nil {
		return "", err
	}
	entries, err := os.ReadDir(parent)
	if err != nil {
		return "", err
	}
	for _, e := range entries {
		if strings.HasPrefix(e.Name(), "update-staging-") {
			_ = os.RemoveAll(filepath.Join(parent, e.Name()))
		}
	}
	nanos := time.Now().UnixNano()
	if nanos < 0 {
		nanos = 0
	}
	for attempt := 0; attempt < 16; attempt++ {
		path := filepath.Join(parent, fmt.Sprintf("update-staging-%d-%d-%d", os.Getpid(), nanos, attempt))
		err = os.Mkdir(path, 0777)
		if err == nil {
			return path, nil
		}
		if !os.IsExist(err) {
			return "", err
		}
	}
	return "", fmt.Errorf("could not create a unique staging directory")
}

type updaterZipEntry struct {
	file       *zip.File
	components []string
	dir        bool
	link       bool
	target     string
}

func updaterEntryError(name, reason string) error {
	return fmt.Errorf("zip entry %q rejected: %s", name, reason)
}
func updaterKey(parts []string) string {
	return cases.Lower(language.Und).String(strings.Join(parts, "/"))
}
func updaterComponents(raw string) ([]string, bool, string) {
	n := strings.ReplaceAll(raw, `\`, "/")
	if strings.HasPrefix(n, "/") {
		return nil, false, "absolute path"
	}
	trailing := strings.HasSuffix(n, "/")
	body := strings.TrimSuffix(n, "/")
	if body == "" {
		return nil, false, "empty name"
	}
	parts := strings.Split(body, "/")
	for i, c := range parts {
		switch c {
		case "":
			return nil, false, "empty path component"
		case ".":
			return nil, false, "relative path component"
		case "..":
			return nil, false, "path escapes destination"
		}
		if i == 0 && len(c) >= 2 && c[1] == ':' && ((c[0] >= 'a' && c[0] <= 'z') || (c[0] >= 'A' && c[0] <= 'Z')) {
			return nil, false, "absolute path (drive prefix)"
		}
		if strings.ContainsAny(c, `:<>"|?*`) || strings.ContainsFunc(c, unicode.IsControl) {
			return nil, false, "illegal character in path"
		}
		if strings.HasSuffix(c, ".") || strings.HasSuffix(c, " ") {
			return nil, false, "path component ends with a dot or space"
		}
		stem := strings.ToUpper(strings.TrimRightFunc(strings.SplitN(c, ".", 2)[0], unicode.IsSpace))
		if stem == "CON" || stem == "PRN" || stem == "AUX" || stem == "NUL" || (len(stem) == 4 && (strings.HasPrefix(stem, "COM") || strings.HasPrefix(stem, "LPT")) && stem[3] >= '1' && stem[3] <= '9') {
			return nil, false, "reserved device name"
		}
	}
	return parts, trailing, ""
}

func updaterCheckLinks(plan []updaterZipEntry) error {
	links := map[string]updaterZipEntry{}
	existing := map[string]bool{}
	for _, p := range plan {
		if p.link {
			links[updaterKey(p.components)] = p
		}
	}
	if len(links) == 0 {
		return nil
	}
	if runtime.GOOS == "windows" {
		for _, p := range plan {
			if p.link {
				return updaterEntryError(p.file.Name, "symlinks are not supported on this platform")
			}
		}
	}
	for _, p := range plan {
		for depth := 1; depth <= len(p.components); depth++ {
			k := updaterKey(p.components[:depth])
			if depth < len(p.components) {
				if _, ok := links[k]; ok {
					return updaterEntryError(p.file.Name, "entry is nested under a symlink")
				}
			}
			existing[k] = true
		}
	}
	var resolve func([]string, string, *int) ([]string, string)
	resolve = func(parent []string, target string, hops *int) ([]string, string) {
		if target == "" {
			return nil, "symlink target is empty"
		}
		if strings.HasPrefix(target, "/") {
			return nil, "symlink target is an absolute path"
		}
		if strings.Contains(target, `\`) || strings.ContainsFunc(target, unicode.IsControl) {
			return nil, "symlink target has illegal characters"
		}
		current := append([]string(nil), parent...)
		for _, c := range strings.Split(target, "/") {
			switch c {
			case "", ".":
			case "..":
				if len(current) == 0 {
					return nil, "symlink escapes destination"
				}
				current = current[:len(current)-1]
			default:
				current = append(current, c)
				if l, ok := links[updaterKey(current)]; ok {
					*hops++
					if *hops > 40 {
						return nil, "symlink chain is too deep"
					}
					var reason string
					current, reason = resolve(l.components[:len(l.components)-1], l.target, hops)
					if reason != "" {
						return nil, reason
					}
				}
			}
		}
		if len(current) == 0 {
			return nil, "symlink resolves to the archive root"
		}
		if !existing[updaterKey(current)] {
			return nil, "symlink target is missing from the archive"
		}
		return current, ""
	}
	for _, p := range plan {
		if p.link {
			hops := 0
			resolved, reason := resolve(p.components[:len(p.components)-1], p.target, &hops)
			if reason == "" && updaterKey(resolved[:1]) != updaterKey(p.components[:1]) {
				reason = "symlink resolves outside its top-level entry"
			}
			if reason != "" {
				return updaterEntryError(p.file.Name, reason)
			}
		}
	}
	return nil
}

// ExtractZip validates every entry (including link targets) before writing.
// Staging must be an empty real directory owned exclusively by this update.
func ExtractZip(zipPath, staging string, limits ExtractLimits) error {
	meta, err := os.Lstat(staging)
	if err != nil {
		return fmt.Errorf("inspect staging directory: %w", err)
	}
	if !meta.IsDir() {
		return fmt.Errorf("staging directory %s is not a real directory", staging)
	}
	entries, err := os.ReadDir(staging)
	if err != nil {
		return fmt.Errorf("read staging directory: %w", err)
	}
	if len(entries) > 0 {
		return fmt.Errorf("staging directory %s is not empty", staging)
	}
	z, err := zip.OpenReader(zipPath)
	if err != nil {
		return fmt.Errorf("read zip: %w", err)
	}
	defer z.Close()
	if len(z.File) > limits.MaxEntries {
		return fmt.Errorf("zip has more than %d entries", limits.MaxEntries)
	}
	tooLarge := func() error { return fmt.Errorf("zip expands beyond %d bytes", limits.MaxTotalBytes) }
	plan := make([]updaterZipEntry, 0, len(z.File))
	seen := map[string]bool{}
	var declared uint64
	for _, f := range z.File {
		parts, trailing, reason := updaterComponents(f.Name)
		if reason != "" {
			return updaterEntryError(f.Name, reason)
		}
		k := updaterKey(parts)
		if seen[k] {
			return updaterEntryError(f.Name, "duplicate path")
		}
		seen[k] = true
		p := updaterZipEntry{file: f, components: parts, link: f.Mode()&os.ModeSymlink != 0, dir: trailing || f.FileInfo().IsDir()}
		if p.link {
			r, e := f.Open()
			if e != nil {
				return updaterEntryError(f.Name, "symlink target is not readable UTF-8")
			}
			b, e := io.ReadAll(io.LimitReader(r, 1025))
			_ = r.Close()
			if e != nil || !utf8.Valid(b) {
				return updaterEntryError(f.Name, "symlink target is not readable UTF-8")
			}
			if len(b) > 1024 {
				return updaterEntryError(f.Name, "symlink target is too long")
			}
			p.target = string(b)
		} else if !p.dir {
			if f.UncompressedSize64 > limits.MaxTotalBytes-declared {
				return tooLarge()
			}
			declared += f.UncompressedSize64
		}
		plan = append(plan, p)
	}
	parents := map[string]bool{}
	for _, p := range plan {
		for d := 1; d < len(p.components); d++ {
			parents[updaterKey(p.components[:d])] = true
		}
	}
	for _, p := range plan {
		if !p.link && !p.dir && parents[updaterKey(p.components)] {
			return updaterEntryError(p.file.Name, "path is both a file and a directory")
		}
	}
	if err = updaterCheckLinks(plan); err != nil {
		return err
	}
	var written uint64
	for _, p := range plan {
		out := filepath.Join(append([]string{staging}, p.components...)...)
		if p.link {
			continue
		}
		if p.dir {
			if err = os.MkdirAll(out, 0777); err != nil {
				return fmt.Errorf("mkdir %s: %w", out, err)
			}
			continue
		}
		if err = os.MkdirAll(filepath.Dir(out), 0777); err != nil {
			return fmt.Errorf("mkdir %s: %w", filepath.Dir(out), err)
		}
		r, e := p.file.Open()
		if e != nil {
			return fmt.Errorf("read zip: %w", e)
		}
		f, e := os.OpenFile(out, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0666)
		if e != nil {
			_ = r.Close()
			return fmt.Errorf("create %s: %w", out, e)
		}
		remaining := limits.MaxTotalBytes - written
		// Avoid int64 overflow for callers supplying a larger-than-default limit.
		var n uint64
		buf := make([]byte, 64*1024)
		for {
			want := uint64(len(buf))
			if remaining-n < want {
				want = remaining - n + 1
			}
			read, re := r.Read(buf[:int(want)])
			if read > 0 {
				_, e = f.Write(buf[:read])
				n += uint64(read)
				if e != nil || n > remaining {
					break
				}
			}
			if re != nil {
				if re != io.EOF {
					e = re
				}
				break
			}
		}
		_ = r.Close()
		ce := f.Close()
		if e == nil {
			e = ce
		}
		if e != nil {
			return fmt.Errorf("write %s: %w", out, e)
		}
		if n > remaining {
			return tooLarge()
		}
		written += n
		if runtime.GOOS != "windows" && p.file.CreatorVersion>>8 == 3 {
			_ = os.Chmod(out, p.file.Mode().Perm())
		}
	}
	for _, p := range plan {
		if p.link {
			out := filepath.Join(append([]string{staging}, p.components...)...)
			if err = os.MkdirAll(filepath.Dir(out), 0777); err != nil {
				return fmt.Errorf("mkdir %s: %w", filepath.Dir(out), err)
			}
			if err = os.Symlink(p.target, out); err != nil {
				return fmt.Errorf("symlink %s: %w", out, err)
			}
		}
	}
	return nil
}

func FindPayload(root string, payload PayloadSpec) (string, error) {
	is := func(p string, dir bool) bool {
		m, e := os.Lstat(p)
		return e == nil && ((dir && m.IsDir()) || (!dir && m.Mode().IsRegular()))
	}
	matches := func(p string) bool {
		if payload.Kind == WindowsExe {
			return is(p, false)
		}
		return is(p, true) && is(filepath.Join(p, "Contents", "Info.plist"), false) && is(filepath.Join(p, "Contents", "MacOS"), true)
	}
	direct := filepath.Join(root, payload.Name)
	if matches(direct) {
		return direct, nil
	}
	dirs, err := os.ReadDir(root)
	if err != nil {
		return "", fmt.Errorf("read %s: %w", root, err)
	}
	for _, d := range dirs {
		if d.IsDir() {
			p := filepath.Join(root, d.Name(), payload.Name)
			if matches(p) {
				return p, nil
			}
		}
	}
	return "", fmt.Errorf("extracted zip does not contain %s", payload.Name)
}

func ExtractUpdatePayload(zipPath, staging string, payload PayloadSpec, limits ExtractLimits) (string, error) {
	if err := ExtractZip(zipPath, staging, limits); err != nil {
		return "", err
	}
	return FindPayload(staging, payload)
}

func ExtractUpdateZip(zipPath, cacheDir string) (string, error) {
	// Rust remove_dir_all refuses a symlink here; do not remove such a link.
	legacy := filepath.Join(cacheDir, "extracted")
	if m, e := os.Lstat(legacy); e == nil && m.IsDir() {
		_ = os.RemoveAll(legacy)
	}
	staging, err := CreateStagingDir(cacheDir)
	if err != nil {
		return "", fmt.Errorf("create extract dir: %w", err)
	}
	if err = ExtractZip(zipPath, staging, DefaultExtractLimits()); err != nil {
		return "", err
	}
	switch runtime.GOOS {
	case "windows":
		return FindPayload(staging, PayloadSpec{WindowsExe, "quick-cleaner.exe"})
	case "darwin":
		return FindPayload(staging, PayloadSpec{MacApp, "QuickCleaner.app"})
	default:
		return "", fmt.Errorf("unsupported platform for app update")
	}
}
