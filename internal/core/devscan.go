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
	"sync"
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
	return buildCandidateAllowedWith(path, marker, os.Stat)
}

func buildCandidateAllowedWith(path string, marker BuildMarker, stat func(string) (os.FileInfo, error)) bool {
	if stat == nil {
		stat = os.Stat
	}
	if IsProtectedResidualPath(path) || developmentInstallationPath(path, stat) {
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
			md, err := stat(filepath.Join(parent, "src"))
			return err == nil && md.IsDir() && strings.TrimSpace(build) != ""
		}
		return true
	case ".venv", "venv":
		md, err := stat(filepath.Join(path, "pyvenv.cfg"))
		return err == nil && md.Mode().IsRegular()
	default:
		return true
	}
}

// treeNamesCacheDirTag is the indexed channel's name check. Rust opens
// CACHEDIR.TAG only after the in-memory tree already lists that file.
// A directory the tree does not name is not a tag, so the scan must not
// CreateFile it. The walk channel has no tree and still opens the file.
func treeNamesCacheDirTag(tree *SizeTree, dir uint32) bool {
	if tree == nil || int(dir) >= len(tree.children) {
		return false
	}
	if tree.baseCount == 0 {
		if int(dir) >= len(tree.entries) || !tree.entries[dir].Used {
			return false
		}
		for _, child := range tree.children[dir] {
			if int(child) >= len(tree.entries) {
				continue
			}
			e := &tree.entries[child]
			if e.Used && !e.IsDir && isCacheDirTagName(e.Name) {
				return true
			}
		}
		return false
	}
	if !tree.Valid(dir) {
		return false
	}
	for _, child := range tree.children[dir] {
		if tree.Valid(child) && !tree.IsDir(child) && isCacheDirTagName(tree.EntryName(child)) {
			return true
		}
	}
	return false
}

// asciiEqFold matches Rust's eq_ignore_ascii_case. Marker and skip names are
// ASCII; unicode EqualFold on every directory dominated the indexed walk.
func asciiEqFold(a, b string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := 0; i < len(a); i++ {
		ca, cb := a[i], b[i]
		if ca == cb {
			continue
		}
		if ca >= 'A' && ca <= 'Z' {
			ca += 'a' - 'A'
		}
		if cb >= 'A' && cb <= 'Z' {
			cb += 'a' - 'A'
		}
		if ca != cb {
			return false
		}
	}
	return true
}

func isCacheDirTagName(name string) bool {
	return len(name) == 12 && asciiEqFold(name, "CACHEDIR.TAG")
}

// hasCacheDirectoryTag opens before confining: almost every directory has no
// tag, and confining first stats the whole ancestor chain for each of them.
// The opened handle must be the confined file, so a swap after the open fails.
func hasCacheDirectoryTag(anchor, dir string) bool {
	path := filepath.Join(dir, "CACHEDIR.TAG")
	file, err := os.Open(path)
	if err != nil {
		return false
	}
	defer file.Close()
	if ok, err := ConfinedRuleFile(anchor, path); !ok || err != nil {
		return false
	}
	opened, err := file.Stat()
	if err != nil {
		return false
	}
	if current, err := os.Lstat(path); err != nil || !os.SameFile(opened, current) {
		return false
	}
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
	if tree != nil {
		depth := int(^uint(0) >> 1)
		if runtime.GOOS == "windows" {
			depth = 12
		}
		return discoverIndexed(ctx, s, rule, tree, depth)
	}
	return discoverWalked(ctx, s, rule, roots)
}

type walkHit struct {
	anchor, path string
	marker       BuildMarker
}

// walkCollector owns one code root. Ancestor evidence is memoized for that
// walk only; each hit is confined again without the memo before measurement.
type walkCollector struct {
	ctx    context.Context
	rule   *RuleDefinition
	anchor string
	io     *discoverIO
	hits   []walkHit
}

