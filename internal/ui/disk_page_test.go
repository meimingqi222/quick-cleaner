package ui

import (
	"image/png"
	"math"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func diskPageFixture() DiskPage {
	v := core.VolumeFromMountPoint("/")
	space := &DiskPageSpace{Total: 512 << 30, Free: 128 << 30}
	return DiskPage{Language: English, Loaded: true, Volume: v, Volumes: []DiskPageVolume{{ID: v, Space: space}, {ID: core.VolumeFromMountPoint("/Volumes/Backup")}}, Space: space, AtRoot: true,
		CurrentSize: 640 << 30, UniqueSize: 384 << 30, CurrentName: "System (/)", CurrentPath: "/", Crumbs: []DiskPageCrumb{{ID: 0, Name: "System (/)", Path: "/"}},
		Rows: []DiskPageRow{
			{Node: core.Node{Idx: 1, Name: "Users", IsDir: true, Size: 400 << 30}, Path: "/Users", CanEnter: true, CanSelect: true, Selected: true},
			{Node: core.Node{Idx: 2, Name: "System", IsDir: true, Size: 200 << 30}, Path: "/System", Protected: true, CanEnter: true},
			{Node: core.Node{Idx: 3, Name: "movie.mp4", Size: 40 << 30}, Path: "/movie.mp4", CanSelect: true, CanDelete: true},
		}, SelectedCount: 1, SelectedSize: 400 << 30, CanSelectView: true, HeaderMixed: true, CanRequestDelete: true}
}

func diskPageTester(s *DiskPage, a DiskPageActions) *ui.Tester {
	return ui.NewTester(func(c *ui.Context) {
		Dashboard{Language: s.Language}.RenderPage(c, Disk, DashboardActions{}, func() { s.RenderContent(c, a) })
	}, 1440, 900)
}

func TestDiskPageEventsAreControllerRequests(t *testing.T) {
	s := diskPageFixture()
	original := append([]DiskPageRow(nil), s.Rows...)
	var nav []uint32
	var deleted, toggled []DiskPageRow
	var copied string
	confirmed, cleared := 0, 0
	var viewOn bool
	a := DiskPageActions{
		Navigate:              func(id uint32) { nav = append(nav, id) },
		ToggleSelected:        func(r DiskPageRow) { toggled = append(toggled, r) },
		RequestDelete:         func(r DiskPageRow) { deleted = append(deleted, r) },
		CopyPath:              func(path string) { copied = path },
		RequestDeleteSelected: func() { confirmed++ }, ClearSelection: func() { cleared++ },
		SetViewSelected: func(on bool) { viewOn = on },
		SetRecycleBin:   func(on bool) { s.ToRecycleBin = on },
		SetTab:          func(tab DiskTab) { s.Tab = tab },
	}
	tt := diskPageTester(&s, a)
	for _, label := range []string{"Disk Lens Analyzer", "System Protected", "Free Space", core.FmtSize(s.Space.Total) + " Total", "Selected Disk Items"} {
		if !tt.HasText(label) {
			t.Fatalf("missing %q: %v", label, tt.Texts())
		}
	}
	// The ring summary is two exact lines; wrapped as one, the unit fell
	// onto a line of its own.
	for _, line := range []string{"Used 75%", "Free " + core.FmtSize(s.Space.Free)} {
		if !slices.Contains(tt.Texts(), line) {
			t.Fatalf("ring summary line %q missing: %v", line, tt.Texts())
		}
	}
	r, ok := tt.Find("Select current view")
	if !ok {
		t.Fatal("missing mixed header")
	}
	bg := tt.Image().RGBAAt(int(r.X+3), int(r.Y+r.H/2))
	dash := tt.Image().RGBAAt(int(r.X+8), int(r.Y+r.H/2))
	if bg.B <= bg.R || dash.R < 240 || dash.G < 240 || dash.B < 240 {
		t.Fatalf("mixed header must paint a white dash on blue: %v %v", bg, dash)
	}
	for _, label := range []string{"Users", "Select movie.mp4", "Select current view", "Delete", "Copy path", "Confirm Permanent Delete", "Delete to Recycle Bin", "Clear Selection"} {
		if err := tt.Click(label); err != nil {
			t.Fatal(err)
		}
	}
	if !reflect.DeepEqual(nav, []uint32{1}) || !reflect.DeepEqual(toggled, []DiskPageRow{original[2]}) || !reflect.DeepEqual(deleted, []DiskPageRow{original[2]}) || copied != "/" || confirmed != 1 || cleared != 1 || !viewOn || !s.ToRecycleBin {
		t.Fatalf("events nav=%v toggle=%v delete=%v copy=%q confirm=%d clear=%d view=%v recycle=%v", nav, toggled, deleted, copied, confirmed, cleared, viewOn, s.ToRecycleBin)
	}
	if !tt.HasText("Moved to Recycle Bin (space not freed yet)") {
		t.Fatal("recycle bytes presented as freed")
	}
	if !reflect.DeepEqual(s.Rows, original) {
		t.Fatal("render mutated rows/selection")
	}
	if err := tt.Click("Large Files"); err != nil {
		t.Fatal(err)
	}
	if s.Tab != DiskFiles || tt.HasText("Copy path") || !tt.HasText("/movie.mp4") {
		t.Fatal("large files did not switch to full paths")
	}
}

func TestDiskPageCapabilitiesAndBusy(t *testing.T) {
	for _, mode := range []string{"protected", "no-capability", "cleaning", "denied-batch"} {
		t.Run(mode, func(t *testing.T) {
			s := diskPageFixture()
			s.Rows = s.Rows[2:]
			s.Rows[0].Protected = mode == "protected"
			s.Rows[0].CanSelect = mode != "no-capability"
			s.Rows[0].CanDelete = mode != "no-capability"
			s.Cleaning = mode == "cleaning"
			s.CanRequestDelete = mode != "denied-batch"
			n := 0
			a := DiskPageActions{ToggleSelected: func(DiskPageRow) { n++ }, RequestDelete: func(DiskPageRow) { n++ }, RequestDeleteSelected: func() { n++ }}
			tt := diskPageTester(&s, a)
			if mode != "denied-batch" {
				_ = tt.Click("Select movie.mp4")
				_ = tt.Click("Delete")
			} else {
				_ = tt.Click("Confirm Permanent Delete")
			}
			if n != 0 {
				t.Fatalf("unauthorized request dispatched %d", n)
			}
			if mode == "cleaning" && tt.HasText("Confirm Permanent Delete") {
				t.Fatal("clean bar stayed beside the progress stop control")
			}
		})
	}
}

func TestDiskPageBreadcrumbsAndVolumes(t *testing.T) {
	s := diskPageFixture()
	s.SelectedCount = 0
	s.Crumbs = []DiskPageCrumb{{ID: 0, Name: "Root", Path: "/"}, {ID: 4, Name: "Library", Path: "/Library"}, {ID: 5, Name: "Application Support", Path: "/Library/Application Support"}, {ID: 6, Name: "Example", Path: "/Library/Application Support/Example"}}
	s.CurrentPath = s.Crumbs[3].Path
	var id uint32
	up := 0
	a := DiskPageActions{Navigate: func(n uint32) { id = n }, SetCrumbsExpanded: func(b bool) { s.CrumbsExpanded = b }, Up: func() { up++ }, SwitchVolume: func(v core.VolumeID) { s.Volume = v }}
	tt := diskPageTester(&s, a)
	if slices.Contains(tt.Texts(), "Library") {
		t.Fatal("deep breadcrumbs not collapsed")
	}
	if err := tt.Click("…"); err != nil {
		t.Fatal(err)
	}
	if !slices.Contains(tt.Texts(), "Library") {
		t.Fatal("breadcrumbs not expanded")
	}
	if err := tt.Click("Library"); err != nil {
		t.Fatal(err)
	}
	if id != 4 {
		t.Fatal(id)
	}
	if err := tt.Click("‹ Collapse"); err != nil {
		t.Fatal(err)
	}
	if err := tt.Click("← Up"); err != nil {
		t.Fatal(err)
	}
	if up != 1 || s.CrumbsExpanded {
		t.Fatal("breadcrumb callbacks")
	}
	if err := tt.Click("System (/) ▾"); err != nil {
		t.Fatal(err)
	}
	if err := tt.ChooseMenuItem("Backup"); err != nil {
		t.Fatal(err)
	}
	if s.Volume.Mount != core.VolumeFromMountPoint("/Volumes/Backup").Mount {
		t.Fatal(s.Volume)
	}
}

func TestDiskPageStatesAndSnapshots(t *testing.T) {
	for _, mode := range []string{"tree", "files", "prompt", "loading", "error", "partial", "empty", "chinese", "deep"} {
		t.Run(mode, func(t *testing.T) {
			s := diskPageFixture()
			switch mode {
			case "files":
				s.Tab = DiskFiles
				s.Rows = s.Rows[2:]
			case "prompt":
				s.Loaded = false
			case "loading":
				s.Scanning = true
				s.Windows = true
				s.IndexStatus = "Reading native index"
			case "error":
				s.Error = "Access denied"
			case "partial":
				s.Partial = true
				s.IndexStatus = "Some folders were inaccessible"
				s.CanRequestDelete = false
			case "empty":
				s.Rows = nil
				s.CurrentSize = 0
				s.AtRoot = false
				s.SelectedCount = 0
			case "chinese":
				s.Language = Chinese
			case "deep":
				s.AtRoot = false
				s.Space = nil
				s.CurrentPath = "/Users/example/Library/Application Support/Example"
				s.CurrentName = "Example"
				s.SelectedCount = 0
			}
			a := DiskPageActions{StartScan: func() {}, Up: func() {}, ClearSelection: func() {}, RequestDeleteSelected: func() {}, SwitchVolume: func(core.VolumeID) {}, SetTab: func(DiskTab) {}, Navigate: func(uint32) {}, SetCrumbsExpanded: func(bool) {}, ToggleSelected: func(DiskPageRow) {}, RequestDelete: func(DiskPageRow) {}, SetViewSelected: func(bool) {}, SetRecycleBin: func(bool) {}, CopyPath: func(string) {}}
			tt := diskPageTester(&s, a)
			want := map[string]string{"tree": "Directory Tree", "files": "Largest Files Breakdown", "prompt": "Select a drive to analyze storage hierarchy", "loading": "Reading native index", "error": "Disk analysis failed: Access denied", "partial": "Partial index · Some folders were inaccessible", "empty": "Directory is empty", "chinese": "Disk Lens 磁盘透镜", "deep": core.FmtSize(s.UniqueSize) + " Scanned"}[mode]
			if !tt.HasText(want) {
				t.Fatalf("missing %q: %v", want, tt.Texts())
			}
			if dir := os.Getenv("DISK_PAGE_SNAPSHOTS"); dir != "" {
				f, err := os.Create(filepath.Join(dir, "disk-"+mode+".png"))
				if err != nil {
					t.Fatal(err)
				}
				if err := png.Encode(f, tt.Image()); err != nil {
					_ = f.Close()
					t.Fatal(err)
				}
				if err := f.Close(); err != nil {
					t.Fatal(err)
				}
			}
		})
	}
}

func TestDiskPageBreakdownPhysicalApparentAndUnique(t *testing.T) {
	s := diskPageFixture()
	_, size, _, items := s.breakdown()
	if size != 384<<30 || len(items) != 4 {
		t.Fatalf("physical size=%d items=%v", size, items)
	}
	var sum float64
	for _, item := range items {
		sum += item.ratio
	}
	if math.Abs(sum-1) > 1e-9 {
		t.Fatal("apparent bytes overcount physical chart", sum)
	}
	if items[3].name != "Free Space" || items[3].color != diskColors[4] {
		t.Fatal("free-space fixed color/index")
	}
	// Unlisted used space includes what no row accounts for; no item count.
	one := s
	one.Rows = s.Rows[2:3]
	_, _, _, partial := one.breakdown()
	other := ""
	for _, item := range partial {
		if strings.HasPrefix(item.name, "Other") {
			other = item.name
		}
	}
	if other != "Other used space" {
		t.Fatalf("unlisted used space labelled %q", other)
	}
	s.AtRoot = false
	_, size, _, items = s.breakdown()
	if size != 640<<30 || items[0].ratio != .625 {
		t.Fatal("folder must use apparent bytes", size, items)
	}
	s.Tab = DiskFiles
	s.Rows = []DiskPageRow{{Node: core.Node{Name: "MOVIE.MP4", Size: 10}}, {Node: core.Node{Name: "archive.iso", Size: 30}}, {Node: core.Node{Name: "unknown", Size: 60}}}
	_, size, _, items = s.breakdown()
	if size != 100 || len(items) != 3 || items[0].ratio != .1 || items[1].ratio != .3 || items[2].color != diskColors[5] {
		t.Fatal("file categories", size, items)
	}
}

func TestDiskPagePromptAndResolvedSelection(t *testing.T) {
	s := diskPageFixture()
	s.Loaded = false
	started := 0
	a := DiskPageActions{StartScan: func() { started++ }, SwitchVolume: func(v core.VolumeID) { s.Volume = v }}
	tt := diskPageTester(&s, a)
	if err := tt.Click("Backup"); err != nil {
		t.Fatal(err)
	}
	if err := tt.Click("Analyze Storage for Backup"); err != nil {
		t.Fatal(err)
	}
	if started != 1 || s.Volume.Mount != core.VolumeFromMountPoint("/Volumes/Backup").Mount {
		t.Fatal("prompt callback contract")
	}

	// The controller has selected an ancestor and excluded this child. The
	// view must not inherit again from the selected count or displayed path.
	s.Loaded = true
	s.Rows = []DiskPageRow{{Node: core.Node{Idx: 7, Name: "excluded.txt", Size: 20}, Path: "/Users/excluded.txt", CanSelect: true}}
	s.HeaderMixed = false
	var toggled DiskPageRow
	viewOn := true
	a.ToggleSelected = func(r DiskPageRow) { toggled = r }
	a.SetViewSelected = func(on bool) { viewOn = on }
	tt = diskPageTester(&s, a)
	if err := tt.Click("Select excluded.txt"); err != nil {
		t.Fatal(err)
	}
	if toggled.Selected || toggled.Node.Idx != 7 {
		t.Fatal("excluded child selection was inferred", toggled)
	}
	s.HeaderSelected = true
	tt.Frame()
	if err := tt.Click("Select current view"); err != nil {
		t.Fatal(err)
	}
	if viewOn {
		t.Fatal("selected header must request clear, not select")
	}
}
