package core

import (
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"strconv"
	"strings"
	"unicode/utf8"
)

type Evidence string

const (
	Confirmed Evidence = "Confirmed"
	Absent    Evidence = "Absent"
	Unknown   Evidence = "Unknown"
)

type Condition struct {
	Kind       string      `toml:"kind" json:"kind"`
	Name       string      `toml:"name" json:"name"`
	Conditions []Condition `toml:"conditions" json:"conditions"`
	Condition  *Condition  `toml:"condition" json:"condition"`
}

func (c Condition) Validate(facts map[string]Probe) error { return c.validateAt(facts, 0) }
func (c Condition) validateAt(f map[string]Probe, d int) error {
	if d > 32 {
		return errors.New("condition nesting limit")
	}
	switch c.Kind {
	case "fact":
		if _, ok := f[c.Name]; !ok {
			return errors.New("unknown fact")
		}
	case "not":
		if c.Condition == nil {
			return errors.New("missing condition")
		}
		return c.Condition.validateAt(f, d+1)
	case "all", "any":
		if len(c.Conditions) == 0 || len(c.Conditions) > 256 {
			return errors.New("condition resource limit")
		}
		for _, v := range c.Conditions {
			if err := v.validateAt(f, d+1); err != nil {
				return err
			}
		}
	default:
		return errors.New("unknown condition kind")
	}
	return nil
}
func (c Condition) Evaluate(f map[string]Evidence) Evidence { return c.evaluateAt(f, 0) }
func (c Condition) evaluateAt(f map[string]Evidence, d int) Evidence {
	if d > 32 {
		return Unknown
	}
	switch c.Kind {
	case "fact":
		if v, ok := f[c.Name]; ok && (v == Confirmed || v == Absent) {
			return v
		}
		return Unknown
	case "not":
		if c.Condition == nil {
			return Unknown
		}
		switch c.Condition.evaluateAt(f, d+1) {
		case Confirmed:
			return Absent
		case Absent:
			return Confirmed
		}
		return Unknown
	case "all", "any":
		if len(c.Conditions) == 0 || len(c.Conditions) > 256 {
			return Unknown
		}
		unknown := false
		for _, v := range c.Conditions {
			e := v.evaluateAt(f, d+1)
			if c.Kind == "all" && e == Absent {
				return Absent
			}
			if c.Kind == "any" && e == Confirmed {
				return Confirmed
			}
			unknown = unknown || e == Unknown
		}
		if unknown {
			return Unknown
		}
		if c.Kind == "all" {
			return Confirmed
		}
		return Absent
	}
	return Unknown
}

type Probe struct {
	Kind      string `toml:"kind" json:"kind"`
	Name      string `toml:"name" json:"name"`
	Path      string `toml:"path" json:"path"`
	Signature string `toml:"signature" json:"signature"`
	Pointer   string `toml:"pointer" json:"pointer"`
	Equals    any    `toml:"equals" json:"equals"`
}