func (w *walkCollector) collect(dir string, depth, maxDepth int) {
	if w.ctx.Err() != nil || depth > maxDepth {
		return
	}
	if ok, err := confinedRulePath(w.anchor, dir, false, w.io.Lstat, w.io.Eval); !ok || err != nil {
		return
	}
	file, err := os.Open(dir)
	if err != nil {
		return
	}
	var children []ruleChild
	var files []string
	for w.ctx.Err() == nil {
		batch, err := file.ReadDir(128)
		if err != nil && err != io.EOF {
			break
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
	_ = file.Close()
	for _, child := range children {
		if w.ctx.Err() != nil {
			return
		}
		lower := strings.ToLower(child.name)
		if skippedBuildDir(lower) {
			continue
		}
		var marker *BuildMarker
		for i := range w.rule.Markers {
			m := &w.rule.Markers[i]
			if m.Dir == lower && buildSiblingMatches(files, m.SiblingAny) {
				marker = m
				break
			}
		}
		if marker == nil && stopsWithoutBuildSibling(lower) {
			continue
		}
		if marker == nil && hasCacheDirectoryTag(w.anchor, child.path) {
			marker = &BuildMarker{Dir: "<cachedir>", Zh: "缓存目录（CACHEDIR.TAG）", En: "Cache directory (CACHEDIR.TAG)"}
		}
		if marker == nil {
			w.collect(child.path, depth+1, maxDepth)
			continue
		}
		if buildCandidateAllowedWith(child.path, *marker, w.io.Stat) {
			w.hits = append(w.hits, walkHit{w.anchor, child.path, *marker})
		}
	}
}

// discoverWalked collects code roots concurrently and measures hits on one
// shared worker budget, as Rust does with rayon. Output follows root order.
func discoverWalked(ctx context.Context, s *RuleSnapshot, rule *RuleDefinition, roots []CodeRoot) []ScanItem {
	workers := max(1, runtime.GOMAXPROCS(0))
	perRoot := make([][]walkHit, len(roots))
	boundedEach(ctx, len(roots), workers, func(i int) {
		w := walkCollector{ctx: ctx, rule: rule, anchor: roots[i].Path, io: &discoverIO{}}
		w.collect(roots[i].Path, 0, roots[i].MaxDepth)
		perRoot[i] = w.hits
	})
	seen := map[string]bool{}
	var hits []walkHit
	for _, list := range perRoot {
		for _, hit := range list {
			if key := Norm(hit.path); !seen[key] {
				seen[key] = true
				hits = append(hits, hit)
			}
		}
	}
	items := make([]*ScanItem, len(hits))
	slots := make(chan struct{}, workers)
	boundedEach(ctx, len(hits), workers, func(i int) {
		hit := hits[i]
		if ok, err := ConfinedRulePath(hit.anchor, hit.path); !ok || err != nil {
			return
		}
		identity, ok := CaptureIdentity(hit.path)
		if !ok {
			return
		}
		measurement, ok := measurePathShared(ctx, hit.path, slots)
		if !ok || measurement.Size == 0 {
			return
		}
		parts := strings.Split(filepath.ToSlash(hit.path), "/")
		short := strings.Join(parts[max(0, len(parts)-3):], `\`)
		if len(parts) > 3 {
			short = `…\` + short
		}
		item := ScanItem{Rule: NewRuleRef(s, "build", ""), Path: hit.path, Operation: Operation{Kind: OpTree}, Disposal: Permanent, Category: DevBuild, Label: NewText(hit.marker.Zh+" · "+short, hit.marker.En+" · "+short), Size: measurement.Size, FileCount: measurement.Files, LastModified: measurement.NewestMtime, Identity: &identity}
		frozen := item.FreezePlan()
		items[i] = &frozen
	})
	var out []ScanItem
	for _, item := range items {
		if item != nil {
			out = append(out, *item)
		}
	}
	return out
}

// boundedEach runs fn for 0..n-1 on at most workers goroutines and stops
// handing out indices once ctx is cancelled.
func boundedEach(ctx context.Context, n, workers int, fn func(int)) {
	next := make(chan int)
	var wg sync.WaitGroup
	for range min(n, workers) {
		wg.Go(func() {
			for i := range next {
				fn(i)
			}
		})
	}
	for i := 0; i < n && ctx.Err() == nil; i++ {
		next <- i
	}
	close(next)
	wg.Wait()
}

// discoverIndexed walks directory edges only. A path string is built only for
// a hit or a named CACHEDIR.TAG. File nodes stay out of this loop: the tag
// bitset is one sequential pass, and sibling names are read only after a
// directory name matches a marker.
func discoverIndexed(ctx context.Context, s *RuleSnapshot, rule *RuleDefinition, tree *SizeTree, maxDepth int) []ScanItem {
	if ctx.Err() != nil {
		return nil
	}
	anchor := tree.volume.MountPoint()
	seen := map[string]bool{}
	var out []ScanItem
	lead, filterLead := markerLeadBytes(rule.Markers)
	csr, tags, heap := buildHeapDirCSR(tree)
	pathCache := map[uint32]string{}
	ioCache := &discoverIO{}
	var walk func(uint32, int)
	walk = func(dir uint32, depth int) {
		if ctx.Err() != nil || depth > maxDepth {
			return
		}
		var children []uint32
		if heap {
			if int(dir)+1 >= len(csr.start) {
				return
			}
			children = csr.at[csr.start[dir]:csr.start[dir+1]]
		} else if int(dir) < len(tree.children) {
			children = tree.children[dir]
		} else {
			return
		}
		for _, child := range children {
			name, ok := indexedDirName(tree, child, heap)
			if !ok {
				continue
			}
			if skippedBuildDir(name) || ntfsMetaDir(child, name) {
				continue
			}
			var marker *BuildMarker
			if !filterLead || (name != "" && lead[name[0]]) {
				for i := range rule.Markers {
					if len(rule.Markers[i].Dir) == len(name) && asciiEqFold(name, rule.Markers[i].Dir) && treeSiblingsMatch(tree, dir, rule.Markers[i].SiblingAny) {
						marker = &rule.Markers[i]
						break
					}
				}
			}
			if marker == nil && stopsWithoutBuildSibling(name) {
				continue
			}
			if marker == nil && tagBit(tags, child) {
				path := tree.PathOfWith(child, pathCache)
				if hasCacheDirectoryTag(anchor, path) {
					marker = &BuildMarker{Dir: "<cachedir>", Zh: "缓存目录（CACHEDIR.TAG）", En: "Cache directory (CACHEDIR.TAG)"}
				}
			}
			if marker == nil {
				walk(child, depth+1)
				continue
			}
			path := tree.PathOfWith(child, pathCache)
			if item, hit := acceptBuildHit(s, tree, anchor, path, child, marker, seen, ioCache); hit {
				out = append(out, item)
			}
		}
	}
	walk(tree.Root(), 0)
	return out
}

// dirCSR lists only directory children, in entry order. The full child slice
// still mixes files in; scanning those files on every directory was the walk.
type dirCSR struct {
	start []uint32
	at    []uint32
}

func buildHeapDirCSR(tree *SizeTree) (dirCSR, []uint64, bool) {
	if tree == nil || tree.baseCount != 0 {
		return dirCSR{}, cacheTagBitsSlow(tree), false
	}
	entries := tree.entries
	n := len(entries)
	counts := make([]uint32, n)
	tags := make([]uint64, (n+63)>>6)
	for i := range entries {
		e := &entries[i]
		if e.Used && !e.IsDir && e.Parent != uint32(i) && isCacheDirTagName(e.Name) {
			p := e.Parent
			if int(p) < n && entries[p].Used && entries[p].IsDir {
				tags[p>>6] |= 1 << (p & 63)
			}
		}
		if i == 0 {
			continue
		}
		if p, ok := heapDirEdge(entries, i); ok {
			counts[p]++
		}
	}
	start := make([]uint32, n+1)
	var sum uint32
	for i := 0; i < n; i++ {
		start[i] = sum
		sum += counts[i]
	}
	start[n] = sum
	at := make([]uint32, sum)
	cursor := make([]uint32, n)
	copy(cursor, start[:n])
	for i := 1; i < n; i++ {
		p, ok := heapDirEdge(entries, i)
		if !ok {
			continue
		}
		at[cursor[p]] = uint32(i)
		cursor[p]++
	}
	return dirCSR{start: start, at: at}, tags, true
}

func heapDirEdge(entries []treeSlot, i int) (uint32, bool) {
	e := &entries[i]
	if !e.Used || !e.IsDir {
		return 0, false
	}
	p := e.Parent
	if p == uint32(i) || int(p) >= len(entries) || !entries[p].Used || !entries[p].IsDir {
		return 0, false
	}
	return p, true
}

func cacheTagBitsSlow(tree *SizeTree) []uint64 {
	if tree == nil {
		return nil
	}
	n := tree.nodeCount()
	tags := make([]uint64, (n+63)>>6)
	for i := 0; i < n; i++ {
		e := tree.slot(uint32(i))
		if !e.Used || e.IsDir || e.Parent == uint32(i) || !isCacheDirTagName(e.Name) {
			continue
		}
		p := e.Parent
		if int(p) >= n || !tree.Valid(p) || !tree.IsDir(p) {
			continue
		}
		tags[p>>6] |= 1 << (p & 63)
	}
	return tags
}

func tagBit(tags []uint64, idx uint32) bool {
	word := idx >> 6
	if int(word) >= len(tags) {
		return false
	}
	return tags[word]&(1<<(idx&63)) != 0
}

func markerLeadBytes(markers []BuildMarker) (lead [256]bool, use bool) {
	use = true
	for i := range markers {
		dir := markers[i].Dir
		if dir == "" {
			return [256]bool{}, false
		}
		lead[dir[0]] = true
		switch {
		case dir[0] >= 'a' && dir[0] <= 'z':
			lead[dir[0]-'a'+'A'] = true
		case dir[0] >= 'A' && dir[0] <= 'Z':
			lead[dir[0]-'A'+'a'] = true
		}
	}
	return lead, use
}

func indexedDirName(tree *SizeTree, idx uint32, heap bool) (string, bool) {
	if heap {
		if int(idx) >= len(tree.entries) {
			return "", false
		}
		e := &tree.entries[idx]
		if !e.Used || !e.IsDir {
			return "", false
		}
		return e.Name, true
	}
	if !tree.Valid(idx) || !tree.IsDir(idx) {
		return "", false
	}
	return tree.slot(idx).Name, true
}

func skippedBuildDir(name string) bool {
	switch len(name) {
	case 3:
		return asciiEqFold(name, ".hg")
	case 4:
		return asciiEqFold(name, ".git") || asciiEqFold(name, ".svn")
	case 7:
		return asciiEqFold(name, "appdata") || asciiEqFold(name, "windows")
	case 11:
		return asciiEqFold(name, "programdata")
	case 12:
		return asciiEqFold(name, "$recycle.bin") || asciiEqFold(name, "onedrivetemp")
	case 13:
		return asciiEqFold(name, "program files")
	case 19:
		return asciiEqFold(name, "program files (x86)")
	case 25:
		return asciiEqFold(name, "system volume information")
	default:
		return false
	}
}

func stopsWithoutBuildSibling(name string) bool {
	switch len(name) {
	case 4:
		return asciiEqFold(name, "venv")
	case 5:
		return asciiEqFold(name, ".venv")
	case 12:
		return asciiEqFold(name, "node_modules")
	default:
		return false
	}
}

// ntfsMetaDir matches Rust's MFT walker: early records named like metadata, and
// the shared meta-name list, are not project trees.
func ntfsMetaDir(idx uint32, name string) bool {
	if idx < 16 && (strings.HasPrefix(name, "$") || name == ".") {
		return true
	}
	if name == "" {
		return false
	}
	c := name[0]
	if c != '$' && c != 'D' && c != 'd' && c != 'P' && c != 'p' && c != 'H' && c != 'h' && c != 'S' && c != 's' {
		return false
	}
	if len(name) >= 4 && asciiEqFold(name[:4], "$mft") {
		return true
	}
	if len(name) >= 12 && asciiEqFold(name[:12], "dumpstack.log") {
		return true
	}
	switch len(name) {
	case 5:
		return asciiEqFold(name, "$boot")
	case 7:
		return asciiEqFold(name, "$volume") || asciiEqFold(name, "$bitmap") || asciiEqFold(name, "$secure") || asciiEqFold(name, "$upcase") || asciiEqFold(name, "$extend")
	case 8:
		return asciiEqFold(name, "$logfile") || asciiEqFold(name, "$attrdef") || asciiEqFold(name, "$badclus")
	case 12:
		return asciiEqFold(name, "pagefile.sys") || asciiEqFold(name, "hiberfil.sys") || asciiEqFold(name, "swapfile.sys")
	case 25:
		return asciiEqFold(name, "system volume information")
	default:
		return false
	}
}

func treeSiblingsMatch(tree *SizeTree, dir uint32, required []string) bool {
	if len(required) == 0 {
		return true
	}
	if tree == nil || int(dir) >= len(tree.children) {
		return false
	}
	kids := tree.children[dir]
	if tree.baseCount == 0 {
		entries := tree.entries
		for _, want := range required {
			ext := len(want) > 0 && want[0] == '.'
			for _, child := range kids {
				if int(child) >= len(entries) {
					continue
				}
				e := &entries[child]
				if !e.Used || e.IsDir {
					continue
				}
				if siblingNameMatch(e.Name, want, ext) {
					return true
				}
			}
		}
		return false
	}
	for _, want := range required {
		ext := len(want) > 0 && want[0] == '.'
		for _, child := range kids {
			if !tree.Valid(child) || tree.IsDir(child) {
				continue
			}
			if siblingNameMatch(tree.EntryName(child), want, ext) {
				return true
			}
		}
	}
	return false
}

func siblingNameMatch(name, want string, ext bool) bool {
	if ext {
		return len(name) > len(want) && asciiEqFold(filepath.Ext(name), want)
	}
	return asciiEqFold(name, want)
}

// discoverIO remembers Lstat, Stat and EvalSymlinks for one indexed walk.
// Ancestor evidence does not change during that walk, and stating it again
// for every candidate was most of the discovery time.
type discoverIO struct {
	lstat, stat statMemo
	real        map[string]string
	realErr     map[string]error
}

type statMemo struct {
	info map[string]os.FileInfo
	err  map[string]error
}

func (m *statMemo) call(path string, fn func(string) (os.FileInfo, error)) (os.FileInfo, error) {
	if m.info != nil {
		if info, ok := m.info[path]; ok {
			return info, nil
		}
	}
	if m.err != nil {
		if err, ok := m.err[path]; ok {
			return nil, err
		}
	}
	info, err := fn(path)
	if err != nil {
		if m.err == nil {
			m.err = map[string]error{}
		}
		m.err[path] = err
		return nil, err
	}
	if m.info == nil {
		m.info = map[string]os.FileInfo{}
	}
	m.info[path] = info
	return info, nil
}

func (d *discoverIO) Lstat(path string) (os.FileInfo, error) { return d.lstat.call(path, ruleLstat) }
func (d *discoverIO) Stat(path string) (os.FileInfo, error)  { return d.stat.call(path, os.Stat) }
func (d *discoverIO) Eval(path string) (string, error) {
	if d.real != nil {
		if resolved, ok := d.real[path]; ok {
			return resolved, nil
		}
	}
	if d.realErr != nil {
		if err, ok := d.realErr[path]; ok {
			return "", err
		}
	}
	resolved, err := filepath.EvalSymlinks(path)
	if err != nil {
		if d.realErr == nil {
			d.realErr = map[string]error{}
		}
		d.realErr[path] = err
		return "", err
	}
	if d.real == nil {
		d.real = map[string]string{}
	}
	d.real[path] = resolved
	return resolved, nil
}

func acceptBuildHit(s *RuleSnapshot, tree *SizeTree, anchor, path string, idx uint32, marker *BuildMarker, seen map[string]bool, ioCache *discoverIO) (ScanItem, bool) {
	// Rust reads the in-memory directory total first and skips an empty
	// directory without opening package.json or stating the path.
	if seen[Norm(path)] || !tree.Valid(idx) || tree.SizeOf(idx) == 0 {
		return ScanItem{}, false
	}
	if !buildCandidateAllowedWith(path, *marker, ioCache.Stat) {
		return ScanItem{}, false
	}
	if ok, err := confinedRulePath(anchor, path, false, ioCache.Lstat, ioCache.Eval); !ok || err != nil {
		return ScanItem{}, false
	}
	identity, ok := captureIdentityUsing(path, ioCache.Lstat)
	if !ok {
		return ScanItem{}, false
	}
	seen[Norm(path)] = true
	parts := strings.Split(filepath.ToSlash(path), "/")
	short := strings.Join(parts[max(0, len(parts)-3):], `\`)
	if len(parts) > 3 {
		short = `…\` + short
	}
	item := ScanItem{Rule: NewRuleRef(s, "build", ""), Path: path, Operation: Operation{Kind: OpTree}, Disposal: Permanent, Category: DevBuild, Label: NewText(marker.Zh+" · "+short, marker.En+" · "+short), Size: tree.SizeOf(idx), FileCount: tree.FileCountOf(idx), Identity: &identity}
	return item.FreezePlan(), true
}
