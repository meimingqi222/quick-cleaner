package core

import (
	"context"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"sort"
	"strings"
)

type CodeRoot struct {
	Path     string
	MaxDepth int
}

// CodeRoots takes trusted user/volume roots; it never substitutes the elevated
// process's home. Volume acquisition remains the platform's responsibility.
func CodeRoots(home string, volumes []VolumeID) []CodeRoot {
	names := []string{"code", "dev", "src", "source", "projects", "project", "repos", "repo", "work", "workspace", "workspaces", "git", "github", "gitee", "developer"}
	var roots []CodeRoot
	add := func(path string, depth int) {
		if md, err := os.Lstat(path); err == nil && md.IsDir() && !ruleEvidenceLink(md) {
			roots = append(roots, CodeRoot{path, depth})
		}
	}
	if home != "" {
		for _, name := range names {
			add(filepath.Join(home, name), 6)
		}
		add(home, 2)
	}
	for _, volume := range volumes {
		if _, ok := volume.DriveLetter(); ok {
			for _, name := range names {
				add(filepath.Join(volume.MountPoint(), name), 6)
			}
		}
	}
	sort.Slice(roots, func(i, j int) bool { return roots[i].Path < roots[j].Path })
	return slices.CompactFunc(roots, func(a, b CodeRoot) bool { return a.Path == b.Path })
}

func buildCandidateAllowed(path string, marker BuildMarker) bool {
	if IsProtectedResidualPath(path) || IsDevelopmentInstallationPath(path) {
		return false
	}
	parent := filepath.Dir(path)
	switch marker.Dir {
	case "node_modules", ".next", ".nuxt", ".svelte-kit", ".angular", "dist":
		file, err := os.Open(filepath.Join(parent, "package.json"))
		if err != nil {
			return false
		}
		defer file.Close()
		data, err := io.ReadAll(io.LimitReader(file, 1024*1024+1))
		if err != nil || len(data) > 1024*1024 {
			return false
		}
		var manifest map[string]any
		if json.Unmarshal(data, &manifest) != nil || manifest == nil {
			return false
		}
		if marker.Dir == "dist" {
			scripts, _ := manifest["scripts"].(map[string]any)
			build, _ := scripts["build"].(string)
			md, err := os.Stat(filepath.Join(parent, "src"))
			return err == nil && md.IsDir() && strings.TrimSpace(build) != ""
		}
		return true
	case ".venv", "venv":
		md, err := os.Stat(filepath.Join(path, "pyvenv.cfg"))
		return err == nil && md.Mode().IsRegular()
	default:
		return true
	}
}

func hasCacheDirectoryTag(anchor, dir string) bool {
	path := filepath.Join(dir, "CACHEDIR.TAG")
	if ok, err := ConfinedRuleFile(anchor, path); !ok || err != nil {
		return false
	}
	file, err := os.Open(path)
	if err != nil {
		return false
	}
	defer file.Close()
	const signature = "Signature: 8a477f597d28d172789f06886806bc55"
	var data [len(signature)]byte
	_, err = io.ReadFull(file, data[:])
	return err == nil && string(data[:]) == signature
}

func buildSiblingMatches(files []string, required []string) bool {
	if len(required) == 0 {
		return true
	}
	for _, want := range required {
		want = strings.ToLower(want)
		for _, file := range files {
			if strings.HasPrefix(want, ".") {
				if strings.ToLower(filepath.Ext(file)) == want && len(file) > len(want) {
					return true
				}
			} else if strings.EqualFold(want, file) {
				return true
			}
		}
	}
	return false
}

