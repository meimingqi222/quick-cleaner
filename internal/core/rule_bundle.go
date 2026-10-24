package core

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"strings"
	"sync"

	bundled "github.com/meimingqi222/quick-cleaner/rules"
	"github.com/pelletier/go-toml/v2"
)

const RuleSchema uint32 = 13
const MaxRulePackage = 8 * 1024 * 1024

var ruleCapabilities = []string{"file", "providers", "source_install", "python_module", "registration", "chromium", "build_markers", "docker", "snapshot", "owner", "declutter", "variables", "source_flow", "directory_selection", "updater_artifacts", "version_retention"}

type RuleSnapshot struct{ Bundle RuleBundle }

func (s *RuleSnapshot) Definition(id string) *RuleDefinition {
	if s != nil {
		for i := range s.Bundle.Rules {
			if s.Bundle.Rules[i].ID == id {
				return &s.Bundle.Rules[i]
			}
		}
	}
	return nil
}
func (s *RuleSnapshot) List(id, key string) []string {
	r := s.Definition(id)
	if r == nil {
		return nil
	}
	return append([]string(nil), r.Lists[key]...)
}
func (s *RuleSnapshot) Number(id, key string, fallback ...uint64) uint64 {
	if r := s.Definition(id); r != nil {
		if n, ok := r.Numbers[key]; ok {
			return n
		}
	}
	if len(fallback) > 0 {
		return fallback[0]
	}
	return 0
}
func (s *RuleSnapshot) ProviderPolicy(id, key string) (ProviderPolicy, bool) {
	r := s.Definition(id)
	if r == nil {
		return ProviderPolicy{}, false
	}
	v, ok := r.ProviderPolicies[key]
	return v, ok
}
func (s *RuleSnapshot) CategoryPolicy(key string) (CategoryPolicy, bool) {
	r := s.Definition("engine")
	if r == nil {
		return CategoryPolicy{}, false
	}
	v, ok := r.Categories[key]
	return v, ok
}
func (s *RuleSnapshot) MatchesName(id, policy, name string) bool {
	for _, mode := range []string{"exact", "prefixes", "contains", "suffixes"} {
		for _, v := range s.List(id, policy+"_"+mode) {
			if mode == "exact" && name == v || mode == "prefixes" && strings.HasPrefix(name, v) || mode == "contains" && strings.Contains(name, v) || mode == "suffixes" && strings.HasSuffix(name, v) {
				return true
			}
		}
	}
	return false
}

var rulesOnce sync.Once
var activeRules *RuleSnapshot
var activeRulesError error

