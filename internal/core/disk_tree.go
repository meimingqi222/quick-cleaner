package core

import (
	"container/heap"
	"context"
	"math"
	"path/filepath"
	"sort"
	"strings"
	"sync"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type VolumeID = platform.VolumeID

func VolumeFromMountPoint(mount string) VolumeID { return platform.VolumeFromMountPoint(mount) }
func VolumeFromMountPointWithLabel(mount, label string) VolumeID {
	return platform.VolumeFromMountPointWithLabel(mount, label)
}
func VolumeFromDriveLetter(letter byte) VolumeID {
	return platform.VolumeFromDriveLetter(letter)
}

type DirUsage struct {
	Path            string
	Size, FileCount uint64
}
type Node struct {
	Idx                      uint32
	Name                     string
	IsDir                    bool
	Size, FileCount, OwnSize uint64
}
type SearchHit struct {
	Path, Name  string
	IsDir       bool
	Size, Mtime uint64
}

const RootNode uint32 = 0

// TreeIndexEntry is a portable acquisition/persistence boundary, not a walk inventory.
// Used must be true for live slots. Directory Size is recomputed from descendants.
type TreeIndexEntry struct {
	Parent uint32
	Name   string
	IsDir  bool
	Size   uint64
	Used   bool
	Mtime  uint64
}
type TreeSnapshotEntry struct {
	Path        string
	IsDir       bool
	Size, Mtime uint64
}
type treeSlot struct {
	TreeIndexEntry
	files uint64
}

// SizeTree preserves stable node IDs across incremental edits. Readers may run
// concurrently, but callers must serialize mutations with all readers.
type SizeTree struct {
	volume     VolumeID
	entries    []treeSlot
	children   [][]uint32
	cacheMu    sync.Mutex
	childNames map[uint32]map[string]uint32
	blocked    []string
	ntfs       bool
	// extraParents records hard-link parents that are not the selected FILE_NAME parent.
	// RecomputeAggregates must add them back; the selected parent alone under-counts.
	extraParents map[uint32][]uint32
	// base is the mapped v7 file a loaded index keeps instead of a second heap
	// copy. Nil for trees built in memory. Reads consult it; writes land in
	// overrides so the mapping stays shared and read-only.
	base      *mappedTreeBase
	baseCount int
	overrides map[uint32]treeSlot
	// ancestorStamp is reused by every aggregate walk. A fresh map per file
	// dominated Windows MFT tree builds.
	ancestorStamp []uint32
	ancestorGen   uint32
}

func saturatingAdd(a, b uint64) uint64 {
	if math.MaxUint64-a < b {
		return math.MaxUint64
	}
	return a + b
}
func saturatingSub(a, b uint64) uint64 {
	if b > a {
		return 0
	}
	return a - b
}
func NewSizeTree(volume VolumeID, entries []TreeIndexEntry) *SizeTree {
	if len(entries) == 0 {
		entries = []TreeIndexEntry{{Name: volume.Label, IsDir: true, Used: true}}
	}
	slots := make([]treeSlot, len(entries))
	for i := range entries {
		slots[i].TreeIndexEntry = entries[i]
	}
	return newSizeTreeFromSlots(volume, slots)
}

// newSizeTreeFromSlots takes ownership of slots. The MFT build fills them
// directly; an intermediate []TreeIndexEntry was a second multi-million copy.
func newSizeTreeFromSlots(volume VolumeID, slots []treeSlot) *SizeTree {
	if len(slots) == 0 {
		slots = []treeSlot{{TreeIndexEntry: TreeIndexEntry{Name: volume.Label, IsDir: true, Used: true}}}
	}
	t := &SizeTree{volume: volume, childNames: make(map[uint32]map[string]uint32), entries: slots}
	t.RebuildChildArrays()
	t.RecomputeAggregates()
	return t
}

// heapEdge is the parent link a heap node contributes through: a used node
// under a different, used directory. Child arrays and aggregates share it.
func heapEdge(entries []treeSlot, i int) (uint32, bool) {
	e := &entries[i]
	p := e.Parent
	if i == 0 || !e.Used || p == uint32(i) || int(p) >= len(entries) || !entries[p].Used || !entries[p].IsDir {
		return 0, false
	}
	return p, true
}

// Clone owns independent mutable slots, edges and coverage metadata.
func (t *SizeTree) Clone() *SizeTree {
	out := &SizeTree{volume: t.volume, entries: append([]treeSlot(nil), t.entries...), blocked: append([]string(nil), t.blocked...), ntfs: t.ntfs, extraParents: cloneExtraParents(t.extraParents), base: t.base, baseCount: t.baseCount}
	if len(t.overrides) > 0 {
		out.overrides = make(map[uint32]treeSlot, len(t.overrides))
		for idx, slot := range t.overrides {
			out.overrides[idx] = slot
		}
	}
	out.RebuildChildArrays()
	return out
}
func SizeTreeFromSnapshot(volume VolumeID, snapshot []TreeSnapshotEntry) *SizeTree {
	snapshot = append([]TreeSnapshotEntry(nil), snapshot...)
	sort.SliceStable(snapshot, func(i, j int) bool {
		return strings.Count(filepath.Clean(snapshot[i].Path), string(filepath.Separator)) < strings.Count(filepath.Clean(snapshot[j].Path), string(filepath.Separator))
	})
	entries := []TreeIndexEntry{{Name: volume.Label, IsDir: true, Used: true}}
	paths := map[string]uint32{filepath.Clean(volume.Mount): 0}
	for _, e := range snapshot {
		path := filepath.Clean(e.Path)
		if path == filepath.Clean(volume.Mount) || !pathWithin(path, volume.Mount) {
			continue
		}
		parent, ok := paths[filepath.Dir(path)]
		if !ok || !entries[parent].IsDir {
			continue
		}
		paths[path] = uint32(len(entries))
		entries = append(entries, TreeIndexEntry{Parent: parent, Name: filepath.Base(path), IsDir: e.IsDir, Size: e.Size, Mtime: e.Mtime, Used: true})
	}
	return NewSizeTree(volume, entries)
}
func (t *SizeTree) Volume() VolumeID { return t.volume }
func (t *SizeTree) Root() uint32     { return 0 }
func (t *SizeTree) Valid(idx uint32) bool {
	return uint64(idx) < uint64(t.nodeCount()) && t.slot(idx).Used
}
func (t *SizeTree) IsDir(idx uint32) bool { return t.Valid(idx) && t.slot(idx).IsDir }

// nodeCount counts the mapped base plus nodes appended after it.
func (t *SizeTree) nodeCount() int {
	if t == nil {
		return 0
	}
	return t.baseCount + len(t.entries)
}

// slot reads one node. Mapped base nodes come from the shared file unless an
// override replaced them; appended nodes live in entries.
func (t *SizeTree) slot(idx uint32) treeSlot {
	if int(idx) < t.baseCount {
		if slot, ok := t.overrides[idx]; ok {
			return slot
		}
		return t.base.slot(idx)
	}
	return t.entries[int(idx)-t.baseCount]
}

// setSlot writes one node. A mapped base node becomes an override so the shared
// file is never mutated; an appended node is written in place.
func (t *SizeTree) setSlot(idx uint32, slot treeSlot) {
	if int(idx) < t.baseCount {
		if t.overrides == nil {
			t.overrides = map[uint32]treeSlot{}
		}
		t.overrides[idx] = slot
		return
	}
	t.entries[int(idx)-t.baseCount] = slot
}
func (t *SizeTree) EntryName(idx uint32) string {
	if !t.Valid(idx) {
		return ""
	}
	return t.slot(idx).Name
}
func (t *SizeTree) NameOf(idx uint32) string {
	if idx == 0 {
		return t.volume.Label
	}
	return t.EntryName(idx)
}
func (t *SizeTree) SizeOf(idx uint32) uint64 {
	if !t.Valid(idx) {
		return 0
	}
	return t.slot(idx).Size
}
func (t *SizeTree) FileCountOf(idx uint32) uint64 {
	if !t.Valid(idx) {
		return 0
	}
	if !t.IsDir(idx) {
		return 1
	}
	return t.slot(idx).files
}
func (t *SizeTree) ParentOf(idx uint32) (uint32, bool) {
	if idx == 0 || !t.Valid(idx) {
		return 0, false
	}
	p := t.slot(idx).Parent
	return p, p != idx && t.Valid(p)
}
func (t *SizeTree) ChildIndices(idx uint32) []uint32 {
	if !t.Valid(idx) {
		return nil
	}
	return append([]uint32(nil), t.children[idx]...)
}
func (t *SizeTree) PathOf(idx uint32) string { return t.PathOfWith(idx, make(map[uint32]string)) }
func (t *SizeTree) PathOfWith(idx uint32, cache map[uint32]string) string {
	if !t.Valid(idx) {
		return ""
	}
	var chain []uint32
	cur := idx
	base := ""
	for depth := 0; depth <= 512; depth++ {
		if cur == 0 {
			base = t.volume.Mount
			break
		}
		if p, ok := cache[cur]; ok {
			base = p
			break
		}
		if !t.Valid(cur) {
			return ""
		}
		chain = append(chain, cur)
		next := t.slot(cur).Parent
		if next == cur {
			return ""
		}
		cur = next
	}
	if base == "" {
		return ""
	}
	for i := len(chain) - 1; i >= 0; i-- {
		name := t.slot(chain[i]).Name
		// Never synthesize a deletion path from an untrusted index component.
		if name == "" || name == "." || name == ".." || strings.ContainsAny(name, "/\\") || filepath.IsAbs(name) {
			return ""
		}
		base = filepath.Join(base, name)
		cache[chain[i]] = base
	}
	return base
}
func (t *SizeTree) findChild(parent uint32, name string) (uint32, bool) {
	t.cacheMu.Lock()
	defer t.cacheMu.Unlock()
	if !t.IsDir(parent) {
		return 0, false
	}
	m, ok := t.childNames[parent]
	if !ok {
		m = make(map[string]uint32)
		for _, c := range t.children[parent] {
			if t.Valid(c) {
				key := asciiLower(t.slot(c).Name)
				if _, found := m[key]; !found {
					m[key] = c
				}
			}
		}
		t.childNames[parent] = m
	}
	key := asciiLower(name)
	idx, ok := m[key]
	if !ok {
		return 0, false
	}
	if t.Valid(idx) {
		return idx, true
	}
	for _, c := range t.children[parent] {
		if t.Valid(c) && asciiLower(t.slot(c).Name) == key {
			m[key] = c
			return c, true
		}
	}
	delete(m, key)
	return 0, false
}
func (t *SizeTree) FindPath(path string) []uint32 {
	if !pathWithin(path, t.volume.Mount) || !t.Valid(0) {
		return nil
	}
	rel, _ := filepath.Rel(t.volume.Mount, path)
	chain := []uint32{0}
	if rel == "." {
		return chain
	}
	cur := uint32(0)
	for _, comp := range strings.Split(rel, string(filepath.Separator)) {
		idx, ok := t.findChild(cur, comp)
		if !ok {
			break
		}
		chain = append(chain, idx)
		cur = idx
	}
	return chain
}
func (t *SizeTree) FindNodeByPath(path string) (uint32, bool) {
	if !pathWithin(path, t.volume.Mount) {
		return 0, false
	}
	rel, _ := filepath.Rel(t.volume.Mount, path)
	want := 1
	if rel != "." {
		want += len(strings.Split(rel, string(filepath.Separator)))
	}
	chain := t.FindPath(path)
	if len(chain) != want {
		return 0, false
	}
	return chain[len(chain)-1], true
}
func (t *SizeTree) ownSize(idx uint32) uint64 {
	if !t.IsDir(idx) {
		return t.SizeOf(idx)
	}
	var size uint64
	for _, c := range t.children[idx] {
		if t.Valid(c) && !t.IsDir(c) {
			size = saturatingAdd(size, t.SizeOf(c))
		}
	}
	return size
}
func (t *SizeTree) node(idx uint32) Node {
	return Node{idx, t.NameOf(idx), t.IsDir(idx), t.SizeOf(idx), t.FileCountOf(idx), t.ownSize(idx)}
}

func (t *SizeTree) hiddenNTFSMetadata(idx uint32) bool {
	if !t.ntfs {
		return false
	}
	name := t.EntryName(idx)
	return (idx < 16 && (strings.HasPrefix(name, "$") || name == ".")) || IsNTFSMetaName(name)
}

func (t *SizeTree) Children(idx uint32) []Node {
	var out []Node
	for _, c := range t.ChildIndices(idx) {
		if t.Valid(c) && !t.hiddenNTFSMetadata(c) {
			out = append(out, t.node(c))
		}
	}
	sort.SliceStable(out, func(i, j int) bool {
		if out[i].Size != out[j].Size {
			return out[i].Size > out[j].Size
		}
		return out[i].Name < out[j].Name
	})
	return out
}
func (t *SizeTree) ancestors(idx uint32, fn func(*treeSlot)) {
	n := t.nodeCount()
	if n == 0 {
		return
	}
	if cap(t.ancestorStamp) < n {
		t.ancestorStamp = make([]uint32, n)
	} else if len(t.ancestorStamp) != n {
		t.ancestorStamp = t.ancestorStamp[:n]
	}
	t.ancestorGen++
	if t.ancestorGen == 0 {
		clear(t.ancestorStamp)
		t.ancestorGen = 1
	}
	gen := t.ancestorGen
	// 256 matches the MFT parent walk. The stamp stops a cycle without a map.
	// Unmapped nodes are the MFT build. Copying the slot per file dominated it.
	// fn must not grow t.entries; the pointer would then address a stale array.
	if t.baseCount == 0 {
		entries := t.entries
		for depth := 0; depth <= 256 && int(idx) < len(entries) && entries[idx].Used; depth++ {
			if int(idx) >= len(t.ancestorStamp) || t.ancestorStamp[idx] == gen {
				return
			}
			t.ancestorStamp[idx] = gen
			fn(&entries[idx])
			if idx == 0 {
				return
			}
			next := entries[idx].Parent
			if next == idx {
				return
			}
			idx = next
		}
		return
	}
	for depth := 0; depth <= 256 && t.Valid(idx); depth++ {
		if int(idx) >= len(t.ancestorStamp) || t.ancestorStamp[idx] == gen {
			return
		}
		t.ancestorStamp[idx] = gen
		slot := t.slot(idx)
		fn(&slot)
		t.setSlot(idx, slot)
		if idx == 0 {
			return
		}
		next := slot.Parent
		if next == idx {
			return
		}
		idx = next
	}
}
func cloneExtraParents(in map[uint32][]uint32) map[uint32][]uint32 {
	if len(in) == 0 {
		return nil
	}
	out := make(map[uint32][]uint32, len(in))
	for k, v := range in {
		out[k] = append([]uint32(nil), v...)
	}
	return out
}

func (t *SizeTree) NoteExtraParent(file, parent uint32) {
	if t == nil || !t.Valid(file) || !t.Valid(parent) || t.slot(file).Parent == parent || t.slot(file).IsDir {
		return
	}
	if t.extraParents == nil {
		t.extraParents = map[uint32][]uint32{}
	}
	for _, existing := range t.extraParents[file] {
		if existing == parent {
			return
		}
	}
	t.extraParents[file] = append(t.extraParents[file], parent)
}

func (t *SizeTree) RecomputeAggregates() {
	if t.baseCount == 0 {
		t.recomputeHeapAggregates()
		return
	}
	for i := 0; i < t.nodeCount(); i++ {
		slot := t.slot(uint32(i))
		if slot.IsDir {
			slot.Size = 0
			slot.files = 0
			t.setSlot(uint32(i), slot)
		}
	}
	for i := 0; i < t.nodeCount(); i++ {
		e := t.slot(uint32(i))
		if !e.Used || e.IsDir {
			continue
		}
		t.ancestors(e.Parent, func(p *treeSlot) { p.Size = saturatingAdd(p.Size, e.Size); p.files = saturatingAdd(p.files, 1) })
		for _, parent := range t.extraParents[uint32(i)] {
			if parent == e.Parent || !t.Valid(parent) {
				continue
			}
			t.ancestors(parent, func(p *treeSlot) { p.Size = saturatingAdd(p.Size, e.Size); p.files = saturatingAdd(p.files, 1) })
		}
	}
}

// recomputeHeapAggregates rolls file sizes up in place. A closure per file
// allocated millions of times on an MFT tree; Rust updates the arrays directly.
//
// Each edge is visited once: files add to their directory, and a directory
// adds its total to its parent after all of its child directories have. A
// per-file ancestor walk was O(files × depth) and most of the MFT tree build.
// Directories in a cycle never drain, so a cycle stays local instead of
// reaching the root. Extra hard-link parents still walk their ancestors.
func (t *SizeTree) recomputeHeapAggregates() {
	entries := t.entries
	for i := range entries {
		if entries[i].IsDir {
			entries[i].Size = 0
			entries[i].files = 0
		}
	}
	n := len(entries)
	pending := make([]uint32, n)
	for i := range entries {
		p, ok := heapEdge(entries, i)
		if !ok {
			continue
		}
		if entries[i].IsDir {
			pending[p]++
		} else {
			entries[p].Size = saturatingAdd(entries[p].Size, entries[i].Size)
			entries[p].files = saturatingAdd(entries[p].files, 1)
		}
	}
	var ready []uint32
	for i := range entries {
		if entries[i].Used && entries[i].IsDir && pending[i] == 0 {
			ready = append(ready, uint32(i))
		}
	}
	for len(ready) > 0 {
		d := ready[len(ready)-1]
		ready = ready[:len(ready)-1]
		p, ok := heapEdge(entries, int(d))
		if !ok {
			continue
		}
		entries[p].Size = saturatingAdd(entries[p].Size, entries[d].Size)
		entries[p].files = saturatingAdd(entries[p].files, entries[d].files)
		if pending[p]--; pending[p] == 0 {
			ready = append(ready, p)
		}
	}
	if len(t.extraParents) == 0 {
		return
	}
	if cap(t.ancestorStamp) < n {
		t.ancestorStamp = make([]uint32, n)
	} else if len(t.ancestorStamp) != n {
		t.ancestorStamp = t.ancestorStamp[:n]
	}
	for i := range entries {
		e := &entries[i]
		if !e.Used || e.IsDir {
			continue
		}
		for _, parent := range t.extraParents[uint32(i)] {
			if parent == e.Parent || int(parent) >= n || !entries[parent].Used {
				continue
			}
			t.addHeapAncestors(entries, parent, e.Size)
		}
	}
}

func (t *SizeTree) addHeapAncestors(entries []treeSlot, idx uint32, size uint64) {
	t.ancestorGen++
	if t.ancestorGen == 0 {
		clear(t.ancestorStamp)
		t.ancestorGen = 1
	}
	gen := t.ancestorGen
	for depth := 0; depth <= 256 && int(idx) < len(entries) && entries[idx].Used; depth++ {
		if int(idx) >= len(t.ancestorStamp) || t.ancestorStamp[idx] == gen {
			return
		}
		t.ancestorStamp[idx] = gen
		entries[idx].Size = saturatingAdd(entries[idx].Size, size)
		entries[idx].files = saturatingAdd(entries[idx].files, 1)
		if idx == 0 {
			return
		}
		next := entries[idx].Parent
		if next == idx {
			return
		}
		idx = next
	}
}
func (t *SizeTree) SetDirTotals(idx uint32, size, files uint64) {
	if !t.IsDir(idx) {
		return
	}
	oldSize, oldFiles := t.SizeOf(idx), t.FileCountOf(idx)
	slot := t.slot(idx)
	slot.Size = size
	slot.files = files
	t.setSlot(idx, slot)
	if idx != 0 {
		t.ancestors(slot.Parent, func(e *treeSlot) {
			e.Size = saturatingAdd(saturatingSub(e.Size, oldSize), size)
			e.files = saturatingAdd(saturatingSub(e.files, oldFiles), files)
		})
	}
}
func (t *SizeTree) RemoveSubtree(idx uint32, size, files uint64) {
	if !t.Valid(idx) {
		return
	}
	parent := t.slot(idx).Parent
	stack := []uint32{idx}
	for len(stack) > 0 {
		cur := stack[len(stack)-1]
		stack = stack[:len(stack)-1]
		if !t.Valid(cur) {
			continue
		}
		stack = append(stack, t.children[cur]...)
		slot := t.slot(cur)
		slot.Used = false
		t.setSlot(cur, slot)
		delete(t.childNames, cur)
	}
	if idx != 0 {
		t.ancestors(parent, func(e *treeSlot) { e.Size = saturatingSub(e.Size, size); e.files = saturatingSub(e.files, files) })
	}
}
func (t *SizeTree) RemoveSubtreeInplace(idx uint32) {
	t.RemoveSubtree(idx, t.SizeOf(idx), t.FileCountOf(idx))
}
func (t *SizeTree) link(parent, idx uint32) {
	t.children[parent] = append(t.children[parent], idx)
	if m, ok := t.childNames[parent]; ok {
		key := asciiLower(t.slot(idx).Name)
		if _, found := m[key]; !found {
			m[key] = idx
		}
	}
}
func (t *SizeTree) UpsertFile(path string, size uint64) bool {
	return t.UpsertFileWithMtime(path, size, 0)
}
func (t *SizeTree) UpsertFileWithMtime(path string, size, mtime uint64) bool {
	parent, ok := t.FindNodeByPath(filepath.Dir(path))
	if !ok || !t.IsDir(parent) {
		return false
	}
	name := filepath.Base(path)
	if name == "." || name == string(filepath.Separator) {
		return false
	}
	if idx, found := t.FindNodeByPath(path); found {
		if !t.IsDir(idx) && t.EntryName(idx) == name {
			old := t.SizeOf(idx)
			slot := t.slot(idx)
			slot.Size = size
			slot.Mtime = mtime
			t.setSlot(idx, slot)
			t.ancestors(parent, func(e *treeSlot) { e.Size = saturatingAdd(saturatingSub(e.Size, old), size) })
			return true
		}
		t.RemoveSubtreeInplace(idx)
	}
	idx := uint32(t.nodeCount())
	t.entries = append(t.entries, treeSlot{TreeIndexEntry: TreeIndexEntry{parent, name, false, size, true, mtime}})
	t.children = append(t.children, nil)
	t.link(parent, idx)
	t.ancestors(parent, func(e *treeSlot) { e.Size = saturatingAdd(e.Size, size); e.files = saturatingAdd(e.files, 1) })
	return true
}
func (t *SizeTree) AppendSubtree(parent uint32, subtree *SizeTree, rootName string) {
	if !t.IsDir(parent) || subtree == nil || !subtree.Valid(0) || subtree == t {
		return
	}
	mapping := make(map[uint32]uint32)
	// Keep slot order: capped nonempty searches depend on it. Map before
	// copying so acquisition indexes may contain parents after their children.
	base := t.nodeCount()
	next := uint32(base)
	for old := 0; old < subtree.nodeCount(); old++ {
		if subtree.slot(uint32(old)).Used {
			mapping[uint32(old)] = next
			next++
		}
	}
	for old := 0; old < subtree.nodeCount(); old++ {
		e := subtree.slot(uint32(old))
		if !e.Used {
			continue
		}
		p := parent
		if old == 0 {
			e.Name = rootName
		} else {
			var ok bool
			p, ok = mapping[e.Parent]
			if !ok {
				e.Used = false
				p = parent
			}
		}
		e.Parent = p
		t.entries = append(t.entries, e)
		t.children = append(t.children, nil)
	}
	for i := base; i < t.nodeCount(); i++ {
		e := t.slot(uint32(i))
		if e.Used && t.IsDir(e.Parent) {
			t.link(e.Parent, uint32(i))
		}
	}
	t.ancestors(parent, func(e *treeSlot) {
		e.Size = saturatingAdd(e.Size, subtree.SizeOf(0))
		e.files = saturatingAdd(e.files, subtree.FileCountOf(0))
	})
	newRoot := filepath.Join(t.PathOf(parent), rootName)
	for _, blocked := range subtree.blocked {
		if pathWithin(blocked, subtree.volume.Mount) {
			rel, _ := filepath.Rel(subtree.volume.Mount, blocked)
			t.blocked = append(t.blocked, filepath.Join(newRoot, rel))
		}
	}
}
func (t *SizeTree) RebuildChildArrays() {
	t.childNames = make(map[uint32]map[string]uint32)
	n := t.nodeCount()
	// Heap trees read slots in place; slot() copies one per call.
	edge := func(i int) (uint32, bool) {
		if t.baseCount == 0 {
			return heapEdge(t.entries, i)
		}
		e := t.slot(uint32(i))
		return e.Parent, e.Used && t.IsDir(e.Parent) && e.Parent != uint32(i)
	}
	counts := make([]uint32, n)
	var total uint32
	for i := 1; i < n; i++ {
		if p, ok := edge(i); ok {
			counts[p]++
			total++
		}
	}
	// One backing array, like Rust's child_at. A slice per directory made the
	// MFT tree build allocate once for every directory on the volume.
	backing := make([]uint32, total)
	t.children = make([][]uint32, n)
	var cursor uint32
	for i, c := range counts {
		if c == 0 {
			continue
		}
		start := cursor
		cursor += c
		t.children[i] = backing[start:start:cursor]
	}
	for i := 1; i < n; i++ {
		if p, ok := edge(i); ok {
			t.children[p] = append(t.children[p], uint32(i))
		}
	}
}
func (t *SizeTree) CountUsedDirs() uint64 {
	var n uint64
	for i := 0; i < t.nodeCount(); i++ {
		e := t.slot(uint32(i))
		if e.Used && e.IsDir {
			n++
		}
	}
	return n
}
func (t *SizeTree) CountUsedFiles() uint64 {
	var n uint64
	for i := 0; i < t.nodeCount(); i++ {
		e := t.slot(uint32(i))
		if e.Used && !e.IsDir {
			n++
		}
	}
	return n
}
func (t *SizeTree) CompactEntries() []TreeIndexEntry {
	var out []TreeIndexEntry
	mapping := make(map[uint32]uint32)
	var next uint32
	for idx := 0; idx < t.nodeCount(); idx++ {
		if t.slot(uint32(idx)).Used {
			mapping[uint32(idx)] = next
			next++
		}
	}
	for idx := 0; idx < t.nodeCount(); idx++ {
		slot := t.slot(uint32(idx))
		if !slot.Used {
			continue
		}
		e := slot.TreeIndexEntry
		if idx != 0 {
			parent, ok := mapping[e.Parent]
			if ok {
				e.Parent = parent
			} else {
				e.Parent = math.MaxUint32
			}
		}
		if e.IsDir {
			e.Size = 0
		}
		out = append(out, e)
	}
	return out
}

// Blocked subtrees (including dataless cloud directories) are not covered by
// cached aggregates. Missing nodes are unknown, never evidence of absence.
func (t *SizeTree) SetBlockedSubtrees(paths []string) { t.blocked = append([]string(nil), paths...) }
func (t *SizeTree) BlockedSubtrees() []string         { return append([]string(nil), t.blocked...) }
func (t *SizeTree) CoversSubtree(path string) bool {
	if _, ok := t.FindNodeByPath(path); !ok {
		return false
	}
	for _, blocked := range t.blocked {
		if pathWithin(asciiLower(path), asciiLower(blocked)) || pathWithin(asciiLower(blocked), asciiLower(path)) {
			return false
		}
	}
	return true
}

type SubtreeFile struct {
	Idx         uint32
	Size, Mtime uint64
}

func (t *SizeTree) CollectSubtreeFiles(root uint32, maxDepth int, minSize, maxSize uint64) []SubtreeFile {
	type item struct {
		idx   uint32
		depth int
	}
	stack := []item{{root, 0}}
	var out []SubtreeFile
	seen := make(map[uint32]bool)
	for len(stack) > 0 {
		cur := stack[len(stack)-1]
		stack = stack[:len(stack)-1]
		if !t.Valid(cur.idx) || seen[cur.idx] {
			continue
		}
		seen[cur.idx] = true
		for _, c := range t.children[cur.idx] {
			if !t.Valid(c) {
				continue
			}
			e := t.slot(c)
			if e.IsDir {
				if cur.depth < maxDepth && !IsDeclutterIgnoredDirName(e.Name) {
					stack = append(stack, item{c, cur.depth + 1})
				}
			} else if e.Size >= minSize && e.Size <= maxSize {
				out = append(out, SubtreeFile{c, e.Size, e.Mtime})
			}
		}
	}
	return out
}

type sizedNode struct {
	size uint64
	idx  uint32
}
type sizeHeap []sizedNode

func (h sizeHeap) Len() int { return len(h) }
func (h sizeHeap) Less(i, j int) bool {
	if h[i].size != h[j].size {
		return h[i].size < h[j].size
	}
	return h[i].idx < h[j].idx
}
func (h sizeHeap) Swap(i, j int) { h[i], h[j] = h[j], h[i] }
func (h *sizeHeap) Push(x any)   { *h = append(*h, x.(sizedNode)) }
func (h *sizeHeap) Pop() any     { a := *h; x := a[len(a)-1]; *h = a[:len(a)-1]; return x }
func (t *SizeTree) topNodes(ctx context.Context, n int, filesOnly bool) []sizedNode {
	if n <= 0 {
		return nil
	}
	h := sizeHeap{}
	for i := 0; i < t.nodeCount(); i++ {
		e := t.slot(uint32(i))
		if i%4096 == 0 && ctx.Err() != nil {
			return nil
		}
		if !e.Used || (filesOnly && (e.IsDir || e.Size == 0)) || t.hiddenNTFSMetadata(uint32(i)) {
			continue
		}
		if len(h) == n && e.Size <= h[0].size {
			continue
		}
		heap.Push(&h, sizedNode{e.Size, uint32(i)})
		if len(h) > n {
			heap.Pop(&h)
		}
	}
	sort.Slice(h, func(i, j int) bool {
		if h[i].size != h[j].size {
			return h[i].size > h[j].size
		}
		return h[i].idx < h[j].idx
	})
	return h
}
func (t *SizeTree) LargestFiles(n int) []Node {
	var out []Node
	for _, e := range t.topNodes(context.Background(), n, true) {
		out = append(out, t.node(e.idx))
	}
	return out
}
func (t *SizeTree) Search(query string, maxResults int) []SearchHit {
	return t.SearchCancellable(context.Background(), query, maxResults)
}

// SearchCancellable returns nil on cancellation. Nonempty searches select the
// first N matching slots, then sort them by size, matching the original contract.
func (t *SizeTree) SearchCancellable(ctx context.Context, query string, maxResults int) []SearchHit {
	if maxResults <= 0 {
		return nil
	}
	p := ParseNamePattern(query)
	var candidates []sizedNode
	if p.Empty() {
		candidates = t.topNodes(ctx, maxResults, false)
	} else {
		for i := 0; i < t.nodeCount(); i++ {
			e := t.slot(uint32(i))
			if i%4096 == 0 && ctx.Err() != nil {
				return nil
			}
			if e.Used && !t.hiddenNTFSMetadata(uint32(i)) && p.MatchesRaw(e.Name) {
				candidates = append(candidates, sizedNode{e.Size, uint32(i)})
				if len(candidates) >= maxResults {
					break
				}
			}
		}
		sort.SliceStable(candidates, func(i, j int) bool { return candidates[i].size > candidates[j].size })
	}
	var out []SearchHit
	cache := make(map[uint32]string)
	for _, c := range candidates {
		e := t.slot(c.idx)
		path := t.PathOfWith(c.idx, cache)
		if path != "" {
			out = append(out, SearchHit{path, e.Name, e.IsDir, e.Size, e.Mtime})
		}
	}
	if ctx.Err() != nil {
		return nil
	}
	return out
}

// ScanResult carries acquisition counters without coupling the domain to MFT,
// APFS or a live-walk scanner.
type ScanResult struct {
	Volume                                                        VolumeID
	TotalSize, FileCount, DirCount                                uint64
	Dirs                                                          []DirUsage
	Tree                                                          *SizeTree
	ElapsedMS, RecordsRead, RecordsExpected, MFTRunBytes          uint64
	ExtRecords, ExtDataMerged, HardLinks, UniqueSize, UniqueFiles uint64
	// Spill carries a scan that crossed the macOS spill threshold. Tree is nil
	// until BuildMacWalkSpillIndex streams it; the fields are scan metadata, not
	// part of a persisted result.
	Spill        *platform.MacWalkSpill
	SpillRoot    string
	SpillBlocked []string
}

func (s *ScanResult) RemovePath(path string) {
	if s.Tree == nil {
		return
	}
	idx, ok := s.Tree.FindNodeByPath(path)
	if !ok {
		return
	}
	size, files := s.Tree.SizeOf(idx), s.Tree.FileCountOf(idx)
	s.Tree.RemoveSubtree(idx, size, files)
	s.TotalSize = saturatingSub(s.TotalSize, size)
	s.FileCount = saturatingSub(s.FileCount, files)
}
