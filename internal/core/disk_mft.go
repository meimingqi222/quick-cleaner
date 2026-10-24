package core

import (
	"context"
	"math"
	"sort"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// BuildMFTScanResult adapts raw record numbers to the shared root-zero tree.
// Extra hard-link parents contribute directory totals, not invented filenames:
// MFT's selected FILE_NAME alone supplies a navigable/deletable path.
func BuildMFTScanResult(ctx context.Context, raw platform.MFTScanResult, topN int) (ScanResult, error) {
	if err := ctx.Err(); err != nil {
		return ScanResult{}, err
	}
	root := raw.RootRecord
	if root != platform.MFTRootRecord || root >= uint64(len(raw.Entries)) || len(raw.Entries) > math.MaxUint32 || !raw.Entries[root].Used || !raw.Entries[root].IsDir {
		return ScanResult{}, platform.ErrMFTCorrupt
	}
	mapRecord := func(record uint64) uint32 {
		if record >= uint64(len(raw.Entries)) {
			return math.MaxUint32
		}
		if record == root {
			return 0
		}
		if record == 0 {
			return uint32(root)
		}
		return uint32(record)
	}
	result := ScanResult{
		Volume: raw.Volume, RecordsRead: uint64(len(raw.Entries)),
		RecordsExpected: raw.RecordsExpected, MFTRunBytes: raw.MFTRunBytes, ExtRecords: raw.ExtRecords,
	}
	sizes := make([]uint64, len(raw.Entries))
	for i, entry := range raw.Entries {
		sizes[i] = entry.Size
	}
	for i, entry := range raw.Entries {
		if i%4096 == 0 && ctx.Err() != nil {
			return ScanResult{}, ctx.Err()
		}
		if entry.BaseRecord == 0 || entry.Size == 0 || entry.BaseRecord >= uint64(len(raw.Entries)) {
			continue
		}
		base := raw.Entries[entry.BaseRecord]
		if base.Used && !base.IsDir && base.BaseRecord == 0 {
			sizes[entry.BaseRecord] = saturatingAdd(sizes[entry.BaseRecord], entry.Size)
			result.ExtDataMerged++
		}
	}
	entries := make([]TreeIndexEntry, len(raw.Entries))
	for i, entry := range raw.Entries {
		used := entry.Used && entry.BaseRecord == 0
		entries[mapRecord(uint64(i))] = TreeIndexEntry{
			Parent: mapRecord(entry.Parent), Name: entry.Name, IsDir: entry.IsDir,
			Size: sizes[i], Used: used, Mtime: entry.Mtime,
		}
		if !used {
			continue
		}
		if entry.IsDir {
			result.DirCount++
		} else {
			result.UniqueSize = saturatingAdd(result.UniqueSize, sizes[i])
			result.UniqueFiles++
		}
	}
	entries[0].Parent = 0
	tree := NewSizeTree(raw.Volume, entries)
	tree.ntfs = true
	result.Tree, result.TotalSize, result.FileCount = tree, result.UniqueSize, result.UniqueFiles
	seen := make(map[[2]uint64]struct{})
	for i, entry := range raw.Entries {
		if i%4096 == 0 && ctx.Err() != nil {
			return ScanResult{}, ctx.Err()
		}
		record := uint64(i)
		if entry.BaseRecord != 0 {
			record = entry.BaseRecord
		}
		if record >= uint64(len(raw.Entries)) {
			continue
		}
		base := raw.Entries[record]
		if !base.Used || base.IsDir || base.BaseRecord != 0 {
			continue
		}
		for _, parent := range entry.LinkParents {
			key := [2]uint64{record, parent}
			if parent == base.Parent {
				continue
			}
			if _, found := seen[key]; found {
				continue
			}
			seen[key] = struct{}{}
			result.HardLinks++
			result.TotalSize = saturatingAdd(result.TotalSize, sizes[record])
			result.FileCount++
			mapped := mapRecord(parent)
			tree.NoteExtraParent(mapRecord(record), mapped)
			tree.ancestors(mapped, func(slot *treeSlot) {
				slot.Size = saturatingAdd(slot.Size, sizes[record])
				slot.files++
			})
		}
	}
	if topN > 0 {
		var ranked []uint32
		for i := 0; i < tree.nodeCount(); i++ {
			if tree.IsDir(uint32(i)) && tree.SizeOf(uint32(i)) > 0 {
				ranked = append(ranked, uint32(i))
			}
		}
		sort.Slice(ranked, func(i, j int) bool {
			if tree.SizeOf(ranked[i]) != tree.SizeOf(ranked[j]) {
				return tree.SizeOf(ranked[i]) > tree.SizeOf(ranked[j])
			}
			return ranked[i] < ranked[j]
		})
		cache := make(map[uint32]string)
		for _, idx := range ranked[:min(topN, len(ranked))] {
			result.Dirs = append(result.Dirs, DirUsage{tree.PathOfWith(idx, cache), tree.SizeOf(idx), tree.FileCountOf(idx)})
		}
	}
	if err := ctx.Err(); err != nil {
		return ScanResult{}, err
	}
	return result, nil
}
