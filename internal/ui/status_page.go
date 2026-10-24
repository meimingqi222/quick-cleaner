package ui

import (
	"cmp"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

type StatusTab uint8

const (
	StatusHardware StatusTab = iota
	StatusProcesses
)

type StatusSortKey uint8

const (
	StatusSortCPU StatusSortKey = iota
	StatusSortMemory
	StatusSortName
	StatusSortPID
	StatusSortThreads
	StatusSortCPUTime
	StatusSortUser
)

// The zero value is CPU descending. Text columns default to ascending.
type StatusSort struct {
	Key       StatusSortKey
	Ascending bool
}

func (s StatusSort) Toggled(key StatusSortKey) StatusSort {
	if key == s.Key {
		s.Ascending = !s.Ascending
		return s
	}
	return StatusSort{Key: key, Ascending: key == StatusSortName || key == StatusSortUser}
}

// StatusPage contains presentation facts only. Nil Snapshot means warming up;
// nil optional native readings stay unknown. The parent owns tabs/live controls,
// refresh scheduling and the shared shell. Keep this page across frames for scroll
// and search-popover state, and replace its exported facts after sampling.
type StatusPage struct {
	Language                                             Language
	Tab                                                  StatusTab
	Snapshot                                             *core.StatusSnapshot
	DiskTotal, DiskFree                                  uint64
	CPUHistory                                           []float32
	GPUHistory                                           map[string][]float32
	SelectedGPU                                          string
	FanMode                                              core.FanMode
	FanApplying, FanControlSupported, FanHelperInstalled bool
	Query                                                string
	Filter                                               core.ProcFilterTab
	CurrentUser                                          *string
	Sort                                                 StatusSort
	ExpandedGroups                                       map[string]bool
	SelectedPID                                          *uint32
	SelectedCPUHistory                                   []float32
	SelectedMemHistory                                   []uint64
	CommandCopied                                        bool
	// Icons must already be decoded by a controller; rendering never reads files.
	Icons  map[string]*ui.Bitmap
	list   ui.ListState
	search statusSearchState
}

// RequestQuit must ask for confirmation before terminating. ForceQuit is a
// separate route (SIGKILL on macOS), never an alias for cooperative termination.
// Both receive the frozen sampled identity, not just a recyclable PID. All
// callbacks are event-only; native work belongs on the controller's worker.
type StatusPageActions struct {
	SetQuery        func(string)
	SetFilter       func(core.ProcFilterTab)
	SetSort         func(StatusSort)
	ToggleGroup     func(string)
	SelectProcess   func(uint32)
	CloseProcess    func()
	SelectGPU       func(string)
	ApplyFanMode    func(core.FanMode)
	RemoveFanHelper func()
	RequestQuit     func(core.ProcInfo)
	ForceQuit       func(core.ProcInfo)
	RevealProcess   func(string)
	CopyCommand     func(string)
}

func (s *StatusPage) RenderContent(c *ui.Context, actions StatusPageActions) {
	if s.Snapshot == nil {
		ui.Column(c).Grow(1).FillWidth().Center().Gap(12).Children(func() {
			icon(c, Status, primary, 28)
			ui.Text(c, s.tr("loading")).FontSize(14).TextColor(muted)
		})
		return
	}
	if s.Tab == StatusProcesses {
		s.renderProcesses(c, actions)
	} else {
		s.renderHardware(c, actions)
	}
}

func (s *StatusPage) filteredProcesses() []core.ProcInfo {
	if s.Snapshot == nil {
		return nil
	}
	var out []core.ProcInfo
	for _, p := range s.Snapshot.Processes {
		if core.MatchesProcTab(p, s.Filter, s.CurrentUser) && core.MatchProcQuery(p, s.Query) {
			out = append(out, p)
		}
	}
	slices.SortStableFunc(out, s.compareProcesses)
	return out
}

func statusString(p *string) string {
	if p == nil {
		return "-"
	}
	return *p
}

func (s *StatusPage) compareProcesses(a, b core.ProcInfo) int {
	var n int
	switch s.Sort.Key {
	case StatusSortCPU:
		// NaN compares equal, matching partial_cmp in the original.
		if a.CPU < b.CPU {
			n = -1
		} else if a.CPU > b.CPU {
			n = 1
		}
	case StatusSortMemory:
		n = cmp.Compare(a.MemBytes, b.MemBytes)
	case StatusSortName:
		n = strings.Compare(strings.ToLower(a.Name), strings.ToLower(b.Name))
	case StatusSortPID:
		n = cmp.Compare(a.PID, b.PID)
	case StatusSortThreads:
		n = cmp.Compare(a.Threads, b.Threads)
	case StatusSortCPUTime:
		n = cmp.Compare(a.CPUTimeMS, b.CPUTimeMS)
	case StatusSortUser:
		n = strings.Compare(strings.ToLower(statusString(a.User)), strings.ToLower(statusString(b.User)))
	}
	if !s.Sort.Ascending {
		n = -n
	}
	if n == 0 {
		return cmp.Compare(a.PID, b.PID)
	}
	return n
}

func statusButton(c *ui.Context, key, label string, active, disabled bool, action func()) {
	b := ui.ButtonBase(c.Key(key)).Padding(3, 8).Radius(6).Border(1, outlineVariant.Alpha(.7)).Background(surfaceLow).Disabled(disabled).Cursor(ui.CursorPointer)
	fg := muted
	if active {
		b.Background(primaryFixed)
		fg = primary
	} else if b.Hovered() {
		b.Background(surfaceHigh)
	}
	if disabled {
		fg = fg.Alpha(.5)
	}
	b.Children(func() { ui.Text(c, label).FontSize(12).FixedLineHeight(16).FontWeight(500).TextColor(fg) })
	if b.Clicked() && !disabled && action != nil {
		action()
	}
}
