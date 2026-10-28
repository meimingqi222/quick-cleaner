package core

import (
	"context"
	"fmt"
	"image"
	"image/color"
	_ "image/jpeg"
	_ "image/png"
	"math"
	"math/bits"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
	"sync"

	_ "golang.org/x/image/webp"
)

type PhotoItem struct {
	ID, Filename, Path, PathDisplay string
	Dimensions                      [2]uint32
	Size                            uint64
	IsBestShot, Selected            bool
	DateStr                         string
	BGGradientSeed                  uint32
}

type PhotoGroup struct {
	IndexStr string
	Title    Text
	Photos   []PhotoItem
}

func (g PhotoGroup) CleanableSize() uint64 {
	var bytes uint64
	for _, photo := range g.Photos {
		if photo.Selected {
			bytes = saturatingAdd(bytes, photo.Size)
		}
	}
	return bytes
}

func (g PhotoGroup) SelectedCount() int {
	n := 0
	for _, photo := range g.Photos {
		if photo.Selected {
			n++
		}
	}
	return n
}

type photoCandidate struct {
	IndexedFile
	dimensions [2]uint32
	hash       *[2]uint64
	stem       string
	parent     string
}

func ScanSimilarPhotos(ctx context.Context, s *RuleSnapshot, roots []string, query FileIndexQuery) []PhotoGroup {
	if query == nil {
		query = NewFSIndexEngine(nil)
	}
	files := query.QueryFiles(ctx, NewQueryFilter(roots).WithMaxDepth(int(s.Number("engine", "photo_max_depth", 20))).WithSizeRange(s.Number("engine", "photo_min_size", 10000), s.Number("engine", "photo_max_size", 200000000)))
	dirs := map[string][]IndexedFile{}
	var order []string
	var all []IndexedFile
	for _, file := range files {
		if !declutterExtension(s, "photo_extensions", file.Path) {
			continue
		}
		ignored := false
		for _, part := range strings.Split(filepath.ToSlash(file.Path), "/") {
			ignored = ignored || IsPhotoIgnoredDirName(part, s.List)
		}
		if ignored {
			continue
		}
		parent := filepath.Dir(file.Path)
		if _, exists := dirs[parent]; !exists {
			order = append(order, parent)
		}
		dirs[parent] = append(dirs[parent], file)
		all = append(all, file)
	}
	seen := map[string]bool{}
	var selected []IndexedFile
	push := func(file IndexedFile) {
		if !seen[file.Path] {
			seen[file.Path] = true
			selected = append(selected, file)
		}
	}
	for _, dir := range order {
		if len(dirs[dir]) >= 2 {
			for _, file := range dirs[dir] {
				push(file)
			}
		}
	}
	limit := int(s.Number("engine", "photo_candidate_limit", 1500))
	if len(selected) < int(s.Number("engine", "photo_supplement_below", 500)) {
		for _, file := range all {
			push(file)
			if len(selected) >= limit {
				break
			}
		}
	}
	selected = selected[:min(len(selected), limit)]
	if len(selected) < 2 {
		return nil
	}
	candidates := make([]*photoCandidate, len(selected))
	work := make(chan int)
	var workers sync.WaitGroup
	for range min(len(selected), max(1, runtime.GOMAXPROCS(0))) {
		workers.Go(func() {
			for i := range work {
				if ctx.Err() != nil {
					continue
				}
				file := selected[i]
				c := photoCandidate{IndexedFile: file, parent: filepath.Dir(file.Path), stem: strings.ToLower(strings.TrimSuffix(filepath.Base(file.Path), filepath.Ext(file.Path)))}
				c.dimensions = estimatePhotoDimensions(file.Size)
				if f, err := os.Open(file.Path); err == nil {
					img, _, err := image.Decode(f)
					f.Close()
					if err == nil {
						c.dimensions = [2]uint32{uint32(img.Bounds().Dx()), uint32(img.Bounds().Dy())}
						hash := photoDualHash(img)
						c.hash = &hash
					}
				}
				candidates[i] = &c
			}
		})
	}
	for i := range selected {
		if ctx.Err() != nil {
			break
		}
		work <- i
	}
	close(work)
	workers.Wait()
	var groups [][]photoCandidate
	for _, candidate := range candidates {
		if candidate == nil || ctx.Err() != nil {
			continue
		}
		match := -1
		for i, group := range groups {
			if len(group) >= int(s.Number("engine", "photo_group_size_limit", 12)) {
				continue
			}
			fits := true
			for _, member := range group {
				if !photosSimilar(s, member, *candidate) {
					fits = false
					break
				}
			}
			if fits {
				match = i
				break
			}
		}
		if match < 0 {
			groups = append(groups, []photoCandidate{*candidate})
		} else {
			groups[match] = append(groups[match], *candidate)
		}
	}
	benefit := func(group []photoCandidate) uint64 {
		var total, largest uint64
		for _, c := range group {
			total = saturatingAdd(total, c.Size)
			largest = max(largest, c.Size)
		}
		return saturatingSub(total, largest)
	}
	sort.SliceStable(groups, func(i, j int) bool { return benefit(groups[i]) > benefit(groups[j]) })
	var out []PhotoGroup
	for _, group := range groups {
		if len(group) < 2 {
			continue
		}
		sort.SliceStable(group, func(i, j int) bool {
			if group[i].Mtime != group[j].Mtime {
				return group[i].Mtime < group[j].Mtime
			}
			return group[i].Path < group[j].Path
		})
		best := 0
		pixels := func(c photoCandidate) uint64 { return uint64(c.dimensions[0]) * uint64(c.dimensions[1]) }
		for i, c := range group {
			if pixels(c) > pixels(group[best]) || pixels(c) == pixels(group[best]) && c.Size > group[best].Size {
				best = i
			}
		}
		stem := strings.TrimSuffix(filepath.Base(group[0].Path), filepath.Ext(group[0].Path))
		item := PhotoGroup{IndexStr: fmt.Sprintf("%02d", len(out)+1), Title: NewText("连拍/相似组: "+stem, "Series: "+stem)}
		for i, c := range group {
			h := newRustHasher()
			h.Write([]byte(c.Path))
			palettes := [...]uint32{0x0284c7, 0x0369a1, 0x075985, 0x475569, 0x334155, 0x4f46e5, 0x4338ca, 0x0d9488, 0x0f766e}
			item.Photos = append(item.Photos, PhotoItem{ID: fmt.Sprintf("photo-%d-%d", len(out), i), Filename: filepath.Base(c.Path), Path: c.Path, PathDisplay: c.Path, Dimensions: c.dimensions, Size: c.Size, IsBestShot: i == best, Selected: i != best, DateStr: formatTimestampDate(c.Mtime), BGGradientSeed: palettes[h.Sum64()%uint64(len(palettes))]})
		}
		out = append(out, item)
	}
	return out[:min(len(out), int(s.Number("engine", "photo_group_limit", 30)))]
}

