package ui

import (
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"unicode"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func statusPtr[T any](v T) *T { return &v }

func statusFixture() StatusPage {
	return StatusPage{Language: English, DiskTotal: 1_000_000_000_000, DiskFree: 300_000_000_000, CPUHistory: []float32{10, 40, 20}, Snapshot: &core.StatusSnapshot{
		CPUUsage: 23, CoreCount: 8, MemUsed: 4 << 30, MemTotal: 16 << 30, ProcessCount: 3, OSName: "Windows 11 Home China", UptimeSecs: 90000,
		Processes: []core.ProcInfo{
			{PID: 101, StartTime: 12345, UniqueID: statusPtr(uint64(999)), Name: "node", CPU: 30, MemBytes: 1 << 30, Threads: 8, Ports: []uint16{3000, 3001, 3002}, User: statusPtr("alice"), ExePath: statusPtr("/usr/bin/node"), CmdLine: statusPtr("node server.js"), ParentPID: statusPtr(uint32(50)), ParentName: statusPtr("shell"), StatusText: "Running"},
			{PID: 102, Name: "NODE", CPU: 20, MemBytes: 512 << 20, Threads: 4, User: statusPtr("alice")},
			{PID: 50, Name: "shell", CPU: 1, Threads: 2, User: statusPtr("root")},
		},
	}}
}

func statusTester(s *StatusPage, a StatusPageActions, width int) *ui.Tester {
	return ui.NewTester(func(c *ui.Context) {
		c.SetTheme(theme())
		ui.Column(c).Fill().Gap(0).Background(card).TextColor(text).Children(func() { s.RenderContent(c, a) })
	}, width, 800)
}

func TestStatusLoadingAndHardware(t *testing.T) {
	s := StatusPage{Language: English}
	tt := statusTester(&s, StatusPageActions{}, 1100)
	if !tt.HasText("Sampling…") || tt.HasText("Health Overview") {
		t.Fatal(tt.Texts())
	}
	s = statusFixture()
	tt.Frame()
	for _, label := range []string{"Hardware Overview", "Health Overview", "CPU", "Memory", "Disk", "Network", "Fan & Temp", "No fan detected"} {
		if !tt.HasText(label) {
			t.Errorf("missing %s", label)
		}
	}
	if tt.HasText("GPU") || tt.HasText("Battery") || tt.HasText("Cool") {
		t.Fatal("absent hardware must not acquire fabricated readings or controls")
	}
	// The real rasterizer must produce an offscreen frame, not merely a text tree.
	if im := tt.Image(); im.Bounds().Dx() != 1100 || im.Bounds().Dy() != 800 {
		t.Fatal(im.Bounds())
	}
	path := filepath.Join(t.TempDir(), "hardware.png")
	f, err := os.Create(path)
	if err != nil {
		t.Fatal(err)
	}
	if err = png.Encode(f, tt.Image()); err != nil {
		t.Fatal(err)
	}
	if err = f.Close(); err != nil {
		t.Fatal(err)
	}
	// Optional local review output is never a checked-in fixture.
	if dir := os.Getenv("QUICKCLEANER_STATUS_RENDER_DIR"); dir != "" {
		f, err := os.Create(filepath.Join(dir, "status-hardware.png"))
		if err != nil {
			t.Fatal(err)
		}
		err = png.Encode(f, tt.Image())
		closeErr := f.Close()
		if err != nil || closeErr != nil {
			t.Fatalf("render: %v / %v", err, closeErr)
		}
	}
	s.Language = Chinese
	tt.Frame()
	if !tt.HasText("硬件概览") || tt.HasText("Hardware Overview") {
		t.Fatal(tt.Texts())
	}
}

func TestStatusFanAndGPUActions(t *testing.T) {
	s := statusFixture()
	s.FanControlSupported = true
	s.FanHelperInstalled = true
	s.Snapshot.Thermal.Fans = []core.FanInfo{{RPM: 2345}}
	s.Snapshot.GPUs = []core.GpuReading{{ID: "one", Name: statusPtr("Intel Graphics"), Utilization: statusPtr(float32(12))}, {ID: "two", Name: statusPtr("NVIDIA RTX"), Utilization: statusPtr(float32(81))}}
	var modes []core.FanMode
	removed := 0
	a := StatusPageActions{ApplyFanMode: func(m core.FanMode) { modes = append(modes, m) }, RemoveFanHelper: func() { removed++ }, SelectGPU: func(id string) { s.SelectedGPU = id }}
	tt := statusTester(&s, a, 1280)
	if len(modes) != 0 || removed != 0 {
		t.Fatal("render called controller")
	}
	if tt.HasText("Cool") {
		t.Fatal("cooling without temperature is unsafe")
	}
	if err := tt.Click("Full Speed"); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(modes, []core.FanMode{{Kind: core.FanPercent, Percent: 100}}) {
		t.Fatal(modes)
	}
	s.Snapshot.Thermal.CPUTemp = statusPtr(float32(55))
	tt.Frame()
	if err := tt.Click("Cool"); err != nil {
		t.Fatal(err)
	}
	if modes[1].Percent != 60 {
		t.Fatal(modes)
	}
	s.FanApplying = true
	tt.Frame()
	tt.Click("Auto")
	tt.Click("Remove fan control component")
	if len(modes) != 2 || removed != 0 {
		t.Fatal("applying state must gate all fan writes")
	}
	s.FanApplying = false
	tt.Frame()
	if err := tt.Click("Remove fan control component"); err != nil {
		t.Fatal(err)
	}
	if removed != 1 {
		t.Fatal(removed)
	}
	if err := tt.Click("NVIDIA"); err != nil {
		t.Fatal(err)
	}
	if s.SelectedGPU != "two" {
		t.Fatal(s.SelectedGPU)
	}
	s.FanControlSupported = false
	tt.Frame()
	if tt.HasText("Full Speed") || tt.HasText("Cool") {
		t.Fatal("read-only platform exposes fan writes")
	}
}

func TestStatusProcessQueryFilterSortAndGrouping(t *testing.T) {
	s := statusFixture()
	s.Tab = StatusProcesses
	s.ExpandedGroups = map[string]bool{}
	a := StatusPageActions{SetQuery: func(q string) { s.Query = q }, SetFilter: func(f core.ProcFilterTab) { s.Filter = f }, SetSort: func(v StatusSort) { s.Sort = v }, ToggleGroup: func(g string) { s.ExpandedGroups[g] = !s.ExpandedGroups[g] }, SelectProcess: func(pid uint32) { s.SelectedPID = &pid }}
	tt := statusTester(&s, a, 1280)
	if !tt.HasText("3 running · 14 threads") || !tt.HasText("Showing 3 of 3 processes") {
		t.Fatal(tt.Texts())
	}
	rows := s.processRows(s.filteredProcesses())
	if len(rows) != 2 || rows[0].count != 2 || rows[0].process.CPU != 50 || rows[0].process.Threads != 12 {
		t.Fatal(rows)
	}
	if err := tt.Click("node"); err != nil {
		t.Fatal(err)
	}
	if !s.ExpandedGroups["node"] || !tt.HasText(":3000") {
		t.Fatal(tt.Texts())
	}
	if err := tt.Click("Threads"); err != nil {
		t.Fatal(err)
	}
	if s.Sort.Key != StatusSortThreads || s.Sort.Ascending {
		t.Fatal(s.Sort)
	}
	if err := tt.Click("Threads"); err != nil {
		t.Fatal(err)
	}
	if !s.Sort.Ascending {
		t.Fatal(s.Sort)
	}
	if err := tt.Click("System"); err != nil {
		t.Fatal(err)
	}
	if s.Filter != core.ProcFilterSystem || !tt.HasText("Showing 1 of 3 processes") {
		t.Fatal(tt.Texts())
	}
	s.Filter = core.ProcFilterAll
	for query, want := range map[string][]uint32{":3000": {101}, "cpu>25": {101}, "mem>768MB": {101}, "user:alice": {102, 101}, "-node": {50}, "shell, :3000": {50, 101}, "101": {101}} {
		s.Query = query
		got := s.filteredProcesses()
		ids := make([]uint32, len(got))
		for i, p := range got {
			ids[i] = p.PID
		}
		if !reflect.DeepEqual(ids, want) {
			t.Errorf("%q: got %v want %v", query, ids, want)
		}
	}
}

func TestStatusProcessIdentityAndDetailActions(t *testing.T) {
	s := statusFixture()
	s.Tab = StatusProcesses
	s.SelectedPID = statusPtr(uint32(101))
	var quit, force []core.ProcInfo
	var reveal, copy string
	selected := uint32(0)
	closed := false
	a := StatusPageActions{RequestQuit: func(p core.ProcInfo) { quit = append(quit, p) }, ForceQuit: func(p core.ProcInfo) { force = append(force, p) }, RevealProcess: func(p string) { reveal = p }, CopyCommand: func(cmd string) { copy = cmd }, SelectProcess: func(pid uint32) { selected = pid }, CloseProcess: func() { closed = true }}
	tt := statusTester(&s, a, 1280)
	if len(quit)+len(force) != 0 {
		t.Fatal("render must not terminate")
	}
	for _, label := range []string{"Quit", "Force Quit", "Open File Location", "Copy Command", "shell 50"} {
		if err := tt.Click(label); err != nil {
			t.Fatal(err)
		}
	}
	if len(quit) != 1 || len(force) != 1 || quit[0].StartTime != 12345 || *quit[0].UniqueID != 999 || !reflect.DeepEqual(quit[0], force[0]) {
		t.Fatal(quit, force)
	}
	if reveal != "/usr/bin/node" || copy != "node server.js" || selected != 50 {
		t.Fatal(reveal, copy, selected)
	}
	if err := tt.Click("×"); err != nil {
		t.Fatal(err)
	}
	if !closed {
		t.Fatal("close callback missing")
	}
}

func TestStatusSearchPopoverKeyboardAndCompletion(t *testing.T) {
	s := statusFixture()
	s.Tab = StatusProcesses
	a := StatusPageActions{SetQuery: func(q string) { s.Query = q }, SetFilter: func(f core.ProcFilterTab) { s.Filter = f }}
	tt := statusTester(&s, a, 1280)
	if err := tt.Click("status-search"); err != nil {
		t.Fatal(err)
	}
	tt.Type("mem")
	if s.Query != "mem" || !tt.HasText("Value in MB; m / g accepted") {
		t.Fatal(s.Query, tt.Texts())
	}
	tt.Key(0, ui.KeyEnter)
	if s.Query != "mem > " || !s.search.open {
		t.Fatal(s.Query, s.search.open)
	}
	tt.Type("512")
	if s.Query != "mem > 512" {
		t.Fatal(s.Query)
	}
	tt.Key(0, ui.KeyEscape)
	if s.search.open || s.Query != "mem > 512" {
		t.Fatal("first Escape must only dismiss")
	}
	tt.Key(0, ui.KeyEscape)
	if s.Query != "" {
		t.Fatal("second Escape must clear")
	}
	tt.Key(0, ui.KeyDown)
	tt.Key(0, ui.KeyEnter)
	if s.Query != "cpu > " {
		t.Fatal(s.Query)
	}
	// Clicking another non-input control dismisses without swallowing its action.
	if err := tt.Click("System"); err != nil {
		t.Fatal(err)
	}
	if s.search.open || s.Filter != core.ProcFilterSystem {
		t.Fatalf("outside click: suggestions open=%v, filter=%v, want closed/System", s.search.open, s.Filter)
	}
	for input, want := range map[string]string{"mem": "mem > ", "mem<=": "mem <= ", "cpu>": "cpu > ", "user": "user:"} {
		items := s.candidates(nil, input)
		if len(items) == 0 || items[0].query != want || !items[0].keepOpen {
			t.Fatalf("%s: %v", input, items)
		}
	}
	for _, input := range []string{"mem>1", "cpu>20", "user:SYSTEM"} {
		for _, item := range s.candidates(nil, input) {
			if item.keepOpen {
				t.Fatalf("must not overwrite %q: %v", input, item)
			}
		}
	}
	if statusPortQuery("user:alice") {
		t.Fatal("user field is not a port prefix")
	}
}

func TestStatusPortColumnBoundaryAndVirtualization(t *testing.T) {
	for _, width := range []int{1000, 1320} {
		for _, detail := range []bool{false, true} {
			s := statusFixture()
			s.Tab = StatusProcesses
			s.ExpandedGroups = map[string]bool{"node": true}
			if detail {
				s.SelectedPID = statusPtr(uint32(101))
			}
			tt := statusTester(&s, StatusPageActions{}, width)
			ports, ok := tt.Find("status-ports-101")
			if !ok {
				t.Fatal(tt.Texts())
			}
			cpu, ok := tt.Find("status-cpu-101")
			if !ok || ports.X+ports.W > cpu.X+.01 {
				t.Fatalf("width=%d detail=%v ports=%v cpu=%v", width, detail, ports, cpu)
			}
		}
	}
	s := statusFixture()
	s.Tab = StatusProcesses
	s.Snapshot.Processes = nil
	for i := 0; i < 10000; i++ {
		s.Snapshot.Processes = append(s.Snapshot.Processes, core.ProcInfo{PID: uint32(i + 10), Name: fmt.Sprintf("worker-%05d", i)})
	}
	s.Snapshot.ProcessCount = 10000
	tt := statusTester(&s, StatusPageActions{}, 1280)
	built := 0
	for _, v := range tt.Texts() {
		if strings.HasPrefix(v, "worker-") {
			built++
		}
	}
	if built > 35 || built == 0 {
		t.Fatal("not virtualized:", built)
	}
}

func TestStatusCopyAndCandidates(t *testing.T) {
	for key, pair := range statusCopy {
		if pair[0] == "" || pair[1] == "" {
			t.Fatal(key)
		}
		for _, r := range pair[1] {
			if unicode.Is(unicode.Han, r) {
				t.Fatalf("English %s contains Chinese", key)
			}
		}
	}
	s := statusFixture()
	ports := s.candidates(nil, ":30")
	if len(ports) != 3 || ports[0].query != ":3000" {
		t.Fatal(ports)
	}
	if got := statusCandidateIndex("vanished", ports); got != 0 {
		t.Fatal(got)
	}
	if got := statusCandidateIndex("", nil); got != -1 {
		t.Fatal(got)
	}
	for _, key := range []StatusSortKey{StatusSortCPU, StatusSortMemory, StatusSortName, StatusSortPID, StatusSortThreads, StatusSortCPUTime, StatusSortUser} {
		s.Sort = StatusSort{Key: key}
		got := s.filteredProcesses()
		if len(got) != 3 {
			t.Fatal(key, got)
		}
	}
}
