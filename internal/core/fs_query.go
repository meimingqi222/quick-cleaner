package core

import (
	"container/heap"
	"context"
	"io/fs"
	"math"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"sort"
	"strings"
)

func IsIgnoredDirName(name string) bool {
	if strings.HasPrefix(name, ".") {
		return true
	}
	switch strings.ToLower(name) {
	case "node_modules", "library", "appdata", "application data", "application support", "local settings", "target", "dist", "build", "out", "bin", "obj", "pkg", "vendor", "pods", "deriveddata", "bower_components", "venv", "env", "__pycache__", "cache", "caches", "temp", "tmp", "logs", "gems", "site-packages", "docs", "doc", "documentation", "manual", "sdk", "javadoc", "$recycle.bin", "system volume information":
		return true
	}
	return false
}
func IsDeclutterIgnoredDirName(name string) bool { return IsIgnoredDirName(name) }

// Pass the bundled loader's RuleList here; keeping it explicit avoids a second
// photo table and permits use before the rule-loader migration is linked.
func IsPhotoIgnoredDirName(name string, ruleList func(string, string) []string) bool {
	if IsIgnoredDirName(name) {
		return true
	}
	if ruleList != nil {
		for _, entry := range ruleList("engine", "photo_ignored_directories") {
			if asciiLower(entry) == asciiLower(name) {
				return true
			}
		}
	}
	return false
}

type QueryFilter struct {
	Roots            []string
	MaxDepth         int
	MinSize, MaxSize uint64
	Extensions       map[string]struct{}
	IncludeDirs      bool
}

func NewQueryFilter(roots []string) QueryFilter {
	return QueryFilter{Roots: append([]string(nil), roots...), MaxDepth: math.MaxInt, MaxSize: math.MaxUint64}
}
func (f QueryFilter) WithMaxDepth(depth int) QueryFilter { f.MaxDepth = depth; return f }
func (f QueryFilter) WithSizeRange(min, max uint64) QueryFilter {
	f.MinSize = min
	f.MaxSize = max
	return f
}
func (f QueryFilter) WithExtensions(exts []string) QueryFilter {
	f.Extensions = make(map[string]struct{}, len(exts))
	for _, ext := range exts {
		f.Extensions[strings.ToLower(ext)] = struct{}{}
	}
	return f
}
func (f QueryFilter) WithIncludeDirs(include bool) QueryFilter { f.IncludeDirs = include; return f }
func (f QueryFilter) MatchesFile(path string, size uint64) bool {
	if size < f.MinSize || size > f.MaxSize {
		return false
	}
	if f.Extensions != nil {
		ext := filepath.Ext(path)
		if strings.HasPrefix(filepath.Base(path), ".") && ext == filepath.Base(path) {
			ext = ""
		}
		_, ok := f.Extensions[strings.ToLower(strings.TrimPrefix(ext, "."))]
		return ok
	}
	return true
}

type IndexedFile struct {
	Path        string
	Size, Mtime uint64
	IsDir       bool
}
type PathMeasurement struct{ Size, Files, NewestMtime uint64 }
type DuplicateCandidate struct {
	Path  string
	Mtime uint64
}
type FileIndexQuery interface {
	QueryFiles(context.Context, QueryFilter) []IndexedFile
	MeasurePath(context.Context, string) (PathMeasurement, bool)
	QueryLargeFiles(context.Context, []string, uint64, int) []IndexedFile
	QueryDuplicateBuckets(context.Context, []string, uint64) map[uint64][]DuplicateCandidate
}

// FSIndexEngine uses an optional index. Incomplete/missing roots fall back
// individually; a nonempty result from one root never suppresses another root.
type FSIndexEngine struct {
	tree  *SizeTree
	slots chan struct{}
}

func NewFSIndexEngine(tree *SizeTree) *FSIndexEngine {
	return &FSIndexEngine{tree: tree, slots: newMeasurementSlots()}
}

var _ FileIndexQuery = (*FSIndexEngine)(nil)