func CurrentRules() (*RuleSnapshot, error) {
	rulesOnce.Do(func() {
		b, e := LoadRuleBundle(bundled.Files, 1)
		if e != nil {
			activeRulesError = e
			return
		}
		activeRules = &RuleSnapshot{Bundle: b}
	})
	return activeRules, activeRulesError
}
func RuleList(id, key string) []string {
	s, e := CurrentRules()
	if e != nil {
		return nil
	}
	return s.List(id, key)
}
func RuleNumber(id, key string) uint64 {
	s, e := CurrentRules()
	if e != nil {
		return 0
	}
	return s.Number(id, key)
}
func LoadRuleDirectory(path string, sequence uint64) (RuleBundle, error) {
	return LoadRuleBundle(os.DirFS(path), sequence)
}
func LoadRuleBundle(source fs.FS, sequence uint64) (RuleBundle, error) {
	b := RuleBundle{Schema: RuleSchema, Sequence: sequence}
	names, e := fs.Glob(source, "*.toml")
	if e != nil {
		return b, e
	}
	if len(names) > 4096 {
		return b, errors.New("rule count limit")
	}
	total := 0
	for _, n := range names {
		data, e := fs.ReadFile(source, n)
		if e != nil {
			return b, e
		}
		total += len(data)
		if total > MaxRulePackage {
			return b, errors.New("rule package size limit")
		}
		var shape map[string]any
		if e = toml.Unmarshal(data, &shape); e != nil {
			return b, e
		}
		if e = validateRuleTaggedShape(shape); e != nil {
			return b, fmt.Errorf("%s: %w", n, e)
		}
		var r RuleDefinition
		d := toml.NewDecoder(bytes.NewReader(data)).DisallowUnknownFields()
		if e = d.Decode(&r); e != nil {
			return b, fmt.Errorf("%s: %w", n, e)
		}
		for i := range r.VersionLayouts {
			if r.VersionLayouts[i].Platform == "" {
				r.VersionLayouts[i].Platform = "all"
			}
		}
		b.Rules = append(b.Rules, r)
	}
	return b, b.Validate()
}
func ParseRuleBundle(data []byte) (RuleBundle, error) {
	var b RuleBundle
	if len(data) > MaxRulePackage {
		return b, errors.New("rule package size limit")
	}
	var raw struct {
		Rules []map[string]any `json:"rules"`
	}
	if e := json.Unmarshal(data, &raw); e != nil {
		return b, e
	}
	for _, r := range raw.Rules {
		if e := validateRuleTaggedShape(r); e != nil {
			return b, e
		}
	}
	d := json.NewDecoder(bytes.NewReader(data))
	d.DisallowUnknownFields()
	if e := d.Decode(&b); e != nil {
		return b, e
	}
	if e := d.Decode(new(any)); e != io.EOF {
		return b, errors.New("trailing rule package data")
	}
	return b, b.Validate()
}
func RuleRelative(s string) bool {
	if s == "" || len(s) > 1024 || strings.HasPrefix(s, "/") || strings.HasPrefix(s, `\`) || strings.ContainsAny(s, ":\x00*?\n\r") {
		return false
	}
	for _, p := range strings.FieldsFunc(s, func(r rune) bool { return r == '/' || r == '\\' }) {
		if p == "." || p == ".." || strings.HasSuffix(p, ".") || strings.HasSuffix(p, " ") {
			return false
		}
	}
	return !strings.Contains(strings.ReplaceAll(s, `\`, "/"), "//") && !strings.HasSuffix(s, "/") && !strings.HasSuffix(s, `\`)
}
func validRuleRoot(s string) bool {
	return member(s, []string{"home", "local", "roaming", "cache", "temp", "user_temp", "system"}) || len(s) == 7 && strings.HasPrefix(s, "drive:") && s[6] >= 'A' && s[6] <= 'Z'
}
func validDisposal(d Disposal) bool { return d == "permanent" || d == "recycle_bin" }
func (b RuleBundle) Validate() error {
	if b.Schema != RuleSchema || b.Sequence == 0 || len(b.Rules) == 0 || len(b.Rules) > 4096 {
		return errors.New("unsupported rule bundle")
	}
	ids := map[string]bool{}
	var engine *RuleDefinition
	for i := range b.Rules {
		r := &b.Rules[i]
		if r.ID == "" || strings.Trim(r.ID, "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-") != "" || ids[r.ID] || r.Version == 0 || !member(r.Platform, []string{"all", "windows", "macos"}) {
			return fmt.Errorf("invalid/duplicate rule %s", r.ID)
		}
		ids[r.ID] = true
		for _, c := range r.Required {
			if !member(c, ruleCapabilities) {
				return fmt.Errorf("unknown capability %s", c)
			}
		}
		if r.ID == "engine" {
			engine = r
		} else if len(r.Categories) > 0 {
			return errors.New("only engine declares categories")
		}
		if len(r.Facts) > 256 || len(r.Entries) > 4096 || len(r.Markers) > 256 || len(r.ProviderPolicies) > 128 {
			return errors.New("rule resource limit")
		}
		if _, e := ValidateRuleVariables(r.Variables); e != nil {
			return e
		}
		if len(r.Variables) > 0 && !member("variables", r.Required) {
			return errors.New("undeclared variable capability")
		}
		for _, p := range r.Facts {
			if e := p.Validate(); e != nil {
				return e
			}
			if p.Kind == "variable" {
				if _, ok := r.Variables[p.Name]; !ok {
					return errors.New("unknown variable fact")
				}
			} else if e := ValidateRuleTemplate(p.Path, r.Variables); e != nil {
				return e
			}
		}
		if r.Detect != nil {
			if e := r.Detect.Validate(r.Facts); e != nil {
				return e
			}
		}
		for _, p := range r.Preserve {
			if e := ValidateRuleTemplate(p, r.Variables); e != nil {
				return e
			}
		}
		for _, list := range r.Lists {
			if len(list) > 4096 {
				return errors.New("list resource limit")
			}
			for _, v := range list {
				if len(v) > 1024 || strings.ContainsAny(v, "\x00\r\n") {
					return errors.New("invalid list value")
				}
			}
		}
		for key, p := range r.ProviderPolicies {
			if !ruleIdentifier(key) || len(key) > 128 || !member("providers", r.Required) || !validDisposal(p.Disposal) {
				return errors.New("invalid provider policy")
			}
		}
		for _, p := range r.Entries {
			temp := p.Root == "user_temp" && p.Path == "." && p.Operation == "contents" && len(r.Variables) == 0
			if !temp {
				if e := ValidateRuleTemplate(p.Path, r.Variables); e != nil {
					return e
				}
			}
			capability := "file"
			if p.Operation == "go" || p.Operation == "pnpm" {
				capability = "owner"
			}
			if !validRuleRoot(p.Root) || !member(string(p.Operation), []string{"file", "tree", "contents", "go", "pnpm", "trash"}) || !member(capability, r.Required) || !validDisposal(p.Disposal) || p.Disposal == "recycle_bin" && p.Operation != "file" && p.Operation != "tree" {
				return errors.New("invalid path operation/root/disposal")
			}
		}
		if len(r.Catalogs) > 64 || len(r.Locations) > 64 {
			return errors.New("catalog/location resource limit")
		}
		for _, rows := range r.Catalogs {
			if len(rows) > 1024 {
				return errors.New("catalog limit")
			}
			for _, row := range rows {
				if !RuleRelative(row.Path) || row.Zh == "" || row.En == "" || len(row.Children) > 256 {
					return errors.New("invalid catalog")
				}
				for _, child := range row.Children {
					if !RuleRelative(child.Path) {
						return errors.New("invalid catalog child")
					}
				}
			}
		}
		for _, rows := range r.Locations {
			if len(rows) > 128 {
				return errors.New("location limit")
			}
			for _, row := range rows {
				if !RuleRelative(row.Path) || !member(string(row.Source), strings.Fields("UninstallEntry InstallDir EmptyInstallParent VendorRegKey AppDataDir LikelyAppDataDir AppSupportDir StartMenuDir Shortcut ConfigRegKey LikelyConfigRegKey AppPathsEntry StartupEntry UserEnvEntry Service LikelyService FirewallRule RasTrace LeakDiagnostics CompatSetting InstallerFolderEntry ProgramNameCache DefaultProgramsEntry ComClass ShellExtension ScheduledTask PrefetchFile CrashDump UninstallerLeftover CacheDir LogDir PreferenceFile ContainerDir ApplicationScript RecentDocumentList PackageReceipt LaunchAgent LaunchDaemon DotConfigDir SystemExtension Other")) {
					return errors.New("invalid location")
				}
			}
		}
		for _, m := range r.Markers {
			if !RuleRelative(m.Dir) || strings.ContainsAny(m.Dir, `/\`) || m.Zh == "" || m.En == "" {
				return errors.New("invalid build marker")
			}
			for _, p := range m.SiblingAny {
				if !RuleRelative(p) {
					return errors.New("invalid build sibling")
				}
			}
		}
		if e := validateRuleDeclarations(r); e != nil {
			return fmt.Errorf("%s: %w", r.ID, e)
		}
	}
	for _, id := range []string{"engine", "hermes", "chromium", "build", "development", "cache", "package-macos", "browsers", "macos", "windows", "residual-macos"} {
		if !ids[id] {
			return fmt.Errorf("missing required rule %s", id)
		}
	}
	if engine == nil {
		return errors.New("missing engine")
	}
	for _, r := range b.Rules {
		for _, p := range r.Entries {
			if _, ok := engine.Categories[p.Category]; !ok {
				return errors.New("unknown path category")
			}
		}
	}
	return validateRulePolicies(b, engine)
}
