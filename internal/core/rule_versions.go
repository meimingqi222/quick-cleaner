package core

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
	"time"
)

type VersionGuard struct {
	Layout   string   `json:"layout"`
	Current  Evidence `json:"current"`
	Kept     *string  `json:"kept"`
	Lock     Evidence `json:"lock"`
	Updating bool     `json:"updating"`
	Reason   string   `json:"reason,omitempty"`
}

func captureVersionGuard(layout VersionLayout, anchor string) VersionGuard {
	g := VersionGuard{Layout: layout.ID, Current: Confirmed, Lock: Confirmed}
	base := filepath.Join(anchor, filepath.FromSlash(strings.ReplaceAll(layout.Path, `\`, "/")))
	if layout.Current != nil {
		kept, err := currentVersionName(anchor, filepath.Join(base, layout.Versions), filepath.Join(base, *layout.Current))
		if err != nil {
			g.Current, g.Reason = Unknown, err.Error()
		} else if kept == "" {
			g.Current = Absent
		} else {
			g.Kept = &kept
		}
	}
	if layout.Lock != nil {
		path := filepath.Join(base, layout.Lock.Path)
		ok, err := ConfinedRuleFile(anchor, path)
		if !ok && err == nil {
			_, err = os.Lstat(path)
			if os.IsNotExist(err) {
				g.Lock = Absent
				return g
			}
			if err == nil {
				err = errors.New("Update lock is not a confined file")
			}
		}
		if err == nil {
			md, statErr := os.Lstat(path)
			err = statErr
			if err == nil {
				age := time.Since(md.ModTime())
				if age < 0 {
					err = errors.New("Update lock modification is in the future")
				} else {
					g.Updating = uint64(age/time.Second) < layout.Lock.MaximumAgeSeconds
				}
			}
		}
		if err != nil {
			g.Lock, g.Reason = Unknown, err.Error()
		}
	}
	return g
}
func (g VersionGuard) idle() bool { return g.Lock != Unknown && !g.Updating && g.Current != Unknown }

func currentVersionName(anchor, versions, pointer string) (string, error) {
	parent := filepath.Dir(pointer)
	if ok, err := ConfinedRulePath(anchor, parent); err != nil || !ok {
		return "", errors.New("Current parent unavailable")
	}
	md, err := os.Lstat(pointer)
	if os.IsNotExist(err) {
		return "", nil
	}
	if err != nil {
		return "", err
	}
	if !ruleEvidenceLink(md) {
		return "", errors.New("Current pointer is not a supported link")
	}
	target, err := os.Readlink(pointer)
	if err != nil {
		return "", err
	}
	if len(target) > 8192 {
		return "", errors.New("Current pointer budget exceeded")
	}
	if !filepath.IsAbs(target) {
		target = filepath.Join(parent, target)
	}
	resolved, err := filepath.EvalSymlinks(target)
	if err != nil {
		return "", err
	}
	resolvedVersions, err := filepath.EvalSymlinks(versions)
	if err != nil {
		return "", err
	}
	if Norm(filepath.Dir(resolved)) != Norm(resolvedVersions) {
		return "", errors.New("Current pointer escapes version inventory")
	}
	name := filepath.Base(resolved)
	if name == "." || name == ".." || len(name) > 128 || strings.TrimSpace(name) == "" || strings.ContainsAny(name, "/\\:\x00\r\n") {
		return "", errors.New("Invalid current version name")
	}
	candidate := filepath.Join(versions, name)
	if ok, err := ConfinedRulePath(anchor, candidate); !ok || err != nil {
		return "", errors.New("Current target is not a confined directory")
	}
	md, err = os.Lstat(candidate)
	if err != nil || !md.IsDir() || ruleEvidenceLink(md) {
		return "", errors.New("Current target is not a confined directory")
	}
	return name, nil
}

func (g VersionGuard) revalidate(rule *RuleDefinition, anchor string) error {
	if anchor == "" {
		return errors.New("Missing version scope")
	}
	var layout *VersionLayout
	for i := range rule.VersionLayouts {
		if rule.VersionLayouts[i].ID == g.Layout {
			layout = &rule.VersionLayouts[i]
			break
		}
	}
	if layout == nil {
		return errors.New("Missing frozen version layout")
	}
	if ok, err := ConfinedRulePath(anchor, filepath.Join(anchor, layout.Path)); !ok || err != nil {
		return errors.New("Version root changed or unavailable")
	}
	live := captureVersionGuard(*layout, anchor)
	if !g.idle() || !live.idle() {
		return errors.New("Version update is active or unknown")
	}
	if g.Current != live.Current || (g.Kept == nil) != (live.Kept == nil) || g.Kept != nil && *g.Kept != *live.Kept {
		return errors.New("Current version changed after scanning")
	}
	return nil
}

func RuleVersionTargets(snapshot *RuleSnapshot, roots map[string]string, osName string) []ScanTarget {
	var out []ScanTarget
	enumeration := &ruleEnumeration{}
	for _, rule := range snapshot.Bundle.Rules {
		if rule.Platform != "all" && rule.Platform != osName {
			continue
		}
		for _, layout := range rule.VersionLayouts {
			if layout.Platform != "all" && layout.Platform != "" && !(layout.Platform == "windows" && osName == "windows" || layout.Platform == "unix" && osName != "windows") {
				continue
			}
			anchor := roots[string(layout.Root)]
			if anchor == "" {
				continue
			}
			base := filepath.Join(anchor, layout.Path)
			if ok, err := enumeration.directory(anchor, base); !ok || err != nil {
				continue
			}
			children, ok := enumeration.children(anchor, filepath.Join(base, layout.Versions))
			if !ok {
				continue
			}
			ref := NewRuleRef(snapshot, rule.ID, anchor)
			if ref.observation.Detected != Confirmed {
				continue
			}
			guard := captureVersionGuard(layout, anchor)
			ref.observation.VersionGuard = &guard
			ref.blocked = guard.Reason
			for _, child := range children {
				if !child.directory || member(child.name, layout.Exclude) || guard.Kept != nil && *guard.Kept == child.name {
					continue
				}
				prefix := len(layout.Prefixes) == 0
				for _, p := range layout.Prefixes {
					prefix = prefix || strings.HasPrefix(child.name, p)
				}
				if !prefix {
					continue
				}
				path := child.path
				if layout.Leaf != nil {
					path = filepath.Join(path, *layout.Leaf)
				}
				if ok, err := enumeration.directory(anchor, path); !ok || err != nil {
					continue
				}
				enumeration.emit(&out, ScanTarget{Path: path, Label: NewText(strings.ReplaceAll(layout.Zh, "{name}", child.name), strings.ReplaceAll(layout.En, "{name}", child.name)), Category: CategoryID(layout.Category), Operation: Operation{Kind: OperationKind(layout.Operation)}, Disposal: layout.Disposal, Recommended: layout.Recommended && guard.idle() && guard.Current == Confirmed, Rule: ref}, "directory")
			}
			if d := layout.Downloads; d != nil {
				files, ok := enumeration.children(anchor, filepath.Join(base, d.Path))
				if !ok {
					continue
				}
				for _, file := range files {
					matches := false
					for _, suffix := range d.Suffixes {
						matches = matches || strings.HasSuffix(file.name, suffix)
					}
					if file.directory || !matches {
						continue
					}
					enumeration.emit(&out, ScanTarget{Path: file.path, Label: NewText(strings.ReplaceAll(d.Zh, "{name}", file.name), strings.ReplaceAll(d.En, "{name}", file.name)), Category: CategoryID(layout.Category), Operation: Operation{Kind: OperationKind(d.Operation)}, Disposal: d.Disposal, Recommended: d.Recommended && guard.idle(), Rule: ref}, "file")
				}
			}
		}
	}
	return out
}
