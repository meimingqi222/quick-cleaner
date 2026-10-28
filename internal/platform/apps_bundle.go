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

// macInstalledLocation reports whether an app bundle found by a Bundle ID query
// is an actual installation rather than a copy that happens to carry the same
// Info.plist.
//
// Spotlight happily indexes bundles that are not installs: an application that
// migrates its bundle keeps a full `.backup.app` under
// `~/Library/Caches/<id>.BundleMigration/backups/`, and that copy carries the
// very same CFBundleIdentifier. Counting it as an installation means a finished
// uninstall can never be confirmed — the leftover cache is reported as "the app
// is still installed", so residual cleanup is refused forever.
//
// Only locations that cannot hold an installation are excluded. Anything else,
// including paths this function cannot reason about, stays counted as installed:
// wrongly treating a live app as gone is the one failure that deletes user data.
func macInstalledLocation(path, home string, probes ...macVolumeProbe) bool {
	if !macRealBundle(path) {
		return false
	}
	sameVolume := macOnDataVolume
	if len(probes) > 0 && probes[0] != nil {
		sameVolume = probes[0]
	}
	if same, known := sameVolume(path); known && !same {
		return false
	}
	if home == "" {
		return true
	}
	// Caches never hold an installation, and the Trash is where installations
	// go to die: Spotlight still indexes a trashed .app for a while, so a
	// drag-to-Trash uninstall reads as "installed" unless Trash is excluded.
	// Anything else — including paths this function cannot reason about —
	// keeps counting as installed, because wrongly calling a live app gone is
	// the failure that deletes user data.
	return !macWithin(filepath.Join(home, "Library", "Caches"), path) &&
		!macWithin(filepath.Join(home, ".Trash"), path)
}

// macAnyInstallation reports whether a Bundle ID query returned at least one
// path that can hold an installation. The query already selects by identifier,
// so this only drops the copies Spotlight also indexes (cached bundle backups).
// Unreadable or unclassifiable paths still count: guessing "not installed" here
// would authorize deleting a live app's configuration.
func macAnyInstallation(stdout, home string, probes ...macVolumeProbe) bool {
	for path := range strings.SplitSeq(strings.TrimSpace(stdout), "\n") {
		if path = strings.TrimSpace(path); path != "" && macInstalledLocation(path, home, probes...) {
			return true
		}
	}
	return false
}

// macWithin reports whether path lies inside root. It compares resolved paths so
// an aliased Library directory cannot smuggle a real install into the exception.
func macWithin(root, path string) bool {
	inside := func(root, path string) bool {
		rel, err := filepath.Rel(root, path)
		return err == nil && rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator))
	}
	if inside(root, path) {
		return true
	}
	resolvedRoot, err := filepath.EvalSymlinks(root)
	if err != nil {
		return false
	}
	resolvedPath, err := filepath.EvalSymlinks(path)
	if err != nil {
		return false
	}
	return inside(resolvedRoot, resolvedPath)
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

// macSameVolume reports whether path is on this machine's data volume. The
// second result is false when the probe cannot tell — callers must then keep the
// path counted as live, because "could not check" must never grant removal.
type macVolumeProbe func(path string) (same bool, known bool)

func macRegisteredLive(ctx context.Context, records map[string][]string, trash string, read func(string) *string, probes ...macVolumeProbe) (map[string]struct{}, bool) {
	sameVolume := macOnDataVolume
	if len(probes) > 0 && probes[0] != nil {
		sameVolume = probes[0]
	}
	ids := make(map[string]struct{})
	if resolved, err := filepath.EvalSymlinks(trash); err == nil {
		trash = resolved
	}
	// Trash 与 Library 都是用户 HOME 的直接子项，从它的父目录推出 HOME。
	// 推导不出来就不做缓存排除——少一层过滤只会更保守。
	caches := ""
	if home := filepath.Dir(trash); filepath.Base(trash) == ".Trash" && home != "/" {
		caches = filepath.Join(home, "Library", "Caches")
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
			// 挂载卷（DMG、外接盘、网络卷）上的副本不是安装：LaunchServices
			// 会登记安装盘里那份，把它当成「仍安装」会让刚删掉主程序的用户
			// 永远清不掉残留。测不出卷身份时按安装计，绝不向放行删除的方向猜。
			if same, known := sameVolume(resolved); known && !same {
				continue
			}
			// 缓存里的 bundle 副本同理：应用做 bundle 迁移时会在
			// `~/Library/Caches/<id>.BundleMigration/backups/` 留一份完整的
			// `.backup.app`，它带着同一个 CFBundleIdentifier 并被 LaunchServices
			// 登记。那份副本是卸载后本该清理的残留，不是「应用还在」。
			if caches != "" && macWithin(caches, resolved) {
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