// DiscoverBuildArtifacts shares live project/install evidence across indexed
// and bounded walking channels. Passing a tree reuses its measurements; it does
// not acquire a native index. A hit (even one refused by evidence) stops descent.
func DiscoverBuildArtifacts(ctx context.Context, s *RuleSnapshot, roots []CodeRoot, tree *SizeTree) []ScanItem {
	rule := s.Definition("build")
	if rule == nil {
		return nil
	}
	query := NewFSIndexEngine(tree)
	seen := map[string]bool{}
	var out []ScanItem
	var collect func(string, string, int, int, *uint32)
	collect = func(anchor, dir string, depth, maxDepth int, node *uint32) {
		if ctx.Err() != nil || depth > maxDepth {
			return
		}
		var children []ruleChild
		var files []string
		nodes := map[string]uint32{}
		if node != nil {
			for _, idx := range tree.ChildIndices(*node) {
				if !tree.Valid(idx) {
					continue
				}
				name := tree.EntryName(idx)
				if tree.IsDir(idx) {
					path := filepath.Join(dir, name)
					children = append(children, ruleChild{path, name, true})
					nodes[path] = idx
				} else {
					files = append(files, name)
				}
			}
		} else {
			if ok, err := ConfinedRulePath(anchor, dir); !ok || err != nil {
				return
			}
			file, err := os.Open(dir)
			if err != nil {
				return
			}
			defer file.Close()
			for ctx.Err() == nil {
				batch, err := file.ReadDir(128)
				if err != nil && err != io.EOF {
					return
				}
				for _, entry := range batch {
					md, statErr := entry.Info()
					if statErr != nil || ruleEvidenceLink(md) {
						continue
					}
					if md.IsDir() {
						children = append(children, ruleChild{filepath.Join(dir, entry.Name()), entry.Name(), true})
					} else if md.Mode().IsRegular() {
						files = append(files, entry.Name())
					}
				}
				if err == io.EOF {
					break
				}
			}
		}
		for _, child := range children {
			if ctx.Err() != nil {
				return
			}
			lower := strings.ToLower(child.name)
			switch lower {
			case ".git", ".hg", ".svn", "appdata", "windows", "program files", "program files (x86)", "programdata", "$recycle.bin", "system volume information", "onedrivetemp":
				continue
			}
			var marker *BuildMarker
			for i := range rule.Markers {
				m := &rule.Markers[i]
				if m.Dir == lower && buildSiblingMatches(files, m.SiblingAny) {
					marker = m
					break
				}
			}
			if marker == nil && (lower == "node_modules" || lower == ".venv" || lower == "venv") {
				continue
			}
			if marker == nil && hasCacheDirectoryTag(anchor, child.path) {
				marker = &BuildMarker{Dir: "<cachedir>", Zh: "缓存目录（CACHEDIR.TAG）", En: "Cache directory (CACHEDIR.TAG)"}
			}
			if marker == nil {
				var next *uint32
				if idx, ok := nodes[child.path]; ok {
					next = &idx
				}
				collect(anchor, child.path, depth+1, maxDepth, next)
				continue
			}
			if seen[Norm(child.path)] || !buildCandidateAllowed(child.path, *marker) {
				continue
			}
			if ok, err := ConfinedRulePath(anchor, child.path); !ok || err != nil {
				continue
			}
			identity, ok := CaptureIdentity(child.path)
			if !ok {
				continue
			}
			measurement, ok := query.MeasurePath(ctx, child.path)
			if !ok || measurement.Size == 0 {
				continue
			}
			seen[Norm(child.path)] = true
			parts := strings.Split(filepath.ToSlash(child.path), "/")
			short := strings.Join(parts[max(0, len(parts)-3):], `\`)
			if len(parts) > 3 {
				short = `…\` + short
			}
			item := ScanItem{Rule: NewRuleRef(s, "build", ""), Path: child.path, Operation: Operation{Kind: OpTree}, Disposal: Permanent, Category: DevBuild, Label: NewText(marker.Zh+" · "+short, marker.En+" · "+short), Size: measurement.Size, FileCount: measurement.Files, LastModified: measurement.NewestMtime, Identity: &identity}
			out = append(out, item.FreezePlan())
		}
	}
	if tree != nil {
		depth := int(^uint(0) >> 1)
		if runtime.GOOS == "windows" {
			depth = 12
		}
		root := tree.Root()
		collect(tree.volume.MountPoint(), tree.volume.MountPoint(), 0, depth, &root)
	} else {
		for _, root := range roots {
			collect(root.Path, root.Path, 0, root.MaxDepth, nil)
		}
	}
	return out
}
