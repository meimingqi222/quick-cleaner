package core

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"unicode/utf8"
)

type Variable struct {
	Kind    string `toml:"kind" json:"kind"`
	Path    string `toml:"path" json:"path"`
	Length  int    `toml:"length" json:"length"`
	Pointer string `toml:"pointer" json:"pointer"`
	Limit   int    `toml:"limit" json:"limit"`
}
type Resolution struct {
	Evidence Evidence
	Value    any
}
type VariableValues map[string]Resolution

func ruleIdentifier(s string) bool {
	if s == "" {
		return false
	}
	for _, c := range []byte(s) {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '_') {
			return false
		}
	}
	return true
}
func ruleVariableNames(s string) ([]string, error) {
	var out []string
	for {
		a := strings.Index(s, "${")
		if a < 0 {
			return out, nil
		}
		s = s[a+2:]
		b := strings.IndexByte(s, '}')
		if b < 0 || !ruleIdentifier(s[:b]) {
			return nil, errors.New("invalid variable reference")
		}
		out = append(out, s[:b])
		s = s[b+1:]
	}
}
func ValidateRuleTemplate(s string, v map[string]Variable) error {
	if !RuleRelative(s) {
		return errors.New("invalid relative template")
	}
	ns, e := ruleVariableNames(s)
	if e != nil {
		return e
	}
	for _, n := range ns {
		if _, ok := v[n]; !ok {
			return errors.New("unknown variable reference")
		}
	}
	return nil
}
func ValidateRuleVariables(v map[string]Variable) ([]string, error) {
	if len(v) > 256 {
		return nil, errors.New("variable resource limit")
	}
	for n, x := range v {
		if !ruleIdentifier(n) {
			return nil, errors.New("invalid variable name")
		}
		if e := ValidateRuleTemplate(x.Path, v); e != nil {
			return nil, e
		}
		switch x.Kind {
		case "path":
		case "canonical_hash":
			if x.Length < 8 || x.Length > 64 {
				return nil, errors.New("invalid digest length")
			}
		case "directories":
			if x.Limit < 1 || x.Limit > 256 {
				return nil, errors.New("directory enumeration limit")
			}
		case "json_field", "json_set":
			if !validPointer(x.Pointer) {
				return nil, errors.New("invalid JSON pointer")
			}
		default:
			return nil, errors.New("unknown variable kind")
		}
	}
	state := map[string]int{}
	var order []string
	var visit func(string, int) error
	visit = func(n string, d int) error {
		if state[n] == 2 {
			return nil
		}
		if d >= 32 || state[n] == 1 {
			return errors.New("variable cycle or depth limit")
		}
		state[n] = 1
		ns, _ := ruleVariableNames(v[n].Path)
		for _, dep := range ns {
			if e := visit(dep, d+1); e != nil {
				return e
			}
		}
		state[n] = 2
		order = append(order, n)
		return nil
	}
	keys := make([]string, 0, len(v))
	for n := range v {
		keys = append(keys, n)
	}
	sort.Strings(keys)
	for _, n := range keys {
		if e := visit(n, 0); e != nil {
			return nil, e
		}
	}
	return order, nil
}
func ExpandRulePaths(template string, values VariableValues) ([]string, error) {
	names, e := ruleVariableNames(template)
	if e != nil {
		return nil, e
	}
	out := []string{template}
	for _, n := range names {
		v, ok := values[n]
		if !ok || v.Evidence != Confirmed {
			return nil, errors.New("unconfirmed variable")
		}
		var replacements []string
		switch x := v.Value.(type) {
		case string:
			replacements = []string{x}
		case []string:
			replacements = x
		case []any:
			for _, y := range x {
				s, ok := y.(string)
				if !ok {
					return nil, errors.New("non-string path member")
				}
				replacements = append(replacements, s)
			}
		default:
			return nil, errors.New("invalid path variable type")
		}
		expanded := map[string]bool{}
		for _, p := range out {
			for _, r := range replacements {
				if !RuleRelative(r) || strings.Contains(r, "${") {
					return nil, errors.New("variable path escape")
				}
				s := strings.ReplaceAll(p, "${"+n+"}", r)
				if !RuleRelative(s) || len(expanded) >= 256 {
					return nil, errors.New("path expansion limit or escape")
				}
				expanded[s] = true
			}
		}
		out = nil
		for s := range expanded {
			out = append(out, s)
		}
		sort.Strings(out)
	}
	for _, p := range out {
		if !RuleRelative(p) {
			return nil, errors.New("invalid resolved path")
		}
	}
	return out, nil
}
func EvaluateRuleVariables(vars map[string]Variable, root string) VariableValues {
	values := VariableValues{}
	order, e := ValidateRuleVariables(vars)
	if e != nil {
		return values
	}
	documents := map[string]any{}
	documentErrors := map[string]error{}
	remaining := 8 * 1024 * 1024
	for _, n := range order {
		x := vars[n]
		value, present, err := func() (any, bool, error) {
			paths, e := ExpandRulePaths(x.Path, values)
			if e != nil {
				return nil, false, e
			}
			if len(paths) != 1 {
				return nil, false, errors.New("extraction requires one path")
			}
			relative := paths[0]
			target := filepath.Join(root, filepath.FromSlash(strings.ReplaceAll(relative, `\`, "/")))
			ok, e := ConfinedRulePath(root, target)
			if e != nil || !ok {
				return nil, false, e
			}
			switch x.Kind {
			case "path":
				return relative, true, nil
			case "canonical_hash":
				p, e := filepath.EvalSymlinks(target)
				if e != nil {
					return nil, false, e
				}
				p, e = filepath.Abs(p)
				if e != nil {
					return nil, false, e
				}
				h := sha256.Sum256([]byte(Norm(p)))
				return hex.EncodeToString(h[:])[:x.Length], true, nil
			case "json_field", "json_set":
				ok, e := ConfinedRuleFile(root, target)
				if e != nil {
					return nil, false, e
				}
				if !ok {
					return nil, false, errors.New("JSON input is not a file")
				}
				doc, loaded := documents[target]
				if !loaded {
					if e := documentErrors[target]; e != nil {
						return nil, false, e
					}
					md, e := os.Stat(target)
					if e != nil {
						return nil, false, e
					}
					if md.Size() > int64(remaining) {
						return nil, false, errors.New("extraction byte budget")
					}
					b, e := readRuleEvidence(target)
					if e == nil && !utf8.Valid(b) {
						e = errors.New("invalid JSON UTF-8")
					}
					if e == nil && len(b) > remaining {
						e = errors.New("extraction byte budget")
					}
					if e == nil {
						remaining -= len(b)
						e = json.Unmarshal(b, &doc)
					}
					if e != nil {
						documentErrors[target] = e
						return nil, false, e
					}
					documents[target] = doc
				}
				v, ok := jsonPointer(doc, x.Pointer)
				if !ok {
					return nil, false, nil
				}
				if x.Kind == "json_field" {
					s, ok := v.(string)
					if !ok || len(s) > 1024 {
						return nil, false, errors.New("JSON field type or limit")
					}
					return s, true, nil
				}
				a, ok := v.([]any)
				if !ok || len(a) > 256 {
					return nil, false, errors.New("JSON set type or limit")
				}
				members := map[string]bool{}
				for _, v := range a {
					s, ok := v.(string)
					if !ok || len(s) > 1024 {
						return nil, false, errors.New("JSON set member type or limit")
					}
					members[s] = true
				}
				out := []string{}
				for s := range members {
					out = append(out, s)
				}
				sort.Strings(out)
				return out, true, nil
			case "directories":
				f, e := os.Open(target)
				if e != nil {
					return nil, false, e
				}
				defer f.Close()
				entries, e := f.ReadDir(x.Limit + 1)
				if e != nil && e != io.EOF {
					return nil, false, e
				}
				if len(entries) > x.Limit {
					return nil, false, errors.New("directory enumeration exhausted")
				}
				out := []string{}
				for _, entry := range entries {
					ok, e := ConfinedRulePath(root, filepath.Join(target, entry.Name()))
					if e != nil {
						return nil, false, e
					}
					if ok && entry.IsDir() {
						s := relative + "/" + entry.Name()
						if !utf8.ValidString(s) || !RuleRelative(s) {
							return nil, false, errors.New("invalid directory member")
						}
						out = append(out, s)
					}
				}
				sort.Strings(out)
				return out, true, nil
			}
			return nil, false, errors.New("unknown variable kind")
		}()
		r := Resolution{Evidence: Absent}
		if err != nil {
			r.Evidence = Unknown
		} else if present {
			r.Evidence = Confirmed
			r.Value = value
		}
		values[n] = r
	}
	return values
}