func estimatePhotoDimensions(size uint64) [2]uint32 {
	switch {
	case size > 5000000:
		return [2]uint32{4032, 3024}
	case size > 2000000:
		return [2]uint32{3000, 2000}
	case size > 800000:
		return [2]uint32{2048, 1536}
	default:
		return [2]uint32{1920, 1080}
	}
}

func photosSimilar(s *RuleSnapshot, a, b photoCandidate) bool {
	aspect := a.dimensions[1] > 0 && b.dimensions[1] > 0 && float32(math.Abs(float64(float32(a.dimensions[0])/float32(a.dimensions[1])-float32(b.dimensions[0])/float32(b.dimensions[1])))) < float32(0.08)
	if !aspect {
		return false
	}
	sameDir := a.parent == b.parent
	burst := false
	for _, prefix := range s.List("engine", "photo_camera_prefixes") {
		burst = burst || strings.HasPrefix(a.stem, prefix) && strings.HasPrefix(b.stem, prefix)
	}
	burst = burst && a.stem != "" && b.stem != ""
	sameStem := a.stem != "" && b.stem != "" && (strings.HasPrefix(a.stem, b.stem) || strings.HasPrefix(b.stem, a.stem))
	if a.hash != nil && b.hash != nil {
		d, avg := uint64(bits.OnesCount64(a.hash[0]^b.hash[0])), uint64(bits.OnesCount64(a.hash[1]^b.hash[1]))
		if d <= s.Number("engine", "photo_distance", 5) && avg <= s.Number("engine", "photo_distance", 5) {
			return true
		}
		return sameDir && (burst || sameStem) && d <= s.Number("engine", "photo_burst_distance", 8) && avg <= s.Number("engine", "photo_burst_distance", 8)
	}
	if !sameDir || a.dimensions != b.dimensions || a.dimensions[0] == 0 {
		return false
	}
	delta := max(a.Mtime, b.Mtime) - min(a.Mtime, b.Mtime)
	return sameStem || burst && delta <= s.Number("engine", "photo_burst_seconds", 10)
}

