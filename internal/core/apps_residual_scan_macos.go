package core

import (
	"context"
	"encoding/json"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type macResidualRoots struct {
	home, systemLibrary, receipts, darwinCache, applications string
}

type macResidualEvidence struct {
	metadata   func(string) (*string, *string)
	groups     []string
	extensions []ResidualKind
}

type macResidualScan struct {
	ctx        context.Context
	app        InstalledApp
	roots      macResidualRoots
	definition *RuleDefinition
	items      []ResidualItem
}

func scanMacAppResidualsIn(ctx context.Context, app InstalledApp, roots macResidualRoots, evidence macResidualEvidence, snapshot *RuleSnapshot) (ResidualScanResult, error) {
	result := ResidualScanResult{AppName: app.Name, AppID: app.ID}
	definition := snapshot.Definition("residual-macos")
	if definition == nil {
		return result, ctx.Err()
	}
	s := macResidualScan{ctx: ctx, app: app, roots: roots, definition: definition}
	library := filepath.Join(roots.home, "Library")
	ids := macAppBundleIDs(ctx, app, evidence.metadata)
	for _, id := range ids {
		for _, row := range definition.Locations["satellite_dirs"] {
			confidence := Certain
			for _, review := range definition.Locations["review_id_dirs"] {
				if review.Path == row.Path {
					confidence = Possible
				}
			}
			s.entries(filepath.Join(library, row.Path), row.Source, confidence, func(name string) bool {
				return macBundleSatellite(name, id, definition.Lists["helper_id_suffixes"])
			})
		}
		if roots.darwinCache != "" {
			s.entries(roots.darwinCache, ResidualCacheDir, Certain, func(name string) bool {
				return macBundleSatellite(name, id, definition.Lists["helper_id_suffixes"])
			})
		}
		s.named(filepath.Join(library, "Application Scripts"), id, ResidualApplicationScript, Certain)
		recent := filepath.Join(library, "Application Support/com.apple.sharedfilelist/com.apple.LSSharedFileList.ApplicationRecentDocuments")
		for _, suffix := range []string{".sfl2", ".sfl3", ".sfl4"} {
			s.named(recent, id+suffix, ResidualRecentDocumentList, Certain)
		}
		s.named(filepath.Join(library, "Containers"), id, ResidualContainerDir, Certain)
		for _, suffix := range []string{".bom", ".plist"} {
			s.named(roots.receipts, id+suffix, ResidualPackageReceipt, Possible)
		}
	}
	// These names identify individual entries, not arbitrary relative paths.
	for _, name := range []string{app.Name, app.RegistrySubpath} {
		if name != "" && filepath.Base(name) == name && name != "." && name != ".." {
			s.add(filepath.Join(library, "Application Support", name), ResidualAppSupportDir, Certain)
		}
	}
	if app.Name != "" && filepath.Base(app.Name) == app.Name && app.Name != "." && app.Name != ".." {
		for _, entry := range []struct {
			root, suffix string
			source       ResidualSource
		}{
			{"Caches", "", ResidualCacheDir}, {"Logs", "", ResidualLogDir},
			{"Application Support", "", ResidualAppSupportDir}, {"Preferences", ".plist", ResidualPreferenceFile},
		} {
			root := filepath.Join(library, entry.root)
			if entry.root == "Caches" || entry.root == "Logs" {
				s.add(filepath.Join(root, app.Name), entry.source, Certain)
			}
			wanted := macConfigSlug(app.Name + entry.suffix)
			if len(wanted) >= 3 {
				s.entries(root, entry.source, Certain, func(name string) bool {
					return name != app.Name+entry.suffix && macConfigSlug(name) == wanted
				})
			}
		}
	}
	if len(app.Name) >= 2 {
		s.entries(filepath.Join(library, "Application Support/CrashReporter"), ResidualCrashDump, Certain, func(name string) bool {
			rest, ok := strings.CutPrefix(asciiLower(name), asciiLower(app.Name)+"_")
			uuid, suffix := strings.CutSuffix(rest, ".plist")
			return ok && suffix && isUUIDLike(uuid)
		})
		// CrashReporter directories are never crash metadata files.
		s.items = slices.DeleteFunc(s.items, func(i ResidualItem) bool { return i.Source == ResidualCrashDump && i.Kind.Type == ResidualDirectory })
	}
	s.reviewed(library, ids)
	s.messaging(library)
	s.messaging(roots.systemLibrary)
	for _, id := range evidence.groups {
		if platform.ValidBundleID(id) {
			s.named(filepath.Join(library, "Group Containers"), id, ResidualContainerDir, Certain)
			s.named(filepath.Join(library, "Application Scripts"), id, ResidualApplicationScript, Certain)
		}
	}
	prefixes := s.vendorPrefixes(ids)
	for _, location := range []struct{ key, root string }{{"user_family_dirs", library}, {"system_family_dirs", roots.systemLibrary}} {
		for _, row := range definition.Locations[location.key] {
			s.family(filepath.Join(location.root, row.Path), prefixes, row.Source)
		}
	}
	s.family(roots.receipts, prefixes, ResidualPackageReceipt)
	s.dotConfigs()
	for _, extension := range evidence.extensions {
		exact := slices.ContainsFunc(ids, func(id string) bool { return asciiLower(id) == asciiLower(extension.BundleID) })
		family := slices.ContainsFunc(prefixes, func(prefix string) bool { return macVendorFamily(extension.BundleID, prefix) })
		if extension.Type != ResidualSystemExtension || !exact && !family {
			continue
		}
		confidence := Possible
		if exact {
			confidence = Certain
		}
		item := NewResidualItem(extension, ResidualExtensionSource, confidence)
		item.RequiresConfirmation = true
		s.items = append(s.items, item)
	}
	// Resolve filesystem aliases before overlap accounting, preserving the first
	// observation just as the reference does. Never resolve an execution target.
	seen := map[string]bool{}
	s.items = slices.DeleteFunc(s.items, func(item ResidualItem) bool {
		if item.Kind.Type != ResidualFile && item.Kind.Type != ResidualDirectory {
			return false
		}
		key, known := platform.CanonicalAppPath(item.Kind.Path)
		if !known {
			key = item.Kind.Path
		}
		duplicate := seen[key]
		seen[key] = true
		return duplicate
	})
	result.Items = DedupeResiduals(s.items)
	rule := NewRuleRef(snapshot, "residual-macos", "")
	manualData := slices.Contains(definition.Lists["manual_app_support_bundle_ids"], app.RegistrySubpath)
	for index := range result.Items {
		item := &result.Items[index]
		item.Rule = &rule
		if manualData && item.Source == ResidualAppSupportDir || item.Source == ResidualContainerDir || item.Source == ResidualApplicationScript {
			item.RequiresConfirmation = true
		}
		result.TotalFileSize += item.Size()
	}
	if err := ctx.Err(); err != nil {
		return ResidualScanResult{}, err
	}
	return result, nil
}

func (s *macResidualScan) add(path string, source ResidualSource, confidence Confidence) {
	if s.ctx.Err() != nil {
		return
	}
	info, err := os.Stat(path)
	if err != nil {
		return
	}
	kind := ResidualKind{Type: ResidualFile, Path: path, Bytes: allocatedResidualSize(s.ctx, path)}
	if info.IsDir() {
		kind.Type = ResidualDirectory
	}
	s.items = append(s.items, NewResidualItem(kind, source, confidence))
}

func (s *macResidualScan) entries(root string, source ResidualSource, confidence Confidence, match func(string) bool) {
	entries, _ := os.ReadDir(root)
	for _, entry := range entries {
		if s.ctx.Err() != nil {
			return
		}
		if match(entry.Name()) {
			s.add(filepath.Join(root, entry.Name()), source, confidence)
		}
	}
}

func (s *macResidualScan) named(root, name string, source ResidualSource, confidence Confidence) {
	s.entries(root, source, confidence, func(value string) bool { return asciiLower(value) == asciiLower(name) })
}

func macBundleSatellite(name, bundleID string, suffixes []string) bool {
	if !platform.ValidBundleID(bundleID) {
		return false
	}
	stem := asciiLower(name)
	for _, suffix := range []string{".plist", ".binarycookies", ".savedstate", ".sfl2", ".sfl3", ".sfl4"} {
		if stripped, ok := strings.CutSuffix(stem, suffix); ok {
			stem = stripped
			break
		}
	}
	id := asciiLower(bundleID)
	if stem == id {
		return true
	}
	rest, ok := strings.CutPrefix(stem, id+".")
	if !ok {
		return false
	}
	if isUUIDLike(rest) {
		return true
	}
	for _, suffix := range suffixes {
		suffix = strings.TrimLeft(suffix, ".")
		if rest == suffix {
			return true
		}
		if uuid, ok := strings.CutPrefix(rest, suffix+"."); ok && isUUIDLike(uuid) {
			return true
		}
	}
	return false
}

func macAppBundleIDs(ctx context.Context, app InstalledApp, metadata func(string) (*string, *string)) []string {
	var ids []string
	if platform.ValidBundleID(app.RegistrySubpath) {
		ids = append(ids, app.RegistrySubpath)
	}
	if app.InstallLocation == nil || metadata == nil {
		return ids
	}
	contents := filepath.Join(*app.InstallLocation, "Contents")
	count := 0
	_ = filepath.WalkDir(contents, func(path string, entry fs.DirEntry, err error) error {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		if err != nil {
			return nil
		}
		rel, _ := filepath.Rel(contents, path)
		depth := len(strings.Split(rel, string(filepath.Separator)))
		if entry.IsDir() && depth >= 12 {
			return filepath.SkipDir
		}
		if !entry.Type().IsRegular() || entry.Name() != "Info.plist" {
			return nil
		}
		count++
		if count > 128 {
			return fs.SkipAll
		}
		root := filepath.Dir(filepath.Dir(path))
		ext := asciiLower(filepath.Ext(root))
		loginRel, _ := filepath.Rel(filepath.Join(contents, "Library/LoginItems"), root)
		login := loginRel != ".." && !strings.HasPrefix(loginRel, ".."+string(filepath.Separator)) && !filepath.IsAbs(loginRel)
		if root == *app.InstallLocation || ext != ".xpc" && ext != ".appex" && !(ext == ".app" && login) {
			return nil
		}
		id, _ := metadata(path)
		if id != nil && platform.ValidBundleID(*id) && !strings.HasPrefix(*id, "org.sparkle-project.") && !slices.ContainsFunc(ids, func(known string) bool { return asciiLower(known) == asciiLower(*id) }) {
			ids = append(ids, *id)
		}
		return nil
	})
	return ids
}

func (s *macResidualScan) review(library, path string, source ResidualSource) {
	confined, err := ConfinedRulePath(library, path)
	if err != nil || !confined || IsProtectedResidualPath(path) {
		return
	}
	info, err := os.Lstat(path)
	if err != nil {
		return
	}
	if info.IsDir() {
		s.add(path, source, Possible)
	} else if info.Mode().IsRegular() {
		s.items = append(s.items, PossibleResidual(ResidualKind{Type: ResidualFile, Path: path, Bytes: uint64(info.Size())}, source))
	}
}

func (s *macResidualScan) reviewed(library string, ids []string) {
	for _, key := range []string{"review_id_dirs", "review_dirs"} {
		for _, row := range s.definition.Locations[key] {
			root := filepath.Join(library, row.Path)
			entries, _ := os.ReadDir(root)
			for _, entry := range entries {
				owned := slices.ContainsFunc(ids, func(id string) bool { return asciiLower(entry.Name()) == asciiLower(id) })
				if owned || key == "review_dirs" && IsSafeAppToken(s.app.Name) && asciiLower(entry.Name()) == asciiLower(s.app.Name) {
					s.review(library, filepath.Join(root, entry.Name()), row.Source)
				}
			}
		}
	}
	for key, rows := range s.definition.Locations {
		id, direct := strings.CutPrefix(key, "app:")
		versioned := false
		if !direct {
			id, versioned = strings.CutPrefix(key, "versioned:")
		}
		if !platform.ValidBundleID(id) || asciiLower(id) != asciiLower(s.app.RegistrySubpath) || !direct && !versioned {
			continue
		}
		for _, row := range rows {
			path := filepath.Join(library, row.Path)
			if !versioned {
				s.review(library, path, row.Source)
				continue
			}
			dir, err := os.Open(filepath.Dir(path))
			if err != nil {
				continue
			}
			entries, _ := dir.ReadDir(4096)
			dir.Close()
			for _, entry := range entries {
				version, ok := strings.CutPrefix(asciiLower(entry.Name()), asciiLower(filepath.Base(path)))
				if ok && macNumericVersion(version) {
					s.review(library, filepath.Join(filepath.Dir(path), entry.Name()), row.Source)
				}
			}
		}
	}
}

func macNumericVersion(value string) bool {
	for _, part := range strings.Split(value, ".") {
		if part == "" {
			return false
		}
		for _, c := range []byte(part) {
			if c < '0' || c > '9' {
				return false
			}
		}
	}
	return true
}

func (s *macResidualScan) messaging(library string) {
	if s.app.InstallLocation == nil {
		return
	}
	for _, row := range s.definition.Locations["native_messaging_dirs"] {
		root := filepath.Join(library, row.Path)
		dir, err := os.Open(root)
		if err != nil {
			continue
		}
		entries, _ := dir.ReadDir(4096)
		dir.Close()
		for _, entry := range entries {
			path := filepath.Join(root, entry.Name())
			confined, err := ConfinedRuleFile(library, path)
			if filepath.Ext(path) != ".json" || err != nil || !confined {
				continue
			}
			data, err := readRuleEvidence(path)
			var manifest struct {
				Path string `json:"path"`
			}
			if err != nil || json.Unmarshal(data, &manifest) != nil || manifest.Path == "" {
				continue
			}
			if owned, err := ConfinedRuleFile(*s.app.InstallLocation, manifest.Path); owned && err == nil {
				s.review(library, path, row.Source)
			}
		}
	}
}

func (s *macResidualScan) vendorPrefixes(ids []string) []string {
	var prefixes []string
	for _, id := range ids {
		parts := strings.Split(id, ".")
		if len(parts) < 3 || parts[0] == "" || len(parts[1]) < 2 {
			continue
		}
		prefix := parts[0] + "." + parts[1]
		shared := slices.ContainsFunc(s.definition.Lists["shared_vendor_prefixes"], func(value string) bool { return asciiLower(value) == asciiLower(prefix) })
		if !shared && !slices.ContainsFunc(prefixes, func(value string) bool { return asciiLower(value) == asciiLower(prefix) }) {
			prefixes = append(prefixes, prefix)
		}
	}
	return prefixes
}

func macVendorFamily(name, prefix string) bool {
	name, prefix = asciiLower(name), asciiLower(prefix)
	return name == prefix || strings.HasPrefix(name, prefix+".")
}

func (s *macResidualScan) family(root string, prefixes []string, source ResidualSource) {
	if len(prefixes) != 0 {
		s.entries(root, source, Possible, func(name string) bool {
			return slices.ContainsFunc(prefixes, func(prefix string) bool { return macVendorFamily(name, prefix) })
		})
	}
}

func macConfigSlug(value string) string {
	return strings.Map(func(c rune) rune {
		if c >= 'A' && c <= 'Z' {
			return c + ('a' - 'A')
		}
		if c >= 'a' && c <= 'z' || c >= '0' && c <= '9' {
			return c
		}
		return -1
	}, value)
}

func (s *macResidualScan) dotConfigs() {
	slugs := []string{macConfigSlug(s.app.Name)}
	parts := strings.Split(s.app.RegistrySubpath, ".")
	slugs = append(slugs, macConfigSlug(parts[len(parts)-1]))
	slugs = slices.DeleteFunc(slugs, func(slug string) bool { return len(slug) < 4 })
	if len(slugs) == 0 {
		return
	}
	var others []string
	for _, root := range []string{s.roots.applications, filepath.Join(s.roots.home, "Applications")} {
		entries, _ := os.ReadDir(root)
		for _, entry := range entries {
			if stem, ok := strings.CutSuffix(entry.Name(), ".app"); ok {
				slug := macConfigSlug(stem)
				if slug != "" && slug != macConfigSlug(s.app.Name) {
					others = append(others, slug)
				}
			}
		}
	}
	var candidates []struct{ path, slug string }
	for _, parent := range s.definition.Lists["dot_config_parents"] {
		root := filepath.Join(s.roots.home, parent)
		entries, _ := os.ReadDir(root)
		for _, entry := range entries {
			name := entry.Name()
			if parent == "" {
				var ok bool
				name, ok = strings.CutPrefix(name, ".")
				if !ok {
					continue
				}
			}
			slug := macConfigSlug(name)
			if len(slug) < 4 || slices.Contains(s.definition.Lists["protected_dot_dirs"], slug) {
				continue
			}
			path := filepath.Join(root, entry.Name())
			if info, err := os.Stat(path); err == nil && info.IsDir() {
				candidates = append(candidates, struct{ path, slug string }{path, slug})
			}
		}
	}
	exact := slices.ContainsFunc(candidates, func(candidate struct{ path, slug string }) bool { return slices.Contains(slugs, candidate.slug) })
	for _, candidate := range candidates {
		family := !exact && len(candidate.slug) >= 5 && slices.ContainsFunc(slugs, func(slug string) bool { return strings.HasPrefix(slug, candidate.slug) }) &&
			!slices.ContainsFunc(others, func(slug string) bool { return strings.HasPrefix(slug, candidate.slug) })
		if slices.Contains(slugs, candidate.slug) || family {
			s.add(candidate.path, ResidualDotConfigDir, Possible)
		}
	}
}

func verifyMacResidualsWith(ctx context.Context, items []ResidualItem, extensions []ResidualKind, extensionsKnown bool) ([]ResidualItem, error) {
	remaining := make([]ResidualItem, 0, len(items))
	for _, item := range items {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		switch item.Kind.Type {
		case ResidualFile, ResidualDirectory:
			if _, err := os.Lstat(item.Kind.Path); os.IsNotExist(err) {
				continue
			}
			item.Kind.Bytes = allocatedResidualSize(ctx, item.Kind.Path)
			// The official uninstaller may legitimately rebuild a candidate. The
			// confirmation screen must observe that object, not authorize the old one.
			item.Identity = nil
			if identity, ok := CaptureIdentity(item.Kind.Path); ok {
				item.Identity = &identity
			}
		case ResidualSystemExtension:
			if extensionsKnown && !slices.ContainsFunc(extensions, func(extension ResidualKind) bool {
				return asciiLower(extension.BundleID) == asciiLower(item.Kind.BundleID)
			}) {
				continue
			}
		}
		remaining = append(remaining, item)
	}
	return remaining, ctx.Err()
}
