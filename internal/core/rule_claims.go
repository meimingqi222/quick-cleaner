package core

import (
	"path/filepath"
	"sort"
	"strings"
)

// Claims describe declaration ownership, not filesystem existence or permission
// to delete. Providers use them to avoid counting declared targets twice.
type RuleClaims struct {
	owned    map[string]bool
	children map[string]map[string]bool
}

func CollectRuleClaims(snapshot *RuleSnapshot, roots map[string]string, osName string) RuleClaims {
	c := RuleClaims{owned: map[string]bool{}, children: map[string]map[string]bool{}}
	claim := func(path string) {
		c.owned[Norm(path)] = true
		parent := Norm(filepath.Dir(path))
		if c.children[parent] == nil {
			c.children[parent] = map[string]bool{}
		}
		c.children[parent][filepath.Base(path)] = true
	}
	for _, rule := range snapshot.Bundle.Rules {
		if rule.Platform != "all" && rule.Platform != osName {
			continue
		}
		for _, entry := range rule.Entries {
			if root := roots[entry.Root]; root != "" && !strings.Contains(entry.Path, "{") {
				claim(filepath.Join(root, strings.ReplaceAll(entry.Path, `\`, "/")))
			}
		}
		for _, entry := range rule.Directories {
			root := directoryRuleRoot(entry.Root, roots)
			if root == "" {
				continue
			}
			for _, template := range entry.Paths {
				for _, expanded := range expandDirectoryPaths(rule, template) {
					switch entry.Select.Kind {
					case "path", "signature_leaves":
						claim(filepath.Join(root, expanded.path))
					case "layout_children":
						for _, row := range rule.Catalogs[entry.Select.Catalog] {
							if row.Path == expanded.path {
								for _, child := range row.Children {
									claim(filepath.Join(root, expanded.path, child.Path))
								}
								break
							}
						}
					case "children", "container_directories", "named_files", "named_directories", "files", "manifest_children", "orphaned_children", "catalog_children", "linked_worktrees":
						// These discover beneath the declaration; claiming the root
						// would hide its siblings from provider discovery.
					}
				}
			}
		}
	}
	return c
}

func (c RuleClaims) Owns(dir, name string) bool {
	return c.owned[Norm(filepath.Join(dir, name))]
}

func (c RuleClaims) ClaimedChildren(dir, name string) []string {
	var out []string
	for child := range c.children[Norm(filepath.Join(dir, name))] {
		out = append(out, child)
	}
	sort.Strings(out)
	return out
}