// Match image-rs Triangle: vertical float32 pass, then horizontal pass rounded
// to the original channel depth, then encoded-space luma (not grayscale-first).
func photoDualHash(img image.Image) [2]uint64 {
	bounds := img.Bounds()
	w, h := bounds.Dx(), bounds.Dy()
	if w == 0 || h == 0 {
		return [2]uint64{}
	}
	maxValue, gray := float32(255), false
	switch img.(type) {
	case *image.Gray:
		gray = true
	case *image.Gray16:
		maxValue, gray = 65535, true
	case *image.NRGBA64, *image.RGBA64:
		maxValue = 65535
	}
	pixel := func(x, y int) [3]float32 {
		c := img.At(bounds.Min.X+x, bounds.Min.Y+y)
		if maxValue == 65535 {
			v := color.NRGBA64Model.Convert(c).(color.NRGBA64)
			return [3]float32{float32(v.R), float32(v.G), float32(v.B)}
		}
		v := color.NRGBAModel.Convert(c).(color.NRGBA)
		return [3]float32{float32(v.R), float32(v.G), float32(v.B)}
	}
	type tap struct {
		index  int
		weight float32
	}
	weights := func(source, target, coordinate int) []tap {
		ratio := float32(source) / float32(target)
		scale := max(float32(1), ratio)
		input := (float32(coordinate) + 0.5) * ratio
		left := max(0, min(source-1, int(math.Floor(float64(input-scale)))))
		right := max(left+1, min(source, int(math.Ceil(float64(input+scale)))))
		center := input - 0.5
		var out []tap
		sum := float32(0)
		for i := left; i < right; i++ {
			distance := float32(math.Abs(float64((float32(i) - center) / scale)))
			weight := max(float32(0), 1-distance)
			out = append(out, tap{i, weight})
			sum += weight
		}
		for i := range out {
			out[i].weight /= sum
		}
		return out
	}
	temp := make([][3]float32, w*8)
	for y := range 8 {
		for _, t := range weights(h, 8, y) {
			for x := range w {
				p := pixel(x, t.index)
				for c := range 3 {
					temp[y*w+x][c] += float32(p[c] * t.weight)
				}
			}
		}
	}
	var luma [72]uint8
	coefficients := [3]float32{math.Float32frombits(1046069339), math.Float32frombits(1060574716), math.Float32frombits(1033097077)}
	for x := range 9 {
		taps := weights(w, 9, x)
		for y := range 8 {
			var rgb [3]float32
			for _, t := range taps {
				for c := range 3 {
					rgb[c] += float32(temp[y*w+t.index][c] * t.weight)
				}
			}
			for c := range 3 {
				rgb[c] = float32(math.Round(float64(min(maxValue, max(0, rgb[c])))))
			}
			value := float32(0)
			if gray {
				value = rgb[0] * (255 / maxValue)
			} else {
				for c := range 3 {
					normalized := rgb[c] * (1 / maxValue)
					if runtime.GOARCH == "arm64" {
						value = float32(math.FMA(float64(normalized), float64(coefficients[c]), float64(value)))
					} else {
						value += float32(normalized * coefficients[c])
					}
				}
				value *= 255
			}
			luma[y*9+x] = uint8(math.Round(float64(min(float32(255), max(0, value)))))
		}
	}
	var result [2]uint64
	sum := uint32(0)
	for y := range 8 {
		for x := range 8 {
			if luma[y*9+x] > luma[y*9+x+1] {
				result[0] |= 1 << (y*8 + x)
			}
			sum += uint32(luma[y*9+x])
		}
	}
	for y := range 8 {
		for x := range 8 {
			if luma[y*9+x] >= uint8(sum/64) {
				result[1] |= 1 << (y*8 + x)
			}
		}
	}
	return result
}
