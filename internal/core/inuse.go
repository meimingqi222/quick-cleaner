package core

import "github.com/meimingqi222/quick-cleaner/internal/platform"

type SpotCheck uint8

const (
	SpotClear SpotCheck = iota
	SpotBusy
	SpotUnknown
)

type Busy struct {
	RecommendedBeforeBusy bool
	App                   string
	Open                  bool
	Unknown               bool
}

func (b Busy) IsEmpty() bool { return b.App == "" && !b.Open && !b.Unknown }

// Badge returns the bilingual label, whether it is application-level, and
// whether a badge exists. Application attribution wins over open and unknown.
func (b Busy) Badge() (Text, bool, bool) {
	if b.App != "" {
		return NewText("应用打开中 · "+b.App, b.App+" running"), true, true
	}
	if b.Open {
		return NewText("系统占用", "In use"), false, true
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

// ApplyBusy merges scan-time occupancy into items and clears recommended.
// The previous recommendation is kept so cache release can still see it.
// An empty observation is not occupancy. Returns the number of items changed.
func ApplyBusy(categories []CategorySummary, busy map[string]Busy) int {
	n := 0
	for ci := range categories {
		items := categories[ci].Items
		for ii := range items {
			b, ok := busy[items[ii].Path]
			if !ok || b.IsEmpty() {
				continue
			}
			b.RecommendedBeforeBusy = items[ii].Recommended || items[ii].Busy != nil && items[ii].Busy.RecommendedBeforeBusy
			items[ii].Busy = &b
			items[ii].Recommended = false
			n++
		}
	}
	return n
}
