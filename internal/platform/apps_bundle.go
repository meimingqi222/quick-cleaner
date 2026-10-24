package platform

import (
	"bytes"
	"context"
	"math"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"

	"golang.org/x/text/cases"
	"golang.org/x/text/language"
)

func macSortAppRecords(apps []AppRecord) {
	// Rust's string lowercase includes contextual final sigma. Compute keys
	// after the workers finish; the stateful caser is local to this call.
	lower := cases.Lower(language.Und)
	keys := make(map[string]string, len(apps))
	for _, app := range apps {
		keys[app.Name] = lower.String(app.Name)
	}
	sort.SliceStable(apps, func(i, j int) bool { return keys[apps[i].Name] < keys[apps[j].Name] })
}

type macAppRoot struct {
	path   string
	system bool
}

func macAppRoots(home string) []macAppRoot {
	roots := []macAppRoot{{"/Applications", false}, {"/System/Applications", true}, {"/Applications/Utilities", false}, {"/Library/Input Methods", false}}
	if home != "" {
		roots = append(roots, macAppRoot{filepath.Join(home, "Applications"), false}, macAppRoot{filepath.Join(home, "Library/Input Methods"), false})
	}
	return roots
}

func macDefaultsValue(text, key string) *string {
	for line := range strings.SplitSeq(text, "\n") {
		if value, ok := strings.CutPrefix(strings.TrimSpace(line), key+" = "); ok {
			value = strings.Trim(strings.TrimSpace(strings.TrimRight(value, ";")), "\"")
			if value != "" {
				return &value
			}
		}
	}
	return nil
}

type macAppDate struct {
	date *string
	raw  uint64
}

func macMetadataDate(raw string) macAppDate {
	for _, layout := range []string{"2006-01-02 15:04:05 -0700", time.RFC3339} {
		if t, err := time.Parse(layout, strings.TrimSpace(raw)); err == nil && t.Unix() > 0 {
			s := t.Format("2006-01-02")
			return macAppDate{&s, uint64(t.Unix())}
		}
	}
	return macAppDate{}
}

func macSystemDate(t time.Time) macAppDate {
	if t.Unix() <= 0 {
		return macAppDate{}
	}
	s := t.In(time.Local).Format("2006-01-02")
	return macAppDate{&s, uint64(t.Unix())}
}

func macFallbackLastUsed(home, id string) macAppDate {
	var latest time.Time
	for _, rel := range []string{"Application Support/" + id, "Preferences/" + id + ".plist", "Caches/" + id, "Saved Application State/" + id + ".savedState", "HTTPStorages/" + id, "WebKit/" + id, "Containers/" + id} {
		if info, err := os.Stat(filepath.Join(home, "Library", rel)); err == nil && info.ModTime().After(latest) {
			latest = info.ModTime()
		}
	}
	if latest.Unix() <= 0 {
		return macAppDate{}
	}
	s := latest.UTC().Format("2006-01-02")
	return macAppDate{&s, uint64(latest.Unix())}
}

