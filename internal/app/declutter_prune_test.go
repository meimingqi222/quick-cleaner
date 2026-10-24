package app

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

// 只有「勾选 + 报告没失败 + 路径确实不在了」三条同时成立才摘除。只看报告会把
// 「返回成功但文件还在」的条目从界面抹掉，失败的条目也会消失让人以为清掉了。
func TestDeclutterRowLeavesListOnlyWhenReallyGone(t *testing.T) {
	dir := t.TempDir()
	moved := filepath.Join(dir, "moved.txt")
	still := filepath.Join(dir, "still.txt")
	if err := os.WriteFile(still, []byte("x"), 0600); err != nil {
		t.Fatal(err)
	}
	if !declutterItemIsGone(moved, true, nil) {
		t.Fatal("moved row stayed in the list")
	}
	if declutterItemIsGone(still, true, nil) {
		t.Fatal("a file still on disk left the list")
	}
	if declutterItemIsGone(moved, true, []string{moved}) {
		t.Fatal("a reported failure left the list")
	}
	if declutterItemIsGone(moved, false, nil) {
		t.Fatal("an unselected row left the list")
	}
}

// 清理后不再整页重扫：失败的条目留在原处，用户能看见并重试。整页重扫会把它们
// 当成新结果再列一遍，看起来就是「清理失败后清理项又出现一次」。
func TestPruneDeclutterKeepsFailedRows(t *testing.T) {
	r, _ := testRoot(t)
	dir := t.TempDir()
	moved := filepath.Join(dir, "moved.txt")
	still := filepath.Join(dir, "still.txt")
	if err := os.WriteFile(still, []byte("x"), 0600); err != nil {
		t.Fatal(err)
	}
	r.declutter.LargeFiles = []core.LargeFileItem{
		{Path: moved, Selected: true},
		{Path: still, Selected: true},
		{Path: filepath.Join(dir, "unselected.txt")},
	}
	r.pruneDeclutter(view.DeclutterLargeFiles, core.CleanReport{Failed: []core.CleanFailure{{Path: still}}})
	if len(r.declutter.LargeFiles) != 2 {
		t.Fatalf("rows left %d, want 2 (failed + unselected)", len(r.declutter.LargeFiles))
	}
	if r.declutter.LargeFiles[0].Path != still {
		t.Fatalf("first row %q, want the failed row %q", r.declutter.LargeFiles[0].Path, still)
	}
}