func fileMtime(info fs.FileInfo) uint64 {
	sec := info.ModTime().Unix()
	if sec < 0 {
		return 0
	}
	return uint64(sec)
}
func (e *FSIndexEngine) QueryFiles(ctx context.Context, f QueryFilter) []IndexedFile {
	var out []IndexedFile
	for _, root := range f.Roots {
		if ctx.Err() != nil {
			break
		}
		root = filepath.Clean(root)
		// include_dirs is a live-walk operation in the source engine.
		if e.tree != nil && !f.IncludeDirs && e.tree.CoversSubtree(root) {
			idx, _ := e.tree.FindNodeByPath(root)
			if e.tree.IsDir(idx) {
				before := len(out)
				cache := make(map[uint32]string)
				for _, item := range e.tree.CollectSubtreeFiles(idx, f.MaxDepth, f.MinSize, f.MaxSize) {
					if ctx.Err() != nil {
						break
					}
					path := e.tree.PathOfWith(item.Idx, cache)
					if path == "" || IsProtected(path) || !f.MatchesFile(path, item.Size) {
						continue
					}
					mtime := item.Mtime
					if mtime == 0 {
						if info, err := os.Lstat(path); err == nil {
							mtime = fileMtime(info)
						}
					}
					out = append(out, IndexedFile{path, item.Size, mtime, false})
				}
				if len(out) > before {
					continue
				}
			}
		}
		_ = filepath.WalkDir(root, func(path string, d fs.DirEntry, err error) error {
			if ctx.Err() != nil {
				return ctx.Err()
			}
			if err != nil {
				return nil
			}
			if IsIgnoredDirName(d.Name()) {
				if d.IsDir() {
					return filepath.SkipDir
				}
				return nil
			}
			rel, _ := filepath.Rel(root, path)
			depth := 0
			if rel != "." {
				depth = len(strings.Split(rel, string(filepath.Separator)))
			}
			if depth > f.MaxDepth {
				if d.IsDir() {
					return filepath.SkipDir
				}
				return nil
			}
			if d.Type()&os.ModeSymlink != 0 {
				return nil
			}
			if d.IsDir() && !f.IncludeDirs {
				return nil
			}
			if IsProtected(path) {
				return nil
			}
			info, err := d.Info()
			if err != nil || (!info.IsDir() && !info.Mode().IsRegular()) {
				return nil
			}
			size := uint64(info.Size())
			if f.MatchesFile(path, size) {
				out = append(out, IndexedFile{path, size, fileMtime(info), info.IsDir()})
			}
			return nil
		})
	}
	return out
}

// allocatedBytes reads the portable Unix stat Blocks field without binding the
// domain package to a platform syscall struct. Windows acquisition can provide
// its own allocated-size measurement later; absent Blocks uses logical size.
func allocatedBytes(info fs.FileInfo) uint64 {
	v := reflect.ValueOf(info.Sys())
	if v.IsValid() && v.Kind() == reflect.Pointer && !v.IsNil() {
		v = v.Elem()
		if v.Kind() == reflect.Struct {
			b := v.FieldByName("Blocks")
			if b.IsValid() && b.CanInt() && b.Int() >= 0 {
				return uint64(b.Int()) * 512
			}
		}
	}
	if info.Size() < 0 {
		return 0
	}
	return uint64(info.Size())
}
func (e *FSIndexEngine) MeasurePath(ctx context.Context, path string) (PathMeasurement, bool) {
	if ctx.Err() != nil {
		return PathMeasurement{}, false
	}
	if e.tree != nil && e.tree.CoversSubtree(path) {
		idx, _ := e.tree.FindNodeByPath(path)
		return PathMeasurement{Size: e.tree.SizeOf(idx), Files: e.tree.FileCountOf(idx)}, true
	}
	slots := e.slots
	if slots == nil {
		slots = newMeasurementSlots()
	}
	return measurePathShared(ctx, path, slots)
}

func newMeasurementSlots() chan struct{} { return make(chan struct{}, max(1, runtime.GOMAXPROCS(0)-1)) }