func macBundleQuery(id string) string {
	id = strings.ReplaceAll(strings.ReplaceAll(id, `\`, `\\`), `"`, `\"`)
	return `kMDItemCFBundleIdentifier == "` + id + `"`
}

func macRealBundle(path string) bool {
	if filepath.Ext(path) != ".app" {
		return false
	}
	info, err := os.Stat(filepath.Join(path, "Contents", "Info.plist"))
	return err == nil && info.Mode().IsRegular()
}

func macMetadataValues(stdout []byte, count int) ([]string, bool) {
	if !utf8.Valid(stdout) {
		return nil, false
	}
	values := bytes.Split(stdout, []byte{0})
	if len(values) > 0 && len(values[len(values)-1]) == 0 {
		values = values[:len(values)-1]
	}
	if len(values) != count {
		return nil, false
	}
	result := make([]string, len(values))
	for i, v := range values {
		result[i] = strings.TrimSpace(string(v))
	}
	return result, true
}

func macKnownID(id string) bool {
	return strings.TrimSpace(id) != "" && strings.TrimSpace(id) != "(null)"
}

func macIDsFromMetadata(paths []string, stdout []byte, read func(string) *string) (map[string]struct{}, bool) {
	values, ok := macMetadataValues(stdout, len(paths))
	if !ok {
		return nil, false
	}
	ids := make(map[string]struct{})
	for i, id := range values {
		if !macKnownID(id) {
			fallback := read(filepath.Join(paths[i], "Contents", "Info.plist"))
			if fallback == nil || !macKnownID(*fallback) {
				return nil, false
			}
			id = *fallback
		}
		ids[id] = struct{}{}
	}
	return ids, true
}

func macParseDU(stdout []byte) map[string]uint64 {
	result := map[string]uint64{}
	for line := range strings.SplitSeq(string(stdout), "\n") {
		size, path, ok := strings.Cut(line, "\t")
		if !ok {
			continue
		}
		n, err := strconv.ParseUint(strings.TrimSpace(size), 10, 64)
		if err != nil {
			continue
		}
		if n > math.MaxUint64/1024 {
			result[path] = math.MaxUint64
		} else {
			result[path] = n * 1024
		}
	}
	return result
}

func macGenericUninstaller(path string) *string {
	resources := filepath.Join(path, "Contents", "Resources")
	entries, _ := os.ReadDir(resources)
	for _, entry := range entries {
		candidate := filepath.Join(resources, entry.Name())
		if filepath.Ext(candidate) != ".app" || !strings.HasPrefix(strings.ToLower(strings.TrimSuffix(entry.Name(), ".app")), "uninstall") {
			continue
		}
		if info, err := os.Stat(candidate); err == nil && info.IsDir() {
			return &candidate
		}
	}
	return nil
}

// Registration paths are hints only; actual bundle identity is checked separately.
func macRegistrationRecords(text string) (map[string][]string, bool) {
	if !utf8.ValidString(text) {
		return nil, false
	}
	records := make(map[string][]string)
	for block := range strings.SplitSeq(text, strings.Repeat("-", 80)) {
		if !strings.HasPrefix(strings.TrimLeft(block, " \t\r\n"), "bundle id:") {
			continue
		}
		field := func(key string) string {
			for line := range strings.SplitSeq(block, "\n") {
				if v, ok := strings.CutPrefix(line, key); ok {
					return strings.TrimSpace(v)
				}
			}
			return ""
		}
		id := field("identifier:")
		if !ValidBundleID(id) {
			continue
		}
		path := field("path:")
		at := strings.LastIndex(path, " (0x")
		if at < 0 || !strings.HasSuffix(path, ")") {
			return nil, false
		}
		object := path[at+4 : len(path)-1]
		if object == "" {
			return nil, false
		}
		for _, c := range object {
			if !strings.ContainsRune("0123456789abcdefABCDEF", c) {
				return nil, false
			}
		}
		path = path[:at]
		if !filepath.IsAbs(path) {
			return nil, false
		}
		key := strings.ToLower(id)
		records[key] = append(records[key], path)
	}
	return records, len(records) > 0
}

func macRegisteredLive(ctx context.Context, records map[string][]string, trash string, read func(string) *string) (map[string]struct{}, bool) {
	ids := make(map[string]struct{})
	if resolved, err := filepath.EvalSymlinks(trash); err == nil {
		trash = resolved
	}
	for id, paths := range records {
		for _, path := range paths {
			if ctx.Err() != nil {
				return nil, false
			}
			plist := filepath.Join(path, "Contents", "Info.plist")
			info, err := os.Stat(plist)
			if os.IsNotExist(err) {
				continue
			}
			if err != nil || !info.Mode().IsRegular() {
				return nil, false
			}
			actual := read(plist)
			if actual == nil || !strings.EqualFold(*actual, id) {
				return nil, false
			}
			resolved, err := filepath.EvalSymlinks(path)
			if err != nil {
				return nil, false
			}
			rel, err := filepath.Rel(trash, resolved)
			if err == nil && rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
				continue
			}
			ids[*actual] = struct{}{}
		}
	}
	return ids, ctx.Err() == nil
}

type macBundleRunner func(context.Context, time.Duration, string, ...string) (ProcessResult, bool)

// ReadBundleMetadata reads an Info.plist file (or an application's bundle path).
// Native callers use defaults first; plutil extracts individual keys when
// CFPreferences cannot read the domain, including Date/Data-bearing plists.
func ReadBundleMetadata(path string) (*string, *string) {
	return macReadBundleMetadata(context.Background(), path, func(ctx context.Context, timeout time.Duration, program string, args ...string) (ProcessResult, bool) {
		return RunCommandTreeContext(ctx, exec.Command(program, args...), timeout)
	})
}

func macReadBundleMetadata(ctx context.Context, path string, run macBundleRunner) (*string, *string) {
	if filepath.Ext(path) == ".app" {
		path = filepath.Join(path, "Contents", "Info.plist")
	}
	path, err := filepath.EvalSymlinks(path)
	if err != nil {
		return nil, nil
	}
	path, err = filepath.Abs(path)
	if err != nil {
		return nil, nil
	}
	output, ok := run(ctx, 2*time.Second, "/usr/bin/defaults", "read", path)
	if ok && output.OK {
		return macDefaultsValue(string(output.Stdout), "CFBundleIdentifier"), macDefaultsValue(string(output.Stdout), "CFBundleShortVersionString")
	}
	return macPlistString(ctx, path, "CFBundleIdentifier", run), macPlistString(ctx, path, "CFBundleShortVersionString", run)
}

func macPlistString(ctx context.Context, path, key string, run macBundleRunner) *string {
	output, ok := run(ctx, 2*time.Second, "/usr/bin/plutil", "-extract", key, "raw", "-o", "-", path)
	if !ok || !output.OK || !utf8.Valid(output.Stdout) {
		return nil
	}
	s := strings.TrimSpace(string(output.Stdout))
	if s == "" {
		return nil
	}
	return &s
}
