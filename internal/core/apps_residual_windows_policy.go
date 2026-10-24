package core

import (
	"context"
	"encoding/binary"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"sort"
	"strings"
	"unicode"
	"unicode/utf16"
	"unicode/utf8"

	"golang.org/x/text/cases"
	"golang.org/x/text/language"
)

type windowsResidualReader interface {
	KeyExists(root, path string) bool
	Subkeys(root, path string) []string
	Values(root, path string) map[string]string
	Value(root, path, name string) string
	Expand(string) string
	ShortcutTargets(path, executable string) bool
}
type windowsResidualRoots struct {
	data, shortcuts []string
	local, system   string
}
type windowsResidualContext struct {
	name, token, publisher, install string
	// shortToken is the display name with a trailing version removed, when that
	// differs from token; the half-length rule in matches needs it.
	shortToken string
	exes       []string
	snapshot   *RuleSnapshot
}

// windowsResidualShortName drops a trailing version from a display name.
//
// Windows installers commonly put the version in DisplayName ("MiniMax Code
// 3.0.74"), which lengthens the name enough that matches rejects the "MiniMax"
// folder the app leaves behind in %APPDATA%. Names whose remainder is too short
// or too generic keep their full form, so nothing is matched on "App".
func windowsResidualShortName(name string) string {
	fields := strings.Fields(name)
	for len(fields) > 1 {
		last := fields[len(fields)-1]
		if len(last) > 1 && (last[0] == 'v' || last[0] == 'V') && versionNumber(last[1:]) {
			last = last[1:]
		}
		if !versionNumber(last) {
			break
		}
		fields = fields[:len(fields)-1]
	}
	short := strings.Join(fields, " ")
	if short == name || !IsSafeAppToken(short) || utf8.RuneCountInString(windowsResidualName(short)) < 3 {
		return name
	}
	return short
}

// versionNumber reports whether s is dotted digits, as in "3.0.74" or "24".
func versionNumber(s string) bool {
	if s == "" {
		return false
	}
	for _, part := range strings.Split(s, ".") {
		if part == "" {
			return false
		}
		for _, r := range part {
			if r < '0' || r > '9' {
				return false
			}
		}
	}
	return true
}

func windowsResidualName(text string) string {
	return cases.Lower(language.Und, cases.HandleFinalSigma(false)).String(strings.Map(func(c rune) rune {
		if unicode.IsLetter(c) || unicode.IsNumber(c) || unicode.Is(unicode.Other_Alphabetic, c) {
			return c
		}
		return -1
	}, text))
}
func windowsResidualToken(text string) string {
	return strings.TrimSpace(strings.Map(func(c rune) rune {
		if unicode.IsLetter(c) || unicode.IsNumber(c) || unicode.Is(unicode.Other_Alphabetic, c) || c == '_' || c == '-' || c == ' ' {
			return c
		}
		return -1
	}, strings.TrimSpace(text)))
}
func (c windowsResidualContext) matchable() bool {
	return utf8.RuneCountInString(c.name) >= 3 && IsSafeAppToken(c.token)
}
func (c windowsResidualContext) matches(text string) bool {
	if !c.matchable() {
		return false
	}
	n := windowsResidualName(text)
	size := utf8.RuneCountInString(n)
	return size >= 3 && (strings.Contains(n, c.name) || strings.Contains(c.name, n) && size*2 >= utf8.RuneCountInString(c.name) && !slices.Contains(c.list("generic_folder_names"), n))
}
func (c windowsResidualContext) mentions(text string) bool {
	return c.install != "" && strings.Contains(strings.ReplaceAll(asciiLower(text), "/", `\`), c.install)
}
func (c windowsResidualContext) mentionsExe(text string) bool {
	text = asciiLower(text)
	return slices.ContainsFunc(c.exes, func(exe string) bool { return strings.Contains(text, exe) })
}
func (c windowsResidualContext) list(key string) []string {
	return c.snapshot.List("residual-windows", key)
}
func (c windowsResidualContext) anchor(key string) string {
	v := c.list(key)
	if len(v) == 0 {
		return ""
	}
	return v[0]
}
func newWindowsResidualContext(app InstalledApp, snapshot *RuleSnapshot) windowsResidualContext {
	name := windowsResidualShortName(app.Name)
	c := windowsResidualContext{name: windowsResidualName(name), token: windowsResidualToken(app.Name), publisher: windowsResidualToken(app.Publisher), snapshot: snapshot}
	if short := windowsResidualToken(name); short != c.token && IsSafeAppToken(short) {
		c.shortToken = short
	}
	if app.InstallLocation != nil && !IsSystemRootDir(*app.InstallLocation) {
		c.install = Norm(*app.InstallLocation)
		if len(c.install) <= 3 {
			c.install = ""
		}
	}
	push := func(name string) {
		name = asciiLower(name)
		if strings.HasSuffix(name, ".exe") && !slices.Contains(c.exes, name) {
			c.exes = append(c.exes, name)
		}
	}
	for _, value := range []*string{app.DisplayIcon, app.UninstallString} {
		if value != nil {
			text := strings.TrimSpace(strings.Trim(strings.SplitN(*value, ",", 2)[0], `"`))
			push(filepath.Base(text))
		}
	}
	if c.install != "" && app.InstallLocation != nil {
		for _, entry := range windowsResidualEntries(*app.InstallLocation, 300) {
			push(entry.Name())
		}
	}
	return c
}

