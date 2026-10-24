package core

import (
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"sort"
)

type ruleChild struct {
	path, name string
	directory  bool
}
type directoryProbe struct {
	exists bool
	err    error
}
type ruleEnumeration struct {
	probes                      map[string]directoryProbe
	inventories                 map[string][]ruleChild
	manifests                   map[string]any
	manifestReads, manifestRows int
	entries, candidates         int
}

func (e *ruleEnumeration) manifest(anchor, path string) any {
	if e.manifests == nil {
		e.manifests = map[string]any{}
	}
	key := anchor + "\x00" + path
	if value, exists := e.manifests[key]; exists {
		return value
	}
	e.manifests[key] = nil
	if e.manifestReads >= 512 {
		return nil
	}
	e.manifestReads++
	if ok, err := ConfinedRuleFile(anchor, path); !ok || err != nil {
		return nil
	}
	data, err := readRuleEvidence(path)
	if err != nil {
		return nil
	}
	var value any
	if json.Unmarshal(data, &value) != nil {
		return nil
	}
	e.manifests[key] = value
	return value
}

func (e *ruleEnumeration) directory(anchor, path string) (bool, error) {
	if e.probes == nil {
		e.probes = map[string]directoryProbe{}
	}
	key := anchor + "\x00" + path
	if p, ok := e.probes[key]; ok {
		return p.exists, p.err
	}
	if len(e.probes) >= 512 {
		return false, errors.New("Directory probe budget exhausted")
	}
	ok, err := ConfinedRulePath(anchor, path)
	if ok && err == nil {
		md, statErr := os.Lstat(path)
		err = statErr
		if err == nil && (ruleEvidenceLink(md) || !md.IsDir()) {
			err = errors.New("Not a confined directory")
		}
	}
	e.probes[key] = directoryProbe{exists: ok && err == nil, err: err}
	return ok && err == nil, err
}

func (e *ruleEnumeration) children(anchor, path string) ([]ruleChild, bool) {
	if e.inventories == nil {
		e.inventories = map[string][]ruleChild{}
	}
	key := anchor + "\x00" + path
	if children, ok := e.inventories[key]; ok {
		return children, children != nil
	}
	ok, err := e.directory(anchor, path)
	if !ok || err != nil {
		return nil, false
	}
	// nil caches a failed/over-budget inventory, never a partial deletion grant.
	e.inventories[key] = nil
	if e.entries >= 16384 {
		return nil, false
	}
	file, err := os.Open(path)
	if err != nil {
		return nil, false
	}
	defer file.Close()
	children := []ruleChild{}
	count := 0
	for {
		batch, err := file.ReadDir(128)
		if err != nil && err != io.EOF {
			return nil, false
		}
		for _, entry := range batch {
			count++
			e.entries++
			if count > 4096 || e.entries > 16384 {
				return nil, false
			}
			child := filepath.Join(path, entry.Name())
			md, err := os.Lstat(child)
			if err != nil {
				return nil, false
			}
			if ruleEvidenceLink(md) || (!md.IsDir() && !md.Mode().IsRegular()) {
				continue
			}
			children = append(children, ruleChild{child, entry.Name(), md.IsDir()})
		}
		if err == io.EOF {
			break
		}
	}
	sort.Slice(children, func(i, j int) bool { return children[i].name < children[j].name })
	e.inventories[key] = children
	return children, true
}

func (e *ruleEnumeration) emit(out *[]ScanTarget, target ScanTarget, kind string) {
	if e.candidates >= 16384 {
		return
	}
	e.candidates++
	protected := IsProtectedResidualPath(target.Path)
	if target.Operation.Kind == OpContents {
		protected = IsContentsProtected(target.Path)
	}
	if protected || target.Rule.scope == "" {
		return
	}
	ok, err := ConfinedRulePath(target.Rule.scope, target.Path)
	if !ok || err != nil {
		return
	}
	md, err := os.Lstat(target.Path)
	if err != nil || ruleEvidenceLink(md) {
		return
	}
	if kind == "directory" && !md.IsDir() || kind == "file" && !md.Mode().IsRegular() || !md.IsDir() && !md.Mode().IsRegular() {
		return
	}
	*out = append(*out, target)
}
