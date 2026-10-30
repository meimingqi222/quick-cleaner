package core

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// FilesystemScanTargets collects declarations and filesystem-only providers.
// Native resources (trash, snapshots, Docker, brew) are separate inputs; this
// is not the complete product scan until those providers are connected.
func FilesystemScanTargets(s *RuleSnapshot, roots map[string]string, osName string) []ScanTarget {
	s = captureRuleSnapshot(s)
	claims := CollectRuleClaims(s, roots, osName)
	var targets []ScanTarget
	for _, provider := range s.List("engine", "providers") {
		switch provider {
		case "system":
			if osName == "windows" {
				targets = append(targets, windowsRecycleTargets(s)...)
			}
		case "cache":
			if home := roots["home"]; osName == "macos" && home != "" {
				targets = append(targets, userCacheTargets(s, filepath.Join(home, "Library/Caches"), claims)...)
			}
		case "browser":
			if local := roots["local"]; osName == "windows" && roots["home"] != "" && local != "" {
				if rule := s.Definition("browsers"); rule != nil {
					e := &ruleEnumeration{}
					for _, row := range rule.Catalogs["windows_user_data"] {
						targets = append(targets, chromiumProviderTargets(s, e, local, filepath.Join(local, row.Path), NewText(row.Zh, row.En), BrowserCache, "browser_cache", "chromium_declared")...)
					}
				}
			}
		case "macos":
			if osName == "macos" {
				targets = append(targets, externalVolumeTrashTargets(s)...)
				if home := roots["home"]; home != "" {
					targets = append(targets, brokenLoginItemTargets(s, home)...)
					targets = append(targets, oldIdeDataTargets(s, home)...)
				}
			}
		case "development":
			if home, roaming := roots["home"], roots["roaming"]; home != "" && roaming != "" {
				targets = append(targets, chromiumAppTargets(s, roaming, claims)...)
				if local := roots["local"]; osName == "windows" && local != "" {
					e := &ruleEnumeration{}
					children, _ := e.children(local, local)
					for _, child := range children {
						if child.directory {
							artifacts, _ := updaterTargets(s, e, local, child.path, updaterDisplayStem(s, child.name))
							targets = append(targets, artifacts...)
						}
					}
				}
			}
		}
	}
	targets = append(targets, RulePathTargets(s, roots, osName)...)
	targets = append(targets, RuleDirectoryTargets(s, roots, osName)...)
	targets = append(targets, RuleVersionTargets(s, roots, osName)...)
	return NormalizeScanTargets(targets)
}

// windowsRecycleTargets lists the original user's own recycle bin on every
// drive letter. Rust does not scan another account's SID, and a missing bin is
// not evidence of an empty one, so absent directories are skipped.
func windowsRecycleTargets(s *RuleSnapshot) []ScanTarget {
	sid, ok := platform.UserSID()
	if !ok || !strings.HasPrefix(sid, "S-1-") {
		return nil
	}
	var out []ScanTarget
	for _, dir := range platform.TrashDirectories() {
		info, err := os.Lstat(dir)
		if err != nil || !info.IsDir() {
			continue
		}
		letter := strings.ToUpper(dir[:1])
		out = append(out, ProviderTarget(s, "engine", "user_trash", dir, NewText(letter+": 回收站", letter+": Recycle Bin"), RecycleBinCategory, Operation{Kind: OpTrash}, false))
	}
	return out
}

// externalVolumeTrashTargets lists the current user's trash on each mounted
// volume. Files deleted on an external disk never appear in ~/.Trash; each
// volume keeps its own .Trashes/<uid>. The root volume's trash is ~/.Trash and
// is not repeated here.
func externalVolumeTrashTargets(s *RuleSnapshot) []ScanTarget {
	entries, err := os.ReadDir("/Volumes")
	if err != nil {
		return nil
	}
	uid := platform.CurrentUID()
	var out []ScanTarget
	for _, entry := range entries {
		volume := filepath.Join("/Volumes", entry.Name())
		trash := filepath.Join(volume, ".Trashes", uid)
		info, err := os.Lstat(trash)
		if err != nil || !info.IsDir() {
			continue
		}
		name := entry.Name()
		out = append(out, ProviderTarget(s, "engine", "volume_trash", trash, NewText("废纸篓 · "+name, "Trash · "+name), RecycleBinCategory, Operation{Kind: OpTrash}, false))
	}
	return out
}

