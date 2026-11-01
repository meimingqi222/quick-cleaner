package core

import "github.com/meimingqi222/quick-cleaner/internal/platform"

type SpotCheck uint8

const (
	SpotClear SpotCheck = iota
	SpotBusy
	SpotUnknown
)

// Busy is display evidence only. It never changes recommendation or
// selection; the cleaner resolves occupancy per leaf at execution time.
type Busy struct {
	App     string
	Open    bool
	Unknown bool
}

func (b Busy) IsEmpty() bool { return b.App == "" && !b.Open && !b.Unknown }

// Badge returns the bilingual label, whether it is application-level, and
// whether a badge exists. Application attribution wins over open and unknown.
func (b Busy) Badge() (Text, bool, bool) {
	if b.App != "" {
		return NewText("应用打开中 · "+b.App, b.App+" running"), true, true
	}
	if b.Open {
		return NewText("正在使用", "In use"), false, true
	}
	if b.Unknown {
		return NewText("占用状态未知", "Busy status unknown"), false, true
	}
	return Text{}, false, false
}

// IsOpen returns (open, known). Unknown evidence must never authorize deletion.
func IsOpen(path string) (bool, bool) { return platform.IsOpen(path) }
func SpotCheckInUse(paths []string) map[string]SpotCheck {
	out := make(map[string]SpotCheck, len(paths))
	for path, state := range platform.SpotCheckInUse(paths) {
		out[path] = SpotCheck(state)
	}
	return out
}
func DetectInUse(paths []string) map[string]Busy {
	out := make(map[string]Busy)
	for path, b := range platform.DetectInUse(paths) {
		out[path] = Busy{App: b.App, Open: b.Open, Unknown: b.Unknown}
	}
	return out
}

// ApplyBusy attaches occupancy badges. It must not touch Recommended: default
// selection follows the rule, not a snapshot of who held a file at scan time.
// An empty observation is not occupancy. Returns the number of items changed.
func ApplyBusy(categories []CategorySummary, busy map[string]Busy) int {
	n := 0
	for ci := range categories {
		items := categories[ci].Items
		for ii := range items {
			if items[ii].Operation.IsNativeResource() || IsVirtualPath(items[ii].Path) {
				continue
			}
			b, ok := busy[items[ii].Path]
			if !ok || b.IsEmpty() {
				continue
			}
			items[ii].Busy = &b
			n++
		}
	}
	return n
}

// CleanupAvailable is a selection filter, not execution authority. Only rule
// conflicts block selection; occupancy stays a badge. The cleaner still
// revalidates the frozen plan and skips locked leaves.
func (item ScanItem) CleanupAvailable() bool {
	return item.Rule.BlockedReason() == ""
}

func (category CategorySummary) CleanableSize() uint64 {
	var size uint64
	for _, item := range category.Items {
		if item.CleanupAvailable() {
			size = saturatingAdd(size, item.Size)
		}
	}
	return size
}
