package core

import (
	"os"
	"path/filepath"
	"strings"
)

type signedRuleLeaf struct{ path, name, trail string }

// Vocabulary and display-only leaves come from the captured rule snapshot.
// Profiles, session stores and Crashpad/pending are never whole-tree targets.
func chromiumLeaves(s *RuleSnapshot, e *ruleEnumeration, anchor, dir, signature string) []signedRuleLeaf {
	if signature != "chromium" && signature != "chromium_declared" {
		return nil
	}
	directory := func(path string) bool {
		ok, err := ConfinedRulePath(anchor, path)
		if !ok || err != nil {
			return false
		}
		md, err := os.Lstat(path)
		return err == nil && md.IsDir() && !ruleEvidenceLink(md)
	}
	count := func(path string, names []string) int {
		n := 0
		for _, name := range names {
			if directory(filepath.Join(path, name)) {
				n++
			}
		}
		return n
	}
	confirmed := func(path string) bool {
		return count(path, s.List("chromium", "strong_leaves")) > 0 || count(path, s.List("chromium", "signature_leaves")) >= int(max(2, s.Number("chromium", "signature_min", 2)))
	}
	var out []signedRuleLeaf
	collect := func(path string) {
		names := append(s.List("chromium", "signature_leaves"), s.List("chromium", "extra_leaves")...)
		names = append(names, "Crashpad/completed")
		for _, name := range names {
			leaf := filepath.Join(path, name)
			if !directory(leaf) {
				continue
			}
			file, err := os.Open(leaf)
			if err != nil {
				continue
			}
			children, _ := file.ReadDir(1)
			file.Close()
			if len(children) == 0 {
				continue
			}
			relative, err := filepath.Rel(dir, leaf)
			if err != nil {
				continue
			}
			parts := strings.Split(filepath.ToSlash(relative), "/")
			out = append(out, signedRuleLeaf{leaf, parts[len(parts)-1], strings.Join(parts, " · ")})
		}
	}
	rootConfirmed := signature == "chromium_declared" || confirmed(dir)
	if rootConfirmed {
		collect(dir)
	}
	children, _ := e.children(anchor, dir)
	var profiles []string
	for _, child := range children {
		if !child.directory {
			continue
		}
		if s.MatchesName("chromium", "profile", child.name) {
			profiles = append(profiles, child.path)
		}
		if strings.HasSuffix(child.name, "-profile") {
			inner, _ := e.children(anchor, child.path)
			for _, profile := range inner {
				if profile.directory && s.MatchesName("chromium", "profile", profile.name) {
					profiles = append(profiles, profile.path)
				}
			}
		}
	}
	for _, profile := range profiles {
		if signature == "chromium_declared" || confirmed(profile) || rootConfirmed && count(profile, s.List("chromium", "signature_leaves")) >= 1 {
			collect(profile)
		}
	}
	return out
}

func chromiumLeafRecommended(snapshot *RuleSnapshot, name string) bool {
	return !member(name, snapshot.List("chromium", "shown_leaves"))
}
