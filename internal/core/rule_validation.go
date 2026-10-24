package core

import (
	"errors"
	"fmt"
	"reflect"
	"strings"
	"unicode"

	bundled "github.com/meimingqi222/quick-cleaner/rules"
	"github.com/pelletier/go-toml/v2"
)

var ruleCategoryNames = strings.Fields("SystemTemp UserTemp UserCache BrowserCache PackageCache UpdaterPackages Logs RecycleBin Thumbnails BrokenLoginItems AiAgents DevBuild DevWorktrees LocalSnapshots IosBackup OldIdeData DockerImages")

func ruleName(s string) bool {
	return strings.TrimSpace(s) != "" && s != "." && s != ".." && len(s) <= 128 && !strings.ContainsAny(s, "/\\:\x00\n\r")
}
func ruleNames(v []string) bool {
	if len(v) > 128 {
		return false
	}
	for _, s := range v {
		if !ruleName(s) {
			return false
		}
	}
	return true
}
func ruleLabel(s string, tokens []string) bool {
	rest := s
	for _, t := range tokens {
		rest = strings.ReplaceAll(rest, "{"+t+"}", "")
	}
	return strings.TrimSpace(s) != "" && len(s) <= 1024 && !strings.ContainsAny(rest, "{}\x00\n\r")
}
func ruleCategory(s string) bool { return member(s, ruleCategoryNames) }
func validateRuleDeclarations(r *RuleDefinition) error {
	bad := func(s string) error { return errors.New("invalid " + s) }
	if len(r.Directories) > 32 || len(r.Directories) > 0 && (len(r.Variables) > 0 || !member("directory_selection", r.Required)) {
		return bad("directory capability")
	}
	ids := map[string]bool{}
	for _, d := range r.Directories {
		if !ruleName(d.ID) || ids[d.ID] || len(d.Paths) == 0 || len(d.Paths) > 32 || !member(string(d.Root), []string{"home", "roaming", "local", "user_temp_parent"}) || !ruleCategory(d.Category) || d.Disposal != "permanent" || !member("file", r.Required) {
			return bad("directory")
		}
		ids[d.ID] = true
		tokens := []string{"name", "parent", "relative"}
		seen := map[string]bool{}
		for _, p := range d.Paths {
			if !RuleRelative(p) {
				return bad("directory path")
			}
			parts := strings.Split(strings.ReplaceAll(p, `\`, "/"), "/")
			if len(parts) > 8 {
				return bad("template depth")
			}
			count, total := 0, 1
			for _, part := range parts {
				if strings.HasPrefix(part, "{") && strings.HasSuffix(part, "}") {
					key := part[1 : len(part)-1]
					if !ruleName(key) || member(key, []string{"name", "parent", "relative"}) || seen[key] {
						return bad("template token")
					}
					seen[key] = true
					count++
					n := 0
					if rows, ok := r.Catalogs[key]; ok {
						n = len(rows)
					} else if ruleNames(r.Lists[key]) {
						n = len(r.Lists[key])
					}
					total *= n
					if count > 2 || n == 0 || total > 64 {
						return bad("template expansion")
					}
					tokens = append(tokens, key)
				} else if strings.ContainsAny(part, "{}") {
					return bad("partial template")
				}
			}
		}
		s := d.Select
		valid := false
		noLog := d.LogPolicy == nil
		switch s.Kind {
		case "children":
			valid = ruleNames(s.Prefixes) && d.Operation == "contents"
		case "container_directories":
			valid = len(s.Paths) > 0 && len(s.Paths) <= 32 && ruleName(s.ExcludeNamePolicy) && d.Operation == "contents" && noLog
			for _, p := range s.Paths {
				valid = valid && RuleRelative(p)
			}
			found := false
			for _, mode := range []string{"exact", "prefixes", "contains", "suffixes"} {
				a := r.Lists[s.ExcludeNamePolicy+"_"+mode]
				found = found || len(a) > 0 && ruleNames(a)
			}
			valid = valid && found
		case "named_files", "named_directories":
			op := PathOperation("file")
			if s.Kind == "named_directories" {
				op = "contents"
			}
			valid = ruleName(s.Name) && s.ChildDepth <= 1 && d.Operation == op && noLog
		case "files":
			valid = ruleNames(s.Prefixes) && ruleNames(s.Suffixes) && (len(s.Prefixes) > 0 || len(s.Suffixes) > 0) && d.Operation == "file" && noLog
		case "path":
			valid = (d.Operation == "contents" && s.DirectoriesOnly || d.Operation == "file" && !s.DirectoriesOnly) && noLog
		case "manifest_children":
			valid = ruleName(s.Manifest) && len(s.Manifest) <= 64 && d.Operation == "tree" && noLog
		case "orphaned_children":
			valid = ruleName(s.Manifest) && len(s.Manifest) <= 64 && validPointer(s.Pointer) && len(s.Pointer) <= 64 && len(s.URIPrefix) > 0 && len(s.URIPrefix) <= 32 && member(string(s.Scope), []string{"home", "roaming"}) && d.Operation == "tree" && noLog
			valid = valid && strings.Trim(s.Pointer, "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/_-") == "" && strings.Trim(s.URIPrefix, "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789:/._-") == ""
			tokens = append(tokens, "project")
		case "catalog_children":
			valid = d.Operation == "contents" && noLog && len(s.CatalogPolicy) <= 32 && strings.Contains(d.Zh, "{name}") && strings.Contains(d.En, "{name}")
			paths := map[string]bool{}
			for key, p := range s.CatalogPolicy {
				rows := r.Catalogs[key]
				valid = valid && len(rows) > 0 && len(rows) <= 128 && ruleCategory(p.Category)
				for _, row := range rows {
					valid = valid && !paths[row.Path]
					paths[row.Path] = true
				}
			}
			if p := s.LeafPolicy; p != nil {
				valid = valid && member(p.Signature, []string{"chromium", "chromium_declared"}) && ruleCategory(p.Category) && ruleLabel(p.Zh, []string{"name", "parent", "relative", "trail"}) && ruleLabel(p.En, []string{"name", "parent", "relative", "trail"})
			}
		case "layout_children":
			rows := r.Catalogs[s.Catalog]
			valid = d.Operation == "contents" && noLog && reflect.DeepEqual(d.Paths, []string{"{" + s.Catalog + "}"}) && len(rows) > 0 && len(rows) <= 128
			for _, row := range rows {
				valid = valid && len(row.Children) > 0
				for _, c := range row.Children {
					valid = valid && ruleName(c.Path)
				}
			}
		case "signature_leaves":
			valid = member(s.Signature, []string{"chromium", "chromium_declared"}) && d.Operation == "contents" && noLog
			tokens = append(tokens, "trail")
		case "linked_worktrees":
			valid = len(s.Containers) <= 4 && ruleNames(s.Containers) && d.Operation == "tree" && noLog
		}
		if !valid || !ruleLabel(d.Zh, tokens) || !ruleLabel(d.En, tokens) {
			return fmt.Errorf("invalid selection %s (%s)", d.ID, s.Kind)
		}
		if p := d.LogPolicy; p != nil {
			if len(p.Extensions) == 0 || !ruleNames(p.Extensions) || !ruleNames(p.ExcludeExact) || !ruleNames(p.ExcludePrefixes) {
				return bad("log policy")
			}
		}
	}
	if len(r.VersionLayouts) > 32 || len(r.VersionLayouts) > 0 && (len(r.Variables) > 0 || !member("file", r.Required) || !member("version_retention", r.Required)) {
		return bad("version capability")
	}
	ids = map[string]bool{}
	for _, v := range r.VersionLayouts {
		if !ruleName(v.ID) || ids[v.ID] || !member(string(v.Root), []string{"home", "roaming"}) || !member(string(v.Platform), []string{"", "all", "windows", "unix"}) || !RuleRelative(v.Path) || v.Versions != "." && !RuleRelative(v.Versions) || v.Current != nil && !RuleRelative(*v.Current) || v.Leaf != nil && !RuleRelative(*v.Leaf) || v.Current == nil && (v.Leaf == nil || len(v.Prefixes) == 0 || v.Downloads != nil) || v.Current != nil && v.Leaf != nil || !ruleNames(v.Prefixes) || !ruleNames(v.Exclude) || !ruleLabel(v.Zh, []string{"name"}) || !ruleLabel(v.En, []string{"name"}) || !ruleCategory(v.Category) || v.Operation != "contents" || v.Disposal != "permanent" {
			return bad("version layout")
		}
		ids[v.ID] = true
		if p := v.Lock; p != nil {
			if !RuleRelative(p.Path) || p.MaximumAgeSeconds < 60 || p.MaximumAgeSeconds > 86400 {
				return bad("version lock")
			}
		}
		if p := v.Downloads; p != nil {
			if !RuleRelative(p.Path) || len(p.Suffixes) == 0 || !ruleNames(p.Suffixes) || !ruleLabel(p.Zh, []string{"name"}) || !ruleLabel(p.En, []string{"name"}) || p.Operation != "file" || p.Disposal != "permanent" {
				return bad("version downloads")
			}
		}
	}
	if a := r.App; a != nil {
		if !reflect.DeepEqual(a.Actions, []SourceAction{"revalidate", "official_operation", "supplement_cleanup", "verify_completion", "remove_recovery_records"}) || a.Completion != "artifacts_and_registrations_absent" {
			return bad("source lifecycle")
		}
		for _, cap := range []string{"source_install", "python_module", "registration", "source_flow"} {
			if !member(cap, r.Required) {
				return bad("installation capability")
			}
		}
		if a.ModuleFile != strings.ReplaceAll(a.Module, ".", "/")+".py" || len(a.ModuleArgs) > 32 || !ruleName(a.EnvironmentName) || a.PythonPrefix == "" || a.BootstrapPrefix == "" || a.InstallerProcess == "" || len(a.Targets) == 0 || a.SourceSignature == "" || a.InstallerSignature == "" || a.TaskPrefix == "" || strings.ContainsAny(a.TaskPrefix, "*?") {
			return bad("installation parameters")
		}
		for _, s := range strings.Split(a.Module, ".") {
			if !ruleIdentifier(s) {
				return bad("module")
			}
		}
		for _, arg := range a.ModuleArgs {
			if arg == "" || member(arg, []string{"-c", "-m", "--command", "--eval", "--exec", "--script", "--code"}) || strings.Trim(arg, "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_.") != "" {
				return bad("module argument")
			}
		}
		paths := []string{a.DefaultHome, a.Source, a.SourceFile, a.Constants, a.SourceOwner, a.Stamp, a.StateDir, a.StateFacts, a.Environments, a.ToolsDir, a.ToolFacts, a.ToolLock, a.Profiles, a.ModuleFile, a.Installer, a.BootstrapDir, a.GatewayDir}
		for _, list := range [][]string{a.Targets, a.Launchers, a.GatewayFiles, a.PathPrefixes, a.Preserve} {
			paths = append(paths, list...)
		}
		for _, p := range paths {
			if !RuleRelative(p) {
				return bad("installation path")
			}
		}
		for _, v := range append(append([]string{}, a.EnvironmentVariables...), a.HomeVariable) {
			if !ruleIdentifier(v) {
				return bad("environment variable")
			}
		}
		for _, launcher := range a.Launchers {
			for _, keep := range a.Preserve {
				if launcher == keep || strings.HasPrefix(launcher, keep+"/") {
					return bad("preserved launcher overlap")
				}
			}
		}
	}
	if len(r.AppDataAliases) > 128 || len(r.ScriptUninstallers) > 128 || len(r.UninstallDataWarnings) > 128 {
		return bad("registration resource limit")
	}
	for _, a := range r.AppDataAliases {
		if r.Platform != "windows" || !registrationToken(a.RegistryID) || strings.TrimSpace(a.Name) == "" || len(a.Name) > 256 || strings.TrimSpace(a.Publisher) == "" || len(a.Publisher) > 256 || !ruleName(a.Directory) || strings.Trim(a.Directory, "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-") != "" {
			return bad("app data alias")
		}
		if len(a.Directory) < 3 || member(strings.ToLower(a.Directory), strings.Fields("app apps microsoft windows system system32 appdata local roaming locallow programdata temp tmp users google apple intel amd nvidia adobe tencent")) {
			return bad("generic app data alias")
		}
	}
	ids = map[string]bool{}
	for _, a := range r.ScriptUninstallers {
		if r.ID != "residual-macos" || r.Platform != "macos" || !registrationToken(a.BundleID) || ids[a.BundleID] || !RuleRelative(a.Script) || strings.Contains(a.Script, `\`) || !strings.HasPrefix(a.Script, "Contents/") || len(a.SHA256) == 0 || len(a.SHA256) > 16 || !member(string(a.Interpreter), []string{"sh", "bash"}) || len(a.Arguments) > 16 || member(a.BundleID, r.Lists["manual_uninstall_bundle_ids"]) {
			return bad("script uninstaller")
		}
		ids[a.BundleID] = true
		for _, h := range a.SHA256 {
			if len(h) != 64 || strings.Trim(h, "0123456789abcdef") != "" {
				return bad("script digest")
			}
		}
		for _, s := range a.Arguments {
			if len(s) > 256 || strings.IndexFunc(s, unicode.IsControl) >= 0 {
				return bad("script argument")
			}
		}
		if len(a.UserBundles)+len(a.SystemBundles) == 0 || len(a.UserBundles)+len(a.SystemBundles) > 16 {
			return bad("script bundles")
		}
		for _, list := range [][]string{a.UserBundles, a.SystemBundles} {
			seen := map[string]bool{}
			for _, p := range list {
				if !RuleRelative(p) || strings.Contains(p, `\`) || !strings.HasSuffix(p, ".app") || !strings.HasPrefix(p, "Applications/") && !strings.HasPrefix(p, "Library/") || seen[p] {
					return bad("script bundle")
				}
				seen[p] = true
			}
		}
	}
	if list, ok := r.Lists["manual_uninstall_bundle_ids"]; ok {
		if r.ID != "residual-macos" || r.Platform != "macos" || len(list) > 128 {
			return bad("manual uninstall identities")
		}
		for _, s := range list {
			if !registrationToken(s) {
				return bad("manual identity")
			}
		}
	}
	ids = map[string]bool{}
	for _, a := range r.UninstallDataWarnings {
		if r.ID != "residual-windows" || r.Platform != "windows" || !registrationToken(a.RegistryID) || !registrationToken(a.Name) || !registrationToken(a.Publisher) || ids[a.RegistryID] {
			return bad("uninstall warning")
		}
		ids[a.RegistryID] = true
	}
	return nil
}
func registrationToken(s string) bool {
	return strings.TrimSpace(s) != "" && len(s) <= 256 && !strings.ContainsAny(s, `/\`) && strings.IndexFunc(s, unicode.IsControl) < 0
}
func validateRulePolicies(b RuleBundle, e *RuleDefinition) error {
	bad := func(s string) error { return errors.New("invalid policy: " + s) }
	if len(e.Categories) != len(ruleCategoryNames) {
		return bad("category coverage")
	}
	for _, name := range ruleCategoryNames {
		p, ok := e.Categories[name]
		if !ok || !member(string(p.Safety), []string{"safe", "caution", "danger"}) || !member(string(p.Group), []string{"general", "developer"}) {
			return bad("category policy")
		}
	}
	for _, key := range strings.Fields("browser_cache cache_candidate development_candidate docker_image broken_login_item local_snapshot old_ide_data volume_trash user_trash updater_artifact brew_cleanup") {
		if _, ok := e.ProviderPolicies[key]; !ok {
			return bad("missing provider " + key)
		}
	}
	for key, bounds := range map[string][2]uint64{"photo_distance": {0, 5}, "photo_burst_distance": {0, 8}, "photo_burst_seconds": {0, 10}, "photo_min_size": {1, 200000000}, "photo_max_size": {1, 200000000}, "photo_max_depth": {1, 20}, "photo_candidate_limit": {2, 1500}, "photo_group_limit": {1, 30}, "photo_group_size_limit": {2, 12}, "photo_supplement_below": {0, 1500}, "duplicate_min_size": {65536, ^uint64(0)}, "duplicate_bucket_limit": {1, 500}, "large_result_limit": {1, 500}, "large_min_size": {1, ^uint64(0)}, "large_selected_age_days": {1, 36500}, "download_max_depth": {1, 4}, "default_min_size_filter": {1, ^uint64(0)}, "default_age_filter_months": {0, 1200}} {
		if n, ok := e.Numbers[key]; ok && (n < bounds[0] || n > bounds[1]) {
			return bad(key)
		}
	}
	snap := RuleSnapshot{Bundle: b}
	if snap.Number("engine", "photo_min_size", 10000) > snap.Number("engine", "photo_max_size", 200000000) || snap.Number("engine", "photo_supplement_below", 500) > snap.Number("engine", "photo_candidate_limit", 1500) {
		return bad("inconsistent photo bounds")
	}
	for _, family := range []string{"download_", "large_", "duplicate_"} {
		seen := map[string]bool{}
		for k, list := range e.Lists {
			if strings.HasPrefix(k, family) {
				for _, s := range list {
					if s == "" || strings.Trim(s, "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789") != "" || seen[s] {
						return bad("file classification")
					}
					seen[s] = true
				}
			}
		}
	}
	for _, p := range e.Lists["providers"] {
		if !member(p, []string{"system", "cache", "browser", "development", "docker", "macos"}) {
			return bad("provider")
		}
	}
	if _, ok := e.Lists["providers"]; !ok {
		return bad("providers")
	}
	if len(e.Lists["photo_extensions"]) == 0 {
		return bad("photo extensions")
	}
	for _, s := range e.Lists["photo_extensions"] {
		if s == "" || strings.Trim(s, "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789") != "" {
			return bad("photo extension")
		}
	}
	for _, key := range []string{"photo_camera_prefixes", "photo_ignored_directories"} {
		for _, s := range e.Lists[key] {
			if !RuleRelative(s) || strings.ContainsAny(s, `/\`) {
				return bad("photo names")
			}
		}
	}
	for _, r := range b.Rules {
		switch r.ID {
		case "chromium":
			for _, key := range []string{"signature_leaves", "extra_leaves", "strong_leaves"} {
				if len(r.Lists[key]) == 0 {
					return bad(key)
				}
				for _, s := range r.Lists[key] {
					if !RuleRelative(s) || strings.ContainsAny(s, `/\`) {
						return bad(key)
					}
				}
			}
			found := false
			for _, mode := range []string{"exact", "prefixes", "suffixes"} {
				a := r.Lists["profile_"+mode]
				valid := len(a) > 0
				for _, s := range a {
					valid = valid && RuleRelative(s) && !strings.ContainsAny(s, `/\`)
				}
				found = found || valid
			}
			if !found || r.Numbers["signature_min"] < 2 || r.Numbers["signature_min"] > 64 {
				return bad("chromium threshold/profile")
			}
		case "cache":
			if !member("updater_artifacts", r.Required) || !member("file", r.Required) {
				return bad("updater capability")
			}
			keys := strings.Fields("updater_directories_exact updater_directories_prefixes updater_files_exact updater_files_suffixes updater_display_suffixes updater_probe_exclude_prefixes")
			for k := range r.Lists {
				if strings.HasPrefix(k, "updater_") && !member(k, keys) {
					return bad("unknown updater policy")
				}
			}
			for _, key := range keys {
				a := r.Lists[key]
				if len(a) == 0 || len(a) > 128 {
					return bad(key)
				}
				for _, s := range a {
					if !ruleName(s) {
						return bad(key)
					}
				}
			}
			if n := r.Numbers["updater_stale_seconds"]; n < 86400 || n > 31536000 {
				return bad("updater age")
			}
		case "macos":
			for _, k := range []string{"old_ide_roots", "user_login_roots", "system_login_roots"} {
				if len(r.Catalogs[k]) == 0 || len(r.Catalogs[k]) > 32 {
					return bad(k)
				}
			}
			if n := r.Numbers["login_item_min_age_seconds"]; n < 86400 || n > 31536000 {
				return bad("login item age")
			}
			for _, k := range []string{"sensitive_cache_exact", "sensitive_cache_prefixes", "sensitive_group_contains"} {
				if len(r.Lists[k]) == 0 {
					return bad(k)
				}
				for _, s := range r.Lists[k] {
					if strings.TrimSpace(s) == "" || strings.ContainsAny(s, "/\\\x00\n\r") {
						return bad(k)
					}
				}
			}
		case "residual-macos":
			data, err := bundled.Files.ReadFile("residual-macos.toml")
			if err != nil {
				return err
			}
			var baseline RuleDefinition
			if err = toml.Unmarshal(data, &baseline); err != nil {
				return err
			}
			for _, k := range []string{"shared_vendor_prefixes", "protected_dot_dirs"} {
				for _, s := range baseline.Lists[k] {
					if !member(s, r.Lists[k]) {
						return bad("weakened residual fence")
					}
				}
			}
			for k, a := range r.Lists {
				for _, s := range a {
					valid := s != "" && !strings.ContainsAny(s, `/\`)
					switch k {
					case "dot_config_parents":
						valid = s == "" || RuleRelative(s)
					case "helper_id_suffixes", "id_file_suffixes":
						valid = strings.HasPrefix(s, ".") && !strings.ContainsAny(s, `/\`)
					}
					if !valid {
						return bad("residual matching")
					}
				}
			}
		}
	}
	return nil
}
