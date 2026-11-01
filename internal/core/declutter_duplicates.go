package core

import (
	"context"
	"crypto/sha256"
	"encoding/binary"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"sort"
	"sync"
)

type DuplicateFileItem struct {
	PathDisplay, Path, ModifiedAtStr string
	ModifiedAtSecs                   uint64
	IsOriginal, Selected             bool
}

type DuplicateGroup struct {
	ID, Filename string
	Category     Text
	SizePerCopy  uint64
	Files        []DuplicateFileItem
}

func (g DuplicateGroup) CleanableSize() uint64 {
	var bytes uint64
	for _, file := range g.Files {
		if file.Selected {
			bytes = saturatingAdd(bytes, g.SizePerCopy)
		}
	}
	return bytes
}

func ScanDuplicateFiles(ctx context.Context, s *RuleSnapshot, roots []string, query FileIndexQuery) []DuplicateGroup {
	if query == nil {
		query = NewFSIndexEngine(nil)
	}
	buckets := query.QueryDuplicateBuckets(ctx, roots, s.Number("engine", "duplicate_min_size", 65536))
	var sizes []uint64
	for size, paths := range buckets {
		if len(paths) > 1 {
			sizes = append(sizes, size)
		}
	}
	sort.Slice(sizes, func(i, j int) bool {
		return sizes[i]*uint64(len(buckets[sizes[i]])-1) > sizes[j]*uint64(len(buckets[sizes[j]])-1)
	})
	sizes = sizes[:min(len(sizes), int(s.Number("engine", "duplicate_bucket_limit", 500)))]
	results := make([][]DuplicateGroup, len(sizes))
	work := make(chan int)
	var workers sync.WaitGroup
	for range min(len(sizes), max(1, runtime.GOMAXPROCS(0))) {
		workers.Go(func() {
			for index := range work {
				if ctx.Err() != nil {
					continue
				}
				size := sizes[index]
				groups := map[[2]uint64][]DuplicateCandidate{}
				for _, file := range buckets[size] {
					hashes, err := duplicateHashes(ctx, file.Path, size)
					if err == nil {
						groups[hashes] = append(groups[hashes], file)
					}
				}
				for hashes, candidates := range groups {
					if len(candidates) < 2 || ctx.Err() != nil {
						continue
					}
					// Above 2 MiB the hashes only sample the file: confirm byte
					// equality before anything is pre-selected for removal.
					sets := [][]DuplicateCandidate{candidates}
					if size > duplicateSampledAbove {
						sets = verifyDuplicateContent(ctx, candidates)
					}
					for set, paths := range sets {
						sort.SliceStable(paths, func(i, j int) bool { return paths[i].Mtime < paths[j].Mtime })
						kind := NewText("其它文件", "Others")
						for _, row := range []struct{ policy, zh, en string }{{"duplicate_image", "图像照片", "Images"}, {"duplicate_video", "视频影音", "Videos"}, {"duplicate_archive", "归档文件", "Archives"}, {"duplicate_document", "文本文档", "Documents"}, {"duplicate_audio", "音频音乐", "Audio"}} {
							if declutterExtension(s, row.policy, paths[0].Path) {
								kind = NewText(row.zh, row.en)
								break
							}
						}
						id := fmt.Sprintf("dup-%d-%d", size, hashes[1])
						if len(sets) > 1 {
							id = fmt.Sprintf("%s-%d", id, set) // one sampled match split into several real ones
						}
						group := DuplicateGroup{ID: id, Filename: filepath.Base(paths[0].Path), Category: kind, SizePerCopy: size}
						for i, file := range paths {
							group.Files = append(group.Files, DuplicateFileItem{Path: file.Path, PathDisplay: file.Path, ModifiedAtSecs: file.Mtime, ModifiedAtStr: formatTimestampDate(file.Mtime), IsOriginal: i == 0, Selected: i != 0})
						}
						results[index] = append(results[index], group)
					}
				}
			}
		})
	}
	for index := range sizes {
		if ctx.Err() != nil {
			break
		}
		work <- index
	}
	close(work)
	workers.Wait()
	var out []DuplicateGroup
	for _, groups := range results {
		out = append(out, groups...)
	}
	sort.SliceStable(out, func(i, j int) bool { return out[i].CleanableSize() > out[j].CleanableSize() })
	return out
}