// oldIdeDataTargets lists superseded JetBrains version directories. A product
// with one version, and anything whose name does not parse as a version, stays.
// The newest version of each product stays too.
func oldIdeDataTargets(s *RuleSnapshot, home string) []ScanTarget {
	def := s.Definition("macos")
	if def == nil {
		return nil
	}
	var out []ScanTarget
	for _, layout := range def.Catalogs["old_ide_roots"] {
		base := filepath.Join(home, layout.Path)
		entries, err := os.ReadDir(base)
		if err != nil {
			continue
		}
		var names []string
		for _, entry := range entries {
			names = append(names, entry.Name())
		}
		for _, old := range selectOldIdeVersions(s.List("macos", "jetbrains_products"), names) {
			out = append(out, ProviderTarget(s, "engine", "old_ide_data", filepath.Join(base, old.name), NewText(fmt.Sprintf("%s %d.%d（旧版本数据）", old.family, old.year, old.minor), fmt.Sprintf("%s %d.%d (old version data)", old.family, old.year, old.minor)), OldIdeData, Operation{Kind: OpTree}, false))
		}
	}
	return out
}

type oldIdeVersion struct {
	name, family string
	year, minor  int
}

func selectOldIdeVersions(products, names []string) []oldIdeVersion {
	parsed := make([]oldIdeVersion, 0, len(names))
	for _, name := range names {
		family, year, minor, ok := parseJetBrainsVersion(products, name)
		if ok {
			parsed = append(parsed, oldIdeVersion{name: name, family: family, year: year, minor: minor})
		}
	}
	var out []oldIdeVersion
	for _, item := range parsed {
		for _, other := range parsed {
			if other.family == item.family && (other.year > item.year || other.year == item.year && other.minor > item.minor) {
				out = append(out, item)
				break
			}
		}
	}
	return out
}

func parseJetBrainsVersion(products []string, name string) (string, int, int, bool) {
	folded := foldIdeName(name)
	for _, product := range products {
		key := foldIdeName(product)
		rest, ok := strings.CutPrefix(folded, key)
		if !ok || rest == "" {
			continue
		}
		parts := strings.Split(rest, ".")
		if len(parts) < 2 {
			continue
		}
		year, err := strconv.Atoi(parts[0])
		if err != nil {
			continue
		}
		minor, err := strconv.Atoi(parts[1])
		if err != nil {
			continue
		}
		if len(parts) == 3 {
			if _, err := strconv.Atoi(parts[2]); err != nil {
				continue
			}
		} else if len(parts) > 3 {
			continue
		}
		return product, year, minor, true
	}
	return "", 0, 0, false
}

func foldIdeName(name string) string {
	var b strings.Builder
	for _, r := range strings.ToLower(name) {
		if r != ' ' && r != '-' {
			b.WriteRune(r)
		}
	}
	return b.String()
}

// brokenLoginItemTargets lists LaunchAgent plists whose executable is gone.
// Parsing failure, a relative command, or a plist younger than the configured
// minimum age is not evidence: an update can briefly disagree with its plist,
// and a relative command may resolve through launchd's PATH.
func brokenLoginItemTargets(s *RuleSnapshot, home string) []ScanTarget {
	def := s.Definition("macos")
	if def == nil {
		return nil
	}
	age := time.Duration(s.Number("macos", "login_item_min_age_seconds", 86400)) * time.Second
	type root struct{ path string }
	var roots []root
	for _, row := range def.Catalogs["user_login_roots"] {
		roots = append(roots, root{filepath.Join(home, row.Path)})
	}
	for _, row := range def.Catalogs["system_login_roots"] {
		roots = append(roots, root{filepath.Join("/Library", row.Path)})
	}
	var out []ScanTarget
	for _, root := range roots {
		entries, err := os.ReadDir(root.path)
		if err != nil {
			continue
		}
		for _, entry := range entries {
			path := filepath.Join(root.path, entry.Name())
			info, err := os.Lstat(path)
			if err != nil || !info.Mode().IsRegular() || !strings.EqualFold(filepath.Ext(entry.Name()), ".plist") {
				continue
			}
			if time.Since(info.ModTime()) < age {
				continue
			}
			if !launchAgentExecutableMissing(path, launchAgentReader) {
				continue
			}
			name := entry.Name()
			out = append(out, ProviderTarget(s, "engine", "broken_login_item", path, NewText("损坏的登录项 · "+name, "Broken login item · "+name), BrokenLoginItems, Operation{Kind: OpFile}, true))
		}
	}
	return out
}

// launchAgentExecutableMissing reports whether a plist names an absolute
// executable that no longer exists. A plist that cannot be parsed, or one
// whose command is relative, returns false: absence of evidence is not a
// grant to delete.
// launchAgentReader is the plist source. Tests replace it; production calls plutil.
var launchAgentReader = readPlistJSON

func readPlistJSON(path string) ([]byte, error) {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	return exec.CommandContext(ctx, "/usr/bin/plutil", "-convert", "json", "-o", "-", path).Output()
}