func windowsResidualEntries(path string, limit int) []os.DirEntry {
	f, err := os.Open(path)
	if err != nil {
		return nil
	}
	defer f.Close()
	entries, _ := f.ReadDir(limit)
	return entries
}
func windowsResidualSize(ctx context.Context, path string) uint64 {
	var size uint64
	_ = filepath.WalkDir(path, func(_ string, entry fs.DirEntry, err error) error {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		if err == nil && entry.Type().IsRegular() {
			if md, err := entry.Info(); err == nil && md.Size() > 0 {
				size += uint64(md.Size())
			}
		}
		return nil
	})
	return size
}
func windowsResidualWalk(ctx context.Context, root string, depth, budget int, visit func(string, fs.DirEntry)) {
	count := 0
	_ = filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		if err != nil {
			return nil
		}
		rel, err := filepath.Rel(root, path)
		if err != nil || rel == "." {
			return nil
		}
		d := len(strings.Split(rel, string(filepath.Separator)))
		if d > depth {
			if entry.IsDir() {
				return filepath.SkipDir
			}
			return nil
		}
		if count >= budget {
			return fs.SkipAll
		}
		count++
		visit(path, entry)
		return nil
	})
}
func windowsResidualFile(ctx context.Context, path string, source ResidualSource, confidence Confidence, directory bool) (ResidualItem, bool) {
	md, err := os.Lstat(path)
	if err != nil || IsProtectedResidualPath(path) {
		return ResidualItem{}, false
	}
	kind := ResidualFile
	if directory || md.IsDir() {
		kind = ResidualDirectory
	}
	return NewResidualItem(ResidualKind{Type: kind, Path: path, Bytes: windowsResidualSize(ctx, path)}, source, confidence), true
}

func scanWindowsAppResiduals(ctx context.Context, app InstalledApp, reader windowsResidualReader, roots windowsResidualRoots) (ResidualScanResult, error) {
	result := ResidualScanResult{AppName: app.Name, AppID: app.ID}
	if err := ctx.Err(); err != nil {
		return result, err
	}
	snapshot, err := CurrentRules()
	if err != nil {
		return result, err
	}
	rule := NewRuleRef(snapshot, "residual-windows", "")
	if app.Discovery != nil && app.Discovery.Rule != nil {
		rule = *app.Discovery.Rule
		if rule.snapshot != nil {
			snapshot = rule.snapshot
		}
	}
	c := newWindowsResidualContext(app, snapshot)
	items := scanWindowsResidualFiles(ctx, app, c, reader, roots)
	items = append(items, scanWindowsResidualRegistry(ctx, app, c, reader)...)
	if app.Discovery != nil {
		for n := range items {
			i := &items[n]
			exactShortcut := i.Kind.Type == ResidualFile && slices.ContainsFunc(app.Discovery.Shortcuts, func(p string) bool { return Norm(p) == Norm(i.Kind.Path) }) && reader.ShortcutTargets(i.Kind.Path, app.Discovery.Executable)
			if i.Source != ResidualInstallDir && !exactShortcut {
				i.Confidence = Possible
			}
		}
		// Preserve the final discovery plan's frozen observations. Do not capture
		// fresh identities for artifacts whose absence/identity was already frozen.
		if plan := app.Discovery.Plan; plan != nil && plan.installation != nil {
			for n := range items {
				i := &items[n]
				observedShortcut := i.Source == ResidualShortcut && slices.ContainsFunc(app.Discovery.Shortcuts, func(p string) bool { return Norm(p) == Norm(i.Kind.Path) })
				if i.Source != ResidualInstallDir && !observedShortcut {
					continue
				}
				id, err := plan.installation.ScannedIdentity(i.Kind.Path)
				if err != nil {
					return result, err
				}
				i.Identity = id
			}
		}
	}
	items = DedupeResiduals(items)
	sort.SliceStable(items, func(i, j int) bool { return items[i].Confidence > items[j].Confidence })
	for n := range items {
		items[n].Rule = &rule
		result.TotalFileSize += items[n].Size()
	}
	result.Items = items
	return result, ctx.Err()
}

func verifyWindowsAppResiduals(ctx context.Context, items []ResidualItem, r windowsResidualReader, system string) ([]ResidualItem, error) {
	out := make([]ResidualItem, 0, len(items))
	for _, item := range items {
		if err := ctx.Err(); err != nil {
			return out, err
		}
		present := false
		switch item.Kind.Type {
		case ResidualFile, ResidualDirectory:
			_, err := os.Stat(item.Kind.Path)
			present = err == nil
			if present && item.Kind.Type == ResidualDirectory {
				item.Kind.Bytes = windowsResidualSize(ctx, item.Kind.Path)
			}
		case ResidualRegistryKey:
			present = r.KeyExists(item.Kind.Root.Label(), item.Kind.Path)
		case ResidualRegistryValue:
			for name := range r.Values(item.Kind.Root.Label(), item.Kind.Path) {
				present = present || strings.EqualFold(name, item.Kind.Name)
			}
		case ResidualScheduledTask:
			rel := strings.ReplaceAll(strings.TrimLeft(item.Kind.Path, `\`), `\`, string(filepath.Separator))
			if rel != "" {
				_, err := os.Stat(filepath.Join(system, "System32", "Tasks", rel))
				present = err == nil
			}
		}
		if present {
			out = append(out, item)
		}
	}
	return out, ctx.Err()
}

func windowsResidualTaskText(data []byte) string {
	if len(data) >= 2 && (data[0] == 255 && data[1] == 254 || data[0] == 254 && data[1] == 255) {
		var words []uint16
		var order binary.ByteOrder = binary.LittleEndian
		if data[0] == 254 {
			order = binary.BigEndian
		}
		for data = data[2:]; len(data) >= 2; data = data[2:] {
			words = append(words, order.Uint16(data))
		}
		return string(utf16.Decode(words))
	}
	return strings.ToValidUTF8(string(data), "\ufffd")
}
