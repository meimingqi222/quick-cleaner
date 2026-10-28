package ui

import (
	"fmt"
	"reflect"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func TestSearchOrderingAndKinds(t *testing.T) {
	for _, tc := range []struct {
		name string
		dir  bool
		want FileVisualKind
	}{
		{"z.exe", true, FileDirectory}, {"x.EXE", false, FileExecutable}, {"x.DB3", false, FileDatabase},
		{"go", false, FileCode}, {"code.tar.GZ", false, FileArchive}, {"a.MOV", false, FileMedia},
		{"readme", false, FileGeneric}, {"x.İNI", false, FileGeneric}, {"x.INI", false, FileDocument},
	} {
		if got := FileKind(tc.name, tc.dir); got != tc.want {
			t.Errorf("%s: %v, want %v", tc.name, got, tc.want)
		}
	}
	hits := []core.SearchHit{
		{Name: "a.txt", Path: "/z", Size: 10}, {Name: "A.TXT", Path: "/a", Size: 30},
		{Name: "z.exe", Path: "/b", Size: 5}, {Name: "dir", Path: "/dir", IsDir: true, Size: 1},
		{Name: "ΟΣ.go", Path: "/g1", Size: 20}, {Name: "ος.go", Path: "/g2", Size: 21},
	}
	for _, tc := range []struct {
		group, asc bool
		col        SearchSortColumn
		want       []string
	}{
		{false, false, SearchSortSize, []string{"/a", "/g2", "/g1", "/z", "/b", "/dir"}},
		{true, false, SearchSortSize, []string{"/dir", "/b", "/g2", "/g1", "/a", "/z"}},
		{false, true, SearchSortName, []string{"/a", "/z", "/dir", "/b", "/g2", "/g1"}},
		{false, true, SearchSortPath, []string{"/a", "/b", "/dir", "/g1", "/g2", "/z"}},
		{true, false, SearchSortPath, []string{"/dir", "/b", "/g2", "/g1", "/z", "/a"}},
	} {
		copy := slices.Clone(hits)
		SortSearchResults(copy, tc.group, tc.col, tc.asc)
		var paths []string
		for _, h := range copy {
			paths = append(paths, h.Path)
		}
		if !reflect.DeepEqual(paths, tc.want) {
			t.Fatalf("group=%v col=%v asc=%v: %v, want %v", tc.group, tc.col, tc.asc, paths, tc.want)
		}
	}
}

func TestSearchEventsKeepResultsAndStablePaths(t *testing.T) {
	s := SearchPage{Language: English, Ready: true, Results: []core.SearchHit{
		{Name: "small.go", Path: "/example/small.go", Size: 15}, {Name: "large.zip", Path: "/example/large.zip", Size: 2 << 30},
	}}
	before := slices.Clone(s.Results)
	var queries, opened, revealed []string
	var sorts []SearchSortColumn
	a := SearchPageActions{
		SetQuery:          func(q string) { s.Query = q; queries = append(queries, q) },
		ToggleGroupByKind: func() { s.GroupByKind = !s.GroupByKind },
		SortBy:            func(col SearchSortColumn) { sorts = append(sorts, col) },
		Open:              func(path string) { opened = append(opened, path) }, Reveal: func(path string) { revealed = append(revealed, path) },
	}
	tt := ui.NewTester(func(c *ui.Context) { s.RenderContent(c, a) }, 1090, 720)
	click := func(label string) {
		t.Helper()
		if err := tt.Click(label); err != nil {
			t.Fatal(err)
		}
	}
	click("Type a filename (supports * and ? wildcards)…")
	tt.Type("缓存*.go")
	if s.Query != "缓存*.go" {
		t.Fatal(s.Query)
	}
	click("Clear search")
	if s.Query != "" || len(queries) < 2 || queries[len(queries)-1] != "" || !tt.HasText("small.go") {
		t.Fatal("clearing did not request top-N search with prior results retained", queries)
	}
	tt.Type("ΟΣ")
	tt.Key(0, ui.KeyEscape)
	if s.Query != "" {
		t.Fatal("Escape did not request empty-query search")
	}
	click("By Type")
	click("Name")
	if !s.GroupByKind || !slices.Equal(sorts, []SearchSortColumn{SearchSortName}) {
		t.Fatal(sorts)
	}
	click("Open")
	click("↗ Reveal")
	if !slices.Equal(opened, []string{"/example/small.go"}) || !slices.Equal(revealed, []string{"/example/small.go"}) {
		t.Fatal(opened, revealed)
	}
	if !reflect.DeepEqual(s.Results, before) {
		t.Fatal("render mutated controller results")
	}
	s.Results = []core.SearchHit{before[1]}
	tt.Frame()
	click("Open")
	if opened[len(opened)-1] != "/example/large.zip" {
		t.Fatal("row action retained stale path", opened)
	}
	tt.Advance(time.Second)
	click("large.zip")
	click("large.zip")
	if len(opened) != 3 || opened[2] != "/example/large.zip" {
		t.Fatal("double-click did not open result", opened)
	}
}

func TestSearchStatesAndVirtualization(t *testing.T) {
	s := SearchPage{Language: English, Ready: true, Indexing: true}
	tt := ui.NewTester(func(c *ui.Context) { s.RenderContent(c, SearchPageActions{}) }, 1090, 720)
	if !tt.HasText("Building full-disk search index…") || tt.HasText("No matching files found") {
		t.Fatal(tt.Texts())
	}
	s.Indexing = false
	s.Searching = true
	tt.Frame()
	if !tt.HasText("Building full-disk search index…") {
		t.Fatal(tt.Texts())
	}
	s.Searching = false
	s.Query = "missing"
	tt.Frame()
	if !tt.HasText("No matching files found") || !tt.HasText("Try another keyword, or widen it with * and ?") || tt.HasText("Matched 0 items") {
		t.Fatal(tt.Texts())
	}
	for i := range 500 {
		s.Results = append(s.Results, core.SearchHit{Name: fmt.Sprintf("item-%03d.go", i), Path: fmt.Sprintf("/example/%03d.go", i), Size: uint64(i + 1)})
	}
	s.Searching = true
	tt.Frame()
	if !tt.HasText("item-000.go") || tt.HasText("item-499.go") || tt.HasText("Building full-disk search index…") || !tt.HasText("Searching…") {
		t.Fatal("searching discarded prior results or list is not virtualized")
	}
	s.Language = Chinese
	s.Searching = false
	tt.Frame()
	if !tt.HasText("文件搜索") || !tt.HasText("500 条结果") || tt.HasText("匹配到 500 个文件 / 文件夹") {
		t.Fatal(tt.Texts())
	}
}

// An empty table must say why it is empty; only a typed query that matched
// nothing is a "not found".
func TestSearchEmptyStatesExplainThemselves(t *testing.T) {
	s := SearchPage{Language: English, Ready: true}
	tt := ui.NewTester(func(c *ui.Context) { s.RenderContent(c, SearchPageActions{}) }, 1090, 720)
	if !tt.HasText("Type a keyword to search") || tt.HasText("No matching files found") {
		t.Fatal("an empty query is not a failed search", tt.Texts())
	}
	s.Ready = false
	tt.Frame()
	if !tt.HasText("The search index is not ready") || tt.HasText("Type a keyword to search") {
		t.Fatal(tt.Texts())
	}
	s.Ready, s.Query = true, "missing"
	tt.Frame()
	if !tt.HasText("No matching files found") || tt.HasText("Type a keyword to search") {
		t.Fatal(tt.Texts())
	}
}