// duplicateSampledAbove is where duplicateHashes stops hashing whole files.
const duplicateSampledAbove = 2 * 1024 * 1024

// verifyDuplicateContent splits sampled-hash candidates into sets whose full
// contents are identical, keeping only sets of two or more. A file that cannot
// be read completely is left out: it cannot be proven a duplicate.
func verifyDuplicateContent(ctx context.Context, candidates []DuplicateCandidate) [][]DuplicateCandidate {
	index := map[[sha256.Size]byte]int{}
	var sets [][]DuplicateCandidate
	for _, file := range candidates {
		sum, err := duplicateContentDigest(ctx, file.Path)
		if err != nil {
			continue
		}
		i, ok := index[sum]
		if !ok {
			i = len(sets)
			index[sum] = i
			sets = append(sets, nil)
		}
		sets[i] = append(sets[i], file)
	}
	return slices.DeleteFunc(sets, func(set []DuplicateCandidate) bool { return len(set) < 2 })
}

func duplicateContentDigest(ctx context.Context, path string) ([sha256.Size]byte, error) {
	var sum [sha256.Size]byte
	file, err := os.Open(path)
	if err != nil {
		return sum, err
	}
	defer file.Close()
	h := sha256.New()
	buf := make([]byte, 1<<20)
	for {
		if err := ctx.Err(); err != nil {
			return sum, err
		}
		n, err := file.Read(buf)
		h.Write(buf[:n])
		if err == io.EOF {
			break
		}
		if err != nil {
			return sum, err
		}
	}
	copy(sum[:], h.Sum(nil))
	return sum, nil
}

// Match the reference's two hashes and one-open policy. Above 2 MiB the
// second hash samples five 16 KiB regions; it is not a byte-equality proof,
// so ScanDuplicateFiles confirms those candidates with verifyDuplicateContent.
func duplicateHashes(ctx context.Context, path string, size uint64) ([2]uint64, error) {
	if err := ctx.Err(); err != nil {
		return [2]uint64{}, err
	}
	file, err := os.Open(path)
	if err != nil {
		return [2]uint64{}, err
	}
	defer file.Close()
	sample, full := newRustHasher(), newRustHasher()
	var length [8]byte
	binary.LittleEndian.PutUint64(length[:], size)
	sample.Write(length[:])
	full.Write(length[:])
	buf := make([]byte, 65536)
	readAt := func(h *rustHasher, offset int64, count int) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		if _, err := file.Seek(offset, io.SeekStart); err != nil {
			return err
		}
		n, err := file.Read(buf[:count])
		if err != nil && err != io.EOF {
			return err
		}
		h.Write(buf[:n])
		return nil
	}
	if err := readAt(&sample, 0, 4096); err != nil {
		return [2]uint64{}, err
	}
	if size > 8192 {
		if err := readAt(&sample, int64(size)-4096, 4096); err != nil {
			return [2]uint64{}, err
		}
	}
	if size > duplicateSampledAbove {
		for _, offset := range []uint64{0, size / 4, size / 2, size * 3 / 4, saturatingSub(size, 16384)} {
			if err := readAt(&full, int64(offset), 16384); err != nil {
				return [2]uint64{}, err
			}
		}
	} else {
		if _, err := file.Seek(0, io.SeekStart); err != nil {
			return [2]uint64{}, err
		}
		for {
			if err := ctx.Err(); err != nil {
				return [2]uint64{}, err
			}
			n, err := file.Read(buf)
			full.Write(buf[:n])
			if err == io.EOF {
				break
			}
			if err != nil {
				return [2]uint64{}, err
			}
		}
	}
	return [2]uint64{sample.Sum64(), full.Sum64()}, nil
}
