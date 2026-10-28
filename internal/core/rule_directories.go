package core

import (
	"os"
	"path/filepath"
	"sort"
	"strings"
)

type rulePathExpansion struct {
	path     string
	bindings map[string]Text
}

func expandDirectoryPaths(rule RuleDefinition, template string) []rulePathExpansion {
	expanded := []rulePathExpansion{{bindings: map[string]Text{}}}
	for _, segment := range strings.Split(strings.ReplaceAll(template, `\`, "/"), "/") {
		values := []rulePathExpansion{{path: segment}}
		if strings.HasPrefix(segment, "{") && strings.HasSuffix(segment, "}") {
			key := strings.TrimSuffix(strings.TrimPrefix(segment, "{"), "}")
			values = nil
			if rows := rule.Catalogs[key]; len(rows) > 0 {
				for _, row := range rows {
					values = append(values, rulePathExpansion{path: row.Path, bindings: map[string]Text{segment: NewText(row.Zh, row.En)}})
				}
			} else {
				for _, name := range rule.Lists[key] {
					values = append(values, rulePathExpansion{path: name, bindings: map[string]Text{segment: SameText(name)}})
				}
			}
		}
		var next []rulePathExpansion
		for _, prefix := range expanded {
			for _, value := range values {
				if len(next) >= 64 {
					return nil
				}
				bindings := map[string]Text{}
				for key, text := range prefix.bindings {
					bindings[key] = text
				}
				for key, text := range value.bindings {
					bindings[key] = text
				}
				path := value.path
				if prefix.path != "" {
					path = prefix.path + "/" + value.path
				}
				next = append(next, rulePathExpansion{path, bindings})
			}
		}
		expanded = next
	}
	return expanded
}

func directoryRuleRoot(root DirectoryRoot, roots map[string]string) string {
	if root == "user_temp_parent" {
		if temp := roots["user_temp"]; temp != "" {
			return filepath.Dir(temp)
		}
		return ""
	}
	return roots[string(root)]
}

func RuleDirectoryTargets(snapshot *RuleSnapshot, roots map[string]string, osName string) []ScanTarget {
	var out []ScanTarget
	enumeration := &ruleEnumeration{}
	for _, rule := range snapshot.Bundle.Rules {
		if rule.Platform != "all" && rule.Platform != osName {
			continue
		}
		observed := map[string]RuleRef{}
		for _, entry := range rule.Directories {
			anchor := directoryRuleRoot(entry.Root, roots)
			if anchor == "" {
				continue
			}
			if ok, err := enumeration.directory(anchor, anchor); !ok || err != nil {
				continue
			}
			ref, exists := observed[anchor]
			if !exists {
				ref = NewRuleRef(snapshot, rule.ID, anchor)
				observed[anchor] = ref
			}
			if ref.observation.Detected != Confirmed {
				continue
			}
			expected := "directory"
			switch entry.Select.Kind {
			case "children":
				if !entry.Select.DirectoriesOnly {
					expected = "any"
				}
			case "named_files", "files":
				expected = "file"
			case "path":
				if !entry.Select.DirectoriesOnly {
					expected = "file"
				}
			}
			for _, declared := range entry.Paths {
				for _, expanded := range expandDirectoryPaths(rule, declared) {
					relative := expanded.path
					root := filepath.Join(anchor, relative)
					push := func(path string, label Text, category CategoryID, recommended bool, operation Operation) {
						enumeration.emit(&out, ScanTarget{Path: path, Label: label, Category: category, Recommended: recommended, Operation: operation, Disposal: entry.Disposal, Rule: ref}, expected)
					}
					emit := func(path, name, rel string, extra map[string]string, recommended bool, operation Operation) {
						render := func(template string, lang Language) string {
							text := strings.NewReplacer("{name}", name, "{parent}", filepath.Base(filepath.Dir(path)), "{relative}", rel).Replace(template)
							for key, value := range expanded.bindings {
								text = strings.ReplaceAll(text, key, value.Get(lang))
							}
							for key, value := range extra {
								text = strings.ReplaceAll(text, key, value)
							}
							return text
						}
						push(path, NewText(render(entry.Zh, Chinese), render(entry.En, English)), CategoryID(entry.Category), entry.Recommended && recommended, operation)
					}
					op := Operation{Kind: OperationKind(entry.Operation)}
					s := entry.Select
					switch s.Kind {
					case "path":
						emit(root, filepath.Base(root), relative, nil, true, op)
						continue
					case "manifest_children":
						rows, ok := enumeration.manifest(anchor, filepath.Join(root, s.Manifest)).(map[string]any)
						if !ok {
							continue
						}
						enumeration.manifestRows += len(rows)
						if enumeration.entries+enumeration.manifestRows > 16384 {
							continue
						}
						keys := make([]string, 0, len(rows))
						for key := range rows {
							keys = append(keys, key)
						}
						sort.Strings(keys)
						for i, key := range keys {
							if i >= 4096 {
								break
							}
							selected, ok := rows[key].(bool)
							if ok && selected == s.SelectedValue && ruleName(key) {
								emit(filepath.Join(root, key), key, relative+"/"+key, nil, true, op)
							}
						}
						continue
					case "signature_leaves":
						for _, leaf := range chromiumLeaves(snapshot, enumeration, anchor, root, s.Signature) {
							emit(leaf.path, leaf.name, relative, map[string]string{"{trail}": leaf.trail}, chromiumLeafRecommended(snapshot, leaf.name), op)
						}
						continue
					case "linked_worktrees":
						containers := []string{root}
						if len(s.Containers) > 0 {
							containers = nil
							for _, name := range s.Containers {
								containers = append(containers, filepath.Join(root, name))
							}
						}
						for _, container := range containers {
							if ok, err := enumeration.directory(anchor, container); !ok || err != nil {
								continue
							}
							for _, path := range DiscoverWorktrees(container) {
								if worktree, err := InspectWorktree(path); err == nil {
									name := filepath.Base(path)
									emit(path, name, relative+"/"+name, nil, true, Operation{Kind: OpGitWorktree, Registration: worktree.Admin})
								}
							}
						}
						continue
					}
					children, ok := enumeration.children(anchor, root)
					if !ok {
						continue
					}
					if s.Kind == "layout_children" {
						for _, row := range rule.Catalogs[s.Catalog] {
							if row.Path == relative {
								for _, declared := range row.Children {
									for _, child := range children {
										if child.directory && child.name == declared.Path {
											emit(child.path, child.name, relative+"/"+child.name, nil, declared.Recommended, op)
										}
									}
								}
							}
						}
						continue
					}
					for _, child := range children {
						rel := relative + "/" + child.name
						switch s.Kind {
						case "children", "files":
							if s.Kind == "children" && s.DirectoriesOnly && !child.directory || s.Kind == "files" && child.directory {
								continue
							}
							prefix, suffix := len(s.Prefixes) == 0, len(s.Suffixes) == 0
							for _, value := range s.Prefixes {
								prefix = prefix || strings.HasPrefix(child.name, value)
							}
							for _, value := range s.Suffixes {
								suffix = suffix || strings.HasSuffix(child.name, value)
							}
							if !prefix || !suffix {
								continue
							}
							recommended := true
							if p := entry.LogPolicy; p != nil {
								extension := strings.TrimPrefix(filepath.Ext(child.name), ".")
								recommended = child.directory
								for _, ext := range p.Extensions {
									recommended = recommended || strings.EqualFold(extension, ext)
								}
								recommended = recommended && !member(child.name, p.ExcludeExact)
								for _, prefix := range p.ExcludePrefixes {
									if strings.HasPrefix(child.name, prefix) {
										recommended = false
									}
								}
								if child.directory && HoldsLiveDatabase(child.path) {
									recommended = false
								}
							}
							emit(child.path, child.name, rel, nil, recommended, op)
						case "container_directories":
							if !child.directory || snapshot.MatchesName(rule.ID, s.ExcludeNamePolicy, strings.ToLower(child.name)) {
								continue
							}
							for _, suffix := range s.Paths {
								emit(filepath.Join(child.path, suffix), child.name, rel+"/"+suffix, nil, false, op)
							}
						case "named_files", "named_directories":
							wantDir := s.Kind == "named_directories"
							if child.name == s.Name && child.directory == wantDir {
								emit(child.path, s.Name, relative, nil, true, op)
							} else if child.directory && s.ChildDepth == 1 {
								emit(filepath.Join(child.path, s.Name), s.Name, rel, nil, true, op)
							}
						case "orphaned_children":
							if !child.directory {
								continue
							}
							rows := enumeration.manifest(anchor, filepath.Join(child.path, s.Manifest))
							value, ok := jsonPointer(rows, s.Pointer)
							if !ok {
								continue
							}
							recorded, ok := value.(string)
							if !ok || !strings.HasPrefix(recorded, s.URIPrefix) {
								continue
							}
							project := strings.TrimPrefix(recorded, s.URIPrefix)
							scope := directoryRuleRoot(s.Scope, roots)
							relativeProject, err := filepath.Rel(scope, project)
							if scope == "" || err != nil || !filepath.IsAbs(project) || strings.Contains(project, "%") || relativeProject == ".." || strings.HasPrefix(relativeProject, ".."+string(filepath.Separator)) {
								continue
							}
							if _, err := os.Stat(project); !os.IsNotExist(err) {
								continue
							}
							emit(child.path, child.name, rel, map[string]string{"{project}": project}, true, op)
						case "catalog_children":
							if !child.directory {
								continue
							}
							if p := s.LeafPolicy; p != nil {
								leaves := chromiumLeaves(snapshot, enumeration, anchor, child.path, p.Signature)
								if len(leaves) > 0 {
									for _, leaf := range leaves {
										render := func(text string) string {
											return strings.NewReplacer("{name}", child.name, "{trail}", leaf.trail).Replace(text)
										}
										push(leaf.path, NewText(render(p.Zh), render(p.En)), CategoryID(p.Category), chromiumLeafRecommended(snapshot, leaf.name), op)
									}
									continue
								}
							}
							matched := false
							keys := make([]string, 0, len(s.CatalogPolicy))
							for key := range s.CatalogPolicy {
								keys = append(keys, key)
							}
							sort.Strings(keys)
							for _, key := range keys {
								policy := s.CatalogPolicy[key]
								for _, row := range rule.Catalogs[key] {
									if row.Path == child.name {
										push(child.path, NewText(row.Zh, row.En), CategoryID(policy.Category), policy.Recommended, op)
										matched = true
										break
									}
								}
								if matched {
									break
								}
							}
							if !matched {
								emit(child.path, child.name, rel, nil, true, op)
							}
						}
					}
				}
			}
		}
	}
	return out
}