func launchAgentExecutableMissing(path string, read func(string) ([]byte, error)) bool {
	output, err := read(path)
	if err != nil {
		return false
	}
	var value map[string]any
	if json.Unmarshal(output, &value) != nil {
		return false
	}
	program, _ := value["Program"].(string)
	if strings.TrimSpace(program) == "" {
		if args, ok := value["ProgramArguments"].([]any); ok && len(args) > 0 {
			program, _ = args[0].(string)
		}
	}
	program = strings.TrimSpace(program)
	if program == "" {
		return true
	}
	if !filepath.IsAbs(program) {
		return false
	}
	_, err = os.Lstat(program)
	return os.IsNotExist(err)
}

func updaterDisplayStem(s *RuleSnapshot, name string) string {
	for _, suffix := range s.List("cache", "updater_display_suffixes") {
		if strings.HasSuffix(name, suffix) {
			return strings.TrimSuffix(name, suffix)
		}
	}
	return name
}

func isUpdaterArtifact(s *RuleSnapshot, child ruleChild) bool {
	policy := "updater_files"
	if child.directory {
		policy = "updater_directories"
	}
	return s.MatchesName("cache", policy, child.name)
}

func updaterTargets(s *RuleSnapshot, e *ruleEnumeration, anchor, dir, stem string) ([]ScanTarget, bool) {
	children, ok := e.children(anchor, dir)
	if !ok {
		return nil, false
	}
	var targets []ScanTarget
	for _, child := range children {
		if !isUpdaterArtifact(s, child) {
			continue
		}
		stale := false
		if md, err := os.Lstat(child.path); err == nil {
			age := time.Since(md.ModTime())
			stale = age >= 0 && uint64(age/time.Second) >= s.Number("cache", "updater_stale_seconds", 604800)
		}
		targets = append(targets, ProviderTarget(s, "engine", "updater_artifact", child.path, SameText(stem+" · "+child.name), UpdaterPackages, Operation{Kind: OpTree}, stale))
	}
	return targets, len(targets) > 0
}

func userCacheTargets(s *RuleSnapshot, cache string, claims RuleClaims) []ScanTarget {
	e := &ruleEnumeration{}
	children, _ := e.children(cache, cache)
	var targets []ScanTarget
	for _, owner := range children {
		if !owner.directory || claims.Owns(cache, owner.name) || s.MatchesName("macos", "sensitive_cache", owner.name) {
			continue
		}
		claimed := claims.ClaimedChildren(cache, owner.name)
		probe := true
		for _, prefix := range s.List("cache", "updater_probe_exclude_prefixes") {
			probe = probe && !strings.HasPrefix(owner.name, prefix)
		}
		hit := false
		if probe {
			artifacts, matched := updaterTargets(s, e, cache, owner.path, updaterDisplayStem(s, owner.name))
			targets, hit = append(targets, artifacts...), matched
		}
		if hit || len(claimed) > 0 {
			residual, _ := e.children(cache, owner.path)
			for _, child := range residual {
				if !isUpdaterArtifact(s, child) && !slices.Contains(claimed, child.name) {
					targets = append(targets, ProviderTarget(s, "engine", "cache_candidate", child.path, SameText("~/Library/Caches/"+owner.name+"/"+child.name), UserTemp, Operation{Kind: OpContents}, false))
				}
			}
			continue
		}
		targets = append(targets, ProviderTarget(s, "engine", "cache_candidate", owner.path, SameText("~/Library/Caches/"+owner.name), UserTemp, Operation{Kind: OpContents}, false))
	}
	return targets
}

func chromiumProviderTargets(s *RuleSnapshot, e *ruleEnumeration, anchor, dir string, owner Text, category CategoryID, policy, signature string) []ScanTarget {
	var targets []ScanTarget
	for _, leaf := range chromiumLeaves(s, e, anchor, dir, signature) {
		label := NewText(owner.Get(Chinese)+" · "+leaf.trail, owner.Get(English)+" · "+leaf.trail)
		targets = append(targets, ProviderTarget(s, "engine", policy, leaf.path, label, category, Operation{Kind: OpContents}, chromiumLeafRecommended(s, leaf.name)))
	}
	return targets
}

func chromiumAppTargets(s *RuleSnapshot, roaming string, claims RuleClaims) []ScanTarget {
	e := &ruleEnumeration{}
	children, _ := e.children(roaming, roaming)
	var targets []ScanTarget
	push := func(child ruleChild) {
		category := UserCache
		for _, name := range s.List("development", "ai_app_dirs") {
			if strings.EqualFold(name, child.name) {
				category = AiAgents
				break
			}
		}
		targets = append(targets, chromiumProviderTargets(s, e, roaming, child.path, SameText(child.name), category, "development_candidate", "chromium")...)
	}
	for _, child := range children {
		if !child.directory || claims.Owns(roaming, child.name) {
			continue
		}
		claimed := claims.ClaimedChildren(roaming, child.name)
		if len(claimed) == 0 {
			push(child)
			continue
		}
		nested, _ := e.children(roaming, child.path)
		for _, sub := range nested {
			if sub.directory && !slices.Contains(claimed, sub.name) {
				push(sub)
			}
		}
	}
	return targets
}