// measurePathShared lets concurrent callers share one fan-out budget, so N
// parallel measurements do not each spawn GOMAXPROCS directory goroutines.
func measurePathShared(ctx context.Context, path string, slots chan struct{}) (PathMeasurement, bool) {
	info, err := os.Lstat(path)
	if err != nil || info.Mode()&os.ModeSymlink != 0 || (!info.IsDir() && !info.Mode().IsRegular()) {
		return PathMeasurement{}, false
	}
	if info.Mode().IsRegular() {
		return PathMeasurement{allocatedBytes(info), 1, fileMtime(info)}, true
	}
	result := measureDirectory(ctx, path, 0, slots)
	return result, ctx.Err() == nil
}
func mergeMeasurement(a, b PathMeasurement) PathMeasurement {
	return PathMeasurement{saturatingAdd(a.Size, b.Size), saturatingAdd(a.Files, b.Files), max(a.NewestMtime, b.NewestMtime)}
}

// Only the first two levels fan out, using a shared nonblocking semaphore so
// deep/wide dependency trees cannot create an unbounded goroutine population.
func measureDirectory(ctx context.Context, path string, depth int, slots chan struct{}) PathMeasurement {
	var result PathMeasurement
	if ctx.Err() != nil {
		return result
	}
	entries, _ := os.ReadDir(path)
	completed := make(chan PathMeasurement, cap(slots))
	pending := 0
	for _, d := range entries {
		if ctx.Err() != nil {
			break
		}
		if d.Type()&os.ModeSymlink != 0 {
			continue
		}
		if d.IsDir() {
			child := filepath.Join(path, d.Name())
			if depth < 2 {
				select {
				case slots <- struct{}{}:
					pending++
					go func() { defer func() { <-slots }(); completed <- measureDirectory(ctx, child, depth+1, slots) }()
					continue
				default:
				}
			}
			result = mergeMeasurement(result, measureDirectory(ctx, child, depth+1, slots))
			continue
		}
		if asciiLower(d.Name()) == "desktop.ini" {
			continue
		}
		info, err := d.Info()
		if err != nil || !info.Mode().IsRegular() {
			continue
		}
		result = mergeMeasurement(result, PathMeasurement{allocatedBytes(info), 1, fileMtime(info)})
	}
	for range pending {
		result = mergeMeasurement(result, <-completed)
	}
	return result
}
func (e *FSIndexEngine) QueryLargeFiles(ctx context.Context, roots []string, minSize uint64, limit int) []IndexedFile {
	if limit <= 0 {
		return nil
	}
	files := e.QueryFiles(ctx, NewQueryFilter(roots).WithMaxDepth(20).WithSizeRange(minSize, math.MaxUint64))
	selected := sizeHeap{}
	for i, file := range files {
		if ctx.Err() != nil {
			break
		}
		if len(selected) == limit && file.Size <= selected[0].size {
			continue
		}
		heap.Push(&selected, sizedNode{file.Size, uint32(i)})
		if len(selected) > limit {
			heap.Pop(&selected)
		}
	}
	sort.Slice(selected, func(i, j int) bool {
		if selected[i].size != selected[j].size {
			return selected[i].size > selected[j].size
		}
		return selected[i].idx < selected[j].idx
	})
	out := make([]IndexedFile, 0, len(selected))
	for _, item := range selected {
		out = append(out, files[item.idx])
	}
	return out
}
func (e *FSIndexEngine) QueryDuplicateBuckets(ctx context.Context, roots []string, minSize uint64) map[uint64][]DuplicateCandidate {
	files := e.QueryFiles(ctx, NewQueryFilter(roots).WithMaxDepth(20).WithSizeRange(minSize, math.MaxUint64))
	buckets := make(map[uint64][]DuplicateCandidate)
	for _, f := range files {
		if ctx.Err() != nil {
			break
		}
		buckets[f.Size] = append(buckets[f.Size], DuplicateCandidate{f.Path, f.Mtime})
	}
	for size, items := range buckets {
		if len(items) < 2 {
			delete(buckets, size)
		}
	}
	return buckets
}
