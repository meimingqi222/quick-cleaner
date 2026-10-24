package core

import (
	"os"
	"path/filepath"
)

type CategoryID string

const (
	SystemTemp         CategoryID = "SystemTemp"
	UserTemp           CategoryID = "UserTemp"
	UserCache          CategoryID = "UserCache"
	BrowserCache       CategoryID = "BrowserCache"
	PackageCache       CategoryID = "PackageCache"
	UpdaterPackages    CategoryID = "UpdaterPackages"
	Logs               CategoryID = "Logs"
	RecycleBinCategory CategoryID = "RecycleBin"
	Thumbnails         CategoryID = "Thumbnails"
	BrokenLoginItems   CategoryID = "BrokenLoginItems"
	AiAgents           CategoryID = "AiAgents"
	DevBuild           CategoryID = "DevBuild"
	DevWorktrees       CategoryID = "DevWorktrees"
	LocalSnapshots     CategoryID = "LocalSnapshots"
	IosBackup          CategoryID = "IosBackup"
	OldIdeData         CategoryID = "OldIdeData"
	DockerImages       CategoryID = "DockerImages"
)

func AllCategories() []CategoryID {
	return []CategoryID{SystemTemp, UserTemp, UserCache, BrowserCache, PackageCache, UpdaterPackages, Logs, RecycleBinCategory, Thumbnails, BrokenLoginItems, AiAgents, DevBuild, DevWorktrees, LocalSnapshots, IosBackup, OldIdeData, DockerImages}
}
func (c CategoryID) Policy() CategoryPolicy {
	if s, err := CurrentRules(); err == nil {
		if p, ok := s.CategoryPolicy(string(c)); ok {
			return p
		}
	}
	return CategoryPolicy{Safety: "danger", Group: "developer"}
}
func (c CategoryID) DefaultSelected() bool { return c.Policy().Safety == "safe" }
func (c CategoryID) IsDeveloper() bool     { return c.Policy().Group == "developer" }
func (c CategoryID) IsDiscovered() bool    { return c == DevBuild }

type ScanTarget struct {
	Operation   Operation
	Disposal    Disposal
	Rule        RuleRef
	Path        string
	Label       Text
	Category    CategoryID
	Recommended bool
	SizeHint    *uint64
}

func ProviderTarget(snapshot *RuleSnapshot, ruleID, key, path string, label Text, category CategoryID, operation Operation, recommended bool) ScanTarget {
	r := ProviderRuleRef(snapshot, ruleID, key)
	p, ok := snapshot.ProviderPolicy(ruleID, key)
	disposal := Permanent
	if ok {
		disposal = p.Disposal
	}
	if disposal == RecycleBin && operation.Kind != OpFile && operation.Kind != OpTree {
		r.blocked = "Provider disposal conflicts with its typed operation"
	}
	return ScanTarget{Operation: operation, Disposal: disposal, Rule: r, Path: path, Label: label, Category: category, Recommended: recommended && ok && p.Recommended}
}

// NormalizeScanTargets merges all contributors before removing overlaps, so a
// covered child cannot lose its preserve constraints or change disposal policy.
func NormalizeScanTargets(targets []ScanTarget) []ScanTarget {
	positions := map[string]int{}
	var merged []ScanTarget
	for _, target := range targets {
		key := Norm(target.Path)
		if i, ok := positions[key]; ok {
			first := &merged[i]
			if first.Operation != target.Operation || first.Disposal != target.Disposal {
				first.Rule.blocked = "Conflicting cleanup policies for the same target"
				first.Recommended = false
			}
			first.Rule = first.Rule.Merge(target.Rule)
			first.Recommended = first.Recommended && target.Recommended
		} else {
			positions[key] = len(merged)
			merged = append(merged, target)
		}
	}
	covered := make([]bool, len(merged))
	for i := range merged {
		parent := -1
		path := Norm(merged[i].Path)
		for j, candidate := range merged {
			key := Norm(candidate.Path)
			if i != j && key != path && candidate.Operation.CoversSubtree() && atOrUnder(path, key) && (parent < 0 || len(key) < len(Norm(merged[parent].Path))) {
				parent = j
			}
		}
		if parent < 0 {
			continue
		}
		covered[i] = true
		child := merged[i]
		p := &merged[parent]
		if p.Disposal != child.Disposal {
			p.Rule.blocked = "Conflicting cleanup policies for the same subtree"
			p.Recommended = false
		}
		p.Rule = p.Rule.Merge(child.Rule)
		p.Recommended = p.Recommended && child.Recommended
	}
	var out []ScanTarget
	for i, target := range merged {
		if covered[i] {
			continue
		}
		preserve := target.Rule.Preserved()
		needsSplit := false
		if target.Operation.Kind == OpTree || target.Operation.Kind == OpContents {
			for _, keep := range preserve {
				if Norm(keep) != Norm(target.Path) && atOrUnder(Norm(keep), Norm(target.Path)) {
					needsSplit = true
				}
			}
		}
		if needsSplit {
			budget := 4096
			if children, ok := splitPreservedTarget(target, preserve, 8, &budget); ok {
				out = append(out, children...)
				continue
			}
		}
		out = append(out, target)
	}
	return out
}

func splitPreservedTarget(parent ScanTarget, preserve []string, depth int, budget *int) ([]ScanTarget, bool) {
	entries, err := os.ReadDir(parent.Path)
	if err != nil {
		return nil, false
	}
	var out []ScanTarget
	for _, entry := range entries {
		if *budget == 0 {
			return nil, false
		}
		child := parent
		child.Path = filepath.Join(parent.Path, entry.Name())
		child.SizeHint = nil
		child.Label = NewText(parent.Label.Get(Chinese)+" · "+entry.Name(), parent.Label.Get(English)+" · "+entry.Name())
		if child.Operation.Kind == OpContents {
			child.Operation = Operation{Kind: OpTree}
		}
		if md, err := os.Lstat(child.Path); err == nil && md.Mode().IsRegular() {
			child.Operation = Operation{Kind: OpFile}
		}
		keep, nested := false, false
		for _, p := range preserve {
			if Norm(p) == Norm(child.Path) {
				keep = true
			}
			if atOrUnder(Norm(p), Norm(child.Path)) {
				nested = true
			}
		}
		if keep {
			continue
		}
		*budget--
		if nested {
			if depth == 0 {
				return nil, false
			}
			children, ok := splitPreservedTarget(child, preserve, depth-1, budget)
			if !ok {
				return nil, false
			}
			out = append(out, children...)
		} else {
			out = append(out, child)
		}
	}
	return out, true
}