func (p Probe) Validate() error {
	if p.Kind == "variable" {
		b, e := json.Marshal(p.Equals)
		if e != nil || len(b) > 65536 || !ruleIdentifier(p.Name) {
			return errors.New("invalid variable fact")
		}
		return nil
	}
	if !RuleRelative(p.Path) {
		return errors.New("probe escapes root")
	}
	switch p.Kind {
	case "file":
	case "text":
		if len(p.Signature) == 0 || len(p.Signature) > 65536 {
			return errors.New("invalid signature")
		}
	case "json":
		if !validPointer(p.Pointer) {
			return errors.New("invalid JSON pointer")
		}
	default:
		return errors.New("unknown probe kind")
	}
	return nil
}
func (p Probe) Evaluate(root string) Evidence { return p.EvaluateWith(root, nil) }
func (p Probe) EvaluateWith(root string, values VariableValues) Evidence {
	if p.Validate() != nil {
		return Unknown
	}
	if p.Kind == "variable" {
		v, ok := values[p.Name]
		if !ok {
			return Unknown
		}
		if v.Evidence != Confirmed {
			return v.Evidence
		}
		if jsonEqual(v.Value, p.Equals) {
			return Confirmed
		}
		return Absent
	}
	paths, err := ExpandRulePaths(p.Path, values)
	if err != nil || len(paths) != 1 {
		return Unknown
	}
	target := filepath.Join(root, filepath.FromSlash(strings.ReplaceAll(paths[0], `\`, "/")))
	ok, err := ConfinedRuleFile(root, target)
	if err != nil {
		return Unknown
	}
	if !ok {
		return Absent
	}
	if p.Kind == "file" {
		return Confirmed
	}
	b, err := readRuleEvidence(target)
	if err != nil {
		return Unknown
	}
	matched := false
	if p.Kind == "text" {
		if !utf8.Valid(b) {
			return Unknown
		}
		matched = strings.Contains(string(b), p.Signature)
	} else {
		var v any
		if !utf8.Valid(b) || json.Unmarshal(b, &v) != nil {
			return Unknown
		}
		v, ok = jsonPointer(v, p.Pointer)
		matched = ok && jsonEqual(v, p.Equals)
	}
	if matched {
		return Confirmed
	}
	return Absent
}
func jsonEqual(a, b any) bool {
	x, e := json.Marshal(a)
	y, f := json.Marshal(b)
	return e == nil && f == nil && reflect.DeepEqual(x, y)
}
func validPointer(p string) bool { return strings.HasPrefix(p, "/") && len(p) <= 1024 }
func jsonPointer(v any, p string) (any, bool) {
	if p == "" {
		return v, true
	}
	if !strings.HasPrefix(p, "/") {
		return nil, false
	}
	for _, s := range strings.Split(p[1:], "/") {
		s = strings.ReplaceAll(strings.ReplaceAll(s, "~1", "/"), "~0", "~")
		switch x := v.(type) {
		case map[string]any:
			var ok bool
			v, ok = x[s]
			if !ok {
				return nil, false
			}
		case []any:
			i, e := strconv.Atoi(s)
			if e != nil || i < 0 || i >= len(x) || (len(s) > 1 && s[0] == '0') {
				return nil, false
			}
			v = x[i]
		default:
			return nil, false
		}
	}
	return v, true
}

// ruleLstat is the default ancestor probe; tests count calls through it.
var ruleLstat = os.Lstat

func ConfinedRuleFile(root, target string) (bool, error) {
	return confinedRulePath(root, target, true, nil, nil)
}
func ConfinedRulePath(root, target string) (bool, error) {
	return confinedRulePath(root, target, false, nil, nil)
}
func confinedRulePath(root, target string, fileOnly bool, lstat func(string) (os.FileInfo, error), eval func(string) (string, error)) (bool, error) {
	if lstat == nil {
		lstat = ruleLstat
	}
	if eval == nil {
		eval = filepath.EvalSymlinks
	}
	if strings.HasPrefix(target, root+string(filepath.Separator)) {
		for _, part := range strings.Split(strings.TrimPrefix(target, root+string(filepath.Separator)), string(filepath.Separator)) {
			if part == "." || part == ".." {
				return false, errors.New("evidence escapes root")
			}
		}
	}
	rel, err := filepath.Rel(root, target)
	if err != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) || filepath.IsAbs(rel) {
		return false, errors.New("evidence escapes root")
	}
	md, err := lstat(root)
	if os.IsNotExist(err) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	if ruleEvidenceLink(md) {
		return false, errors.New("redirected evidence root")
	}
	real, err := eval(root)
	if err != nil {
		return false, err
	}
	if rel == "." {
		return md.Mode().IsRegular() || (!fileOnly && md.IsDir()), nil
	}
	parts := strings.Split(rel, string(filepath.Separator))
	for i, p := range parts {
		real = filepath.Join(real, p)
		md, err = lstat(real)
		if os.IsNotExist(err) {
			return false, nil
		}
		if err != nil {
			return false, err
		}
		if ruleEvidenceLink(md) {
			return false, errors.New("redirected evidence")
		}
		if i == len(parts)-1 {
			return md.Mode().IsRegular() || (!fileOnly && md.IsDir()), nil
		}
		if !md.IsDir() {
			return false, errors.New("invalid evidence ancestor")
		}
	}
	return false, nil
}
func ruleEvidenceLink(md os.FileInfo) bool {
	if md.Mode()&os.ModeSymlink != 0 {
		return true
	}
	// Windows junctions and non-symlink reparse points are redirects too.
	if runtime.GOOS == "windows" {
		v := reflect.ValueOf(md.Sys())
		if v.Kind() == reflect.Pointer && !v.IsNil() {
			v = v.Elem()
		}
		if v.Kind() == reflect.Struct {
			a := v.FieldByName("FileAttributes")
			if a.IsValid() && a.CanUint() {
				return a.Uint()&0x400 != 0
			}
		}
	}
	return false
}
func readRuleEvidence(path string) ([]byte, error) {
	f, e := os.Open(path)
	if e != nil {
		return nil, e
	}
	defer f.Close()
	b, e := io.ReadAll(io.LimitReader(f, 4*1024*1024+1))
	if len(b) > 4*1024*1024 {
		return nil, errors.New("evidence byte limit")
	}
	return b, e
}
func (r *RuleDefinition) EvidenceAt(root string) (VariableValues, map[string]Evidence) {
	v := EvaluateRuleVariables(r.Variables, root)
	f := map[string]Evidence{}
	for n, p := range r.Facts {
		f[n] = p.EvaluateWith(root, v)
	}
	return v, f
}
func (r *RuleDefinition) MatchesAt(root string) Evidence {
	if r == nil {
		return Unknown
	}
	if r.Detect == nil {
		return Confirmed
	}
	_, f := r.EvidenceAt(root)
	return r.Detect.Evaluate(f)
}
