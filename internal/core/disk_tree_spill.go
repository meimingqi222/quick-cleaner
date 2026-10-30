package core

import (
	"context"
	"io"
	"math"
	"os"
	"slices"
	"unicode/utf8"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// The caller owns a private refresh candidate and abandons it on failure.
// Spill parents precede children, so slots need only an offset, not a remap
// table or a second materialized subtree beside the finished candidate.
func (t *SizeTree) appendSpillSubtree(ctx context.Context, parent uint32, rootName string, scan *ScanResult) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if scan == nil || scan.Spill == nil || !t.IsDir(parent) || len(rootName) > math.MaxUint16 || !utf8.ValidString(rootName) || !validDiskIndexName(rootName) {
		return platform.ErrMacWalkMalformed
	}
	count := scan.Spill.Count()
	if count == 0 || count > uint64(cacheParentMask) || uint64(t.nodeCount())+count+1 > uint64(cacheParentMask)+1 {
		return platform.ErrMacWalkMalformed
	}
	file, err := os.Open(scan.Spill.Path())
	if err != nil {
		return err
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return err
	}
	// Every valid record has a 19-byte header and a nonempty name. Bound the
	// reservation by the actual file before trusting its in-memory count.
	if !info.Mode().IsRegular() || info.Size() < 0 || count > uint64(info.Size())/20 {
		return platform.ErrMacWalkMalformed
	}
	first, base := len(t.entries), uint32(t.nodeCount())
	t.entries = slices.Grow(t.entries, int(count)+1)
	t.entries = append(t.entries, treeSlot{TreeIndexEntry: TreeIndexEntry{Parent: parent, Name: rootName, Used: true, IsDir: true}})
	reader := newSpillRecordReader(file)
	for index := uint64(1); ; index++ {
		if index%4096 == 1 {
			if err := ctx.Err(); err != nil {
				return err
			}
		}
		entry, err := reader.next(true)
		if err == io.EOF {
			if index != count+1 {
				return platform.ErrMacWalkMalformed
			}
			break
		}
		if err != nil {
			return err
		}
		if index > count || uint64(entry.Parent) >= index || !t.entries[first+int(entry.Parent)].IsDir || !utf8.Valid(entry.Name) || !validDiskIndexName(entry.Name) {
			return platform.ErrMacWalkMalformed
		}
		slot := treeSlot{TreeIndexEntry: TreeIndexEntry{Parent: base + entry.Parent, Name: string(entry.Name), Used: true, IsDir: entry.IsDir, Mtime: uint64(entry.Mtime)}}
		if !entry.IsDir {
			slot.Size = entry.Size
		}
		t.entries = append(t.entries, slot)
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	// Match spill base construction: aggregate each complete child once,
	// preserving unsigned size addition rather than walking ancestors.
	for i := len(t.entries) - 1; i > first; i-- {
		if i%4096 == 0 {
			if err := ctx.Err(); err != nil {
				return err
			}
		}
		child := &t.entries[i]
		p := &t.entries[first+int(child.Parent-base)]
		p.Size += child.Size
		if child.IsDir {
			p.files += child.files
		} else {
			p.files++
		}
	}
	if t.base == nil {
		t.children = append(t.children, make([][]uint32, int(count)+1)...)
	}
	for i := first; i < len(t.entries); i++ {
		if i%4096 == 0 {
			if err := ctx.Err(); err != nil {
				return err
			}
		}
		t.link(t.entries[i].Parent, uint32(t.baseCount+i))
	}
	size, files := t.entries[first].Size, t.entries[first].files
	t.ancestors(parent, func(e *treeSlot) {
		e.Size = saturatingAdd(e.Size, size)
		e.files = saturatingAdd(e.files, files)
	})
	t.appendSubtreeCoverage(parent, rootName, scan.Volume, scan.SpillBlocked)
	return nil
}
