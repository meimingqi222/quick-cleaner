package core

import (
	"context"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func isUUIDLike(value string) bool {
	parts := strings.Split(value, "-")
	lengths := []int{8, 4, 4, 4, 12}
	if len(parts) != len(lengths) {
		return false
	}
	for i, part := range parts {
		if len(part) != lengths[i] {
			return false
		}
		for _, c := range []byte(part) {
			if !(c >= 'a' && c <= 'f' || c >= 'A' && c <= 'F' || c >= '0' && c <= '9') {
				return false
			}
		}
	}
	return true
}

func orphanBundleID(name string, suffixes []string) (string, bool) {
	if strings.HasPrefix(name, ".") {
		return "", false
	}
	stem := name
	for _, suffix := range suffixes {
		if strings.HasSuffix(asciiLower(name), suffix) {
			stem = name[:len(name)-len(suffix)]
			break
		}
	}
	if at := strings.LastIndexByte(stem, '.'); at >= 0 && isUUIDLike(stem[at+1:]) {
		stem = stem[:at]
	}
	if !platform.ValidBundleID(stem) {
		return "", false
	}
	parts := strings.Split(stem, ".")
	tld := parts[0]
	if len(tld) < 2 || len(tld) > 8 {
		return "", false
	}
	for _, c := range []byte(tld) {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z') {
			return "", false
		}
	}
	if len(parts) == 2 {
		if !strings.ContainsAny(parts[1], "ABCDEFGHIJKLMNOPQRSTUVWXYZ") {
			return "", false
		}
	} else if len(tld) != 2 && !slices.Contains([]string{"com", "org", "net", "edu", "gov", "app", "dev", "info", "biz", "xyz", "pro", "cloud", "tech", "software", "tools", "games", "studio", "design", "online", "site", "name", "local", "group"}, asciiLower(tld)) {
		return "", false
	}
	return asciiLower(stem), true
}

func appleOwnedBundleID(id string) bool {
	return strings.HasPrefix(id, "com.apple.") || strings.Contains(id, ".apple.") || strings.HasSuffix(id, ".apple")
}

func orphanID(id string, installed map[string]struct{}) bool {
	if _, ok := installed[id]; ok {
		return false
	}
	parts := strings.Split(id, ".")
	for depth := 2; depth < len(parts); depth++ {
		prefix := strings.Join(parts[:depth], ".")
		if _, ok := installed[prefix]; ok {
			return false
		}
		for known := range installed {
			if strings.HasPrefix(known, prefix+".") {
				return false
			}
		}
	}
	for known := range installed {
		if strings.HasPrefix(known, id+".") {
			return false
		}
	}
	return true
}

func lowerBundleIDs(ids map[string]struct{}) map[string]struct{} {
	out := make(map[string]struct{}, len(ids))
	for id := range ids {
		out[asciiLower(id)] = struct{}{}
	}
	return out
}

func allocatedResidualSize(ctx context.Context, root string) uint64 {
	var size uint64
	_ = filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		if err == nil && entry.Type().IsRegular() {
			if info, err := entry.Info(); err == nil {
				size += platform.AllocatedFileSize(info)
			}
		}
		return nil
	})
	return size
}

func scanOrphanResidualsIn(ctx context.Context, home string, installed map[string]struct{}, snapshot *RuleSnapshot) (ResidualScanResult, error) {
	result := ResidualScanResult{Scope: ResidualScopeOrphanLeftovers}
	definition := snapshot.Definition("residual-macos")
	if definition == nil {
		return result, nil
	}
	rule := NewRuleRef(snapshot, "residual-macos", "")
	for _, location := range definition.Locations["orphan_roots"] {
		entries, err := os.ReadDir(filepath.Join(home, "Library", filepath.FromSlash(location.Path)))
		if err != nil {
			continue
		}
		for _, entry := range entries {
			if ctx.Err() != nil {
				return ResidualScanResult{}, ctx.Err()
			}
			id, ok := orphanBundleID(entry.Name(), definition.Lists["id_file_suffixes"])
			if !ok || appleOwnedBundleID(id) || !orphanID(id, installed) {
				continue
			}
			path := filepath.Join(home, "Library", filepath.FromSlash(location.Path), entry.Name())
			kind := ResidualKind{Type: ResidualFile, Path: path, Bytes: allocatedResidualSize(ctx, path)}
			if info, err := os.Stat(path); err == nil && info.IsDir() {
				kind.Type = ResidualDirectory
			}
			item := PossibleResidual(kind, location.Source).ForBundle(id)
			item.Rule = &rule
			item.RequiresConfirmation = true
			result.Items = append(result.Items, item)
		}
	}
	slices.SortStableFunc(result.Items, func(a, b ResidualItem) int {
		if a.Size() > b.Size() {
			return -1
		}
		if a.Size() < b.Size() {
			return 1
		}
		return 0
	})
	for _, item := range result.Items {
		result.TotalFileSize += item.Size()
	}
	return result, ctx.Err()
}

func classifyOrphanEvidence(result *ResidualScanResult, installed, removed map[string]struct{}) {
	result.Items = slices.DeleteFunc(result.Items, func(item ResidualItem) bool { id, ok := item.Owner(); return !ok || !orphanID(id, installed) })
	result.TotalFileSize = 0
	for i := range result.Items {
		item := &result.Items[i]
		id, _ := item.Owner()
		if _, ok := removed[id]; ok {
			item.Confidence = Certain
		}
		item.RequiresConfirmation = true
		result.TotalFileSize += item.Size()
	}
}

func orphanOwnersGone(items []ResidualItem, installed map[string]struct{}) bool {
	if len(items) == 0 {
		return false
	}
	for _, item := range items {
		id, ok := item.Owner()
		if !ok || !orphanID(id, installed) {
			return false
		}
	}
	return true
}
