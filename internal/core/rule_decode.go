package core

import (
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"strings"
)

var probeVariantFields = map[string]string{"variable": "name equals", "file": "path", "text": "path signature", "json": "path pointer equals"}
var variableVariantFields = map[string]string{"path": "path", "canonical_hash": "path length", "json_field": "path pointer", "json_set": "path pointer", "directories": "path limit"}
var conditionVariantFields = map[string]string{"fact": "name", "not": "condition", "all": "conditions", "any": "conditions"}
var selectionVariantFields = map[string]string{"children": "directories_only prefixes", "container_directories": "paths exclude_name_policy", "named_files": "name child_depth", "named_directories": "name child_depth", "files": "prefixes suffixes", "path": "directories_only", "manifest_children": "manifest selected_value", "orphaned_children": "manifest pointer uri_prefix scope", "catalog_children": "catalog_policy leaf_policy", "layout_children": "catalog", "signature_leaves": "signature", "linked_worktrees": "containers"}

func marshalRuleVariant(value any, kind string, variants map[string]string) ([]byte, error) {
	fields, ok := variants[kind]
	if !ok {
		return nil, fmt.Errorf("unknown declaration kind %q", kind)
	}
	data, err := json.Marshal(value)
	if err != nil {
		return nil, err
	}
	var raw map[string]json.RawMessage
	if err = json.Unmarshal(data, &raw); err != nil {
		return nil, err
	}
	allowed := append([]string{"kind"}, strings.Fields(fields)...)
	for key := range raw {
		if !member(key, allowed) {
			delete(raw, key)
		}
	}
	return json.Marshal(raw)
}
func (p Probe) MarshalJSON() ([]byte, error) {
	type plain Probe
	return marshalRuleVariant(plain(p), p.Kind, probeVariantFields)
}
func (v Variable) MarshalJSON() ([]byte, error) {
	type plain Variable
	return marshalRuleVariant(plain(v), v.Kind, variableVariantFields)
}
func (c Condition) MarshalJSON() ([]byte, error) {
	type plain Condition
	return marshalRuleVariant(plain(c), c.Kind, conditionVariantFields)
}
func (s Selection) MarshalJSON() ([]byte, error) {
	type plain Selection
	return marshalRuleVariant(plain(s), s.Kind, selectionVariantFields)
}

// Go structs represent the tagged unions; checking the raw shape also rejects
// fields belonging to a different variant, including explicitly false values.
func validateRuleTaggedShape(raw map[string]any) error {
	check := func(value any, variants map[string]string) error {
		m, ok := value.(map[string]any)
		if !ok {
			return errors.New("invalid tagged declaration")
		}
		kind, ok := m["kind"].(string)
		if !ok {
			return errors.New("missing declaration kind")
		}
		fields, ok := variants[kind]
		if !ok {
			return fmt.Errorf("unknown declaration kind %q", kind)
		}
		allowed := append([]string{"kind"}, strings.Fields(fields)...)
		for k := range m {
			if !member(k, allowed) {
				return fmt.Errorf("field %q is not valid for %q", k, kind)
			}
		}
		for _, k := range allowed {
			if _, ok := m[k]; !ok && k != "leaf_policy" {
				return fmt.Errorf("missing %s field %s", kind, k)
			}
		}
		return nil
	}
	for key, variants := range map[string]map[string]string{"facts": probeVariantFields, "variables": variableVariantFields} {
		if value, ok := raw[key]; ok && value != nil {
			m, ok := value.(map[string]any)
			if !ok {
				return errors.New("invalid named declarations")
			}
			for _, v := range m {
				if e := check(v, variants); e != nil {
					return e
				}
			}
		}
	}
	var condition func(any, int) error
	condition = func(v any, depth int) error {
		if depth > 32 {
			return errors.New("condition depth limit")
		}
		if e := check(v, conditionVariantFields); e != nil {
			return e
		}
		m := v.(map[string]any)
		if m["kind"] == "not" {
			return condition(m["condition"], depth+1)
		}
		if m["kind"] == "all" || m["kind"] == "any" {
			a := reflect.ValueOf(m["conditions"])
			if a.Kind() != reflect.Slice || a.Len() == 0 || a.Len() > 256 {
				return errors.New("condition resource limit")
			}
			for i := 0; i < a.Len(); i++ {
				if e := condition(a.Index(i).Interface(), depth+1); e != nil {
					return e
				}
			}
		}
		return nil
	}
	if v, ok := raw["detect"]; ok && v != nil {
		if e := condition(v, 0); e != nil {
			return e
		}
	}
	if v, ok := raw["directories"]; ok && v != nil {
		a := reflect.ValueOf(v)
		if a.Kind() != reflect.Slice {
			return errors.New("invalid directories")
		}
		for i := 0; i < a.Len(); i++ {
			m, ok := a.Index(i).Interface().(map[string]any)
			if !ok {
				return errors.New("invalid directory declaration")
			}
			if e := check(m["select"], selectionVariantFields); e != nil {
				return e
			}
		}
	}
	return nil
}
