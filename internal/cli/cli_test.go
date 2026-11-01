package cli

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"testing"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func invoke(t *testing.T, ctx context.Context, want int, args ...string) []byte {
	t.Helper()
	var out, diagnostic bytes.Buffer
	if code := Run(ctx, args, &out, &diagnostic); code != want {
		t.Fatalf("%v: exit=%d want=%d\nstdout=%s\nstderr=%s", args, code, want, out.Bytes(), diagnostic.Bytes())
	}
	return out.Bytes()
}

func decode[T any](t *testing.T, data []byte) T {
	t.Helper()
	var value T
	if err := json.Unmarshal(data, &value); err != nil {
		t.Fatalf("invalid JSON: %s: %v", data, err)
	}
	return value
}

func writeFixture(t *testing.T, root, relative, data string) string {
	t.Helper()
	path := filepath.Join(root, relative)
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(data), 0600); err != nil {
		t.Fatal(err)
	}
	return path
}

func TestTreeCommandsUseRealInventoryAndProductSearch(t *testing.T) {
	root := t.TempDir()
	a := writeFixture(t, root, "src/a.go", "a")
	b := writeFixture(t, root, "src/B.GO", "larger")
	writeFixture(t, root, "notes.txt", "notes")
	ctx := context.Background()
	r := decode[treeReport](t, invoke(t, ctx, ExitOK, "disk", "analyze", "--root", root, "--json"))
	if r.Partial || r.Files != 3 || len(r.Rows) != 2 || r.Root != root || r.Source != "live-directory" {
		t.Fatalf("inventory: %+v", r)
	}
	var size uint64
	for _, relative := range []string{"src/a.go", "src/B.GO", "notes.txt"} {
		info, err := os.Stat(filepath.Join(root, relative))
		if err != nil {
			t.Fatal(err)
		}
		size += platform.AllocatedFileSize(info)
	}
	if r.Bytes != size || r.Rows[0].Bytes < r.Rows[1].Bytes {
		t.Fatalf("aggregation/sort: %+v; want %d bytes", r, size)
	}
	s := decode[treeReport](t, invoke(t, ctx, ExitOK, "search", "--root", root, "--query", "*.go", "--json"))
	if len(s.Rows) != 2 || s.Truncated || s.Partial {
		t.Fatalf("search: %+v", s)
	}
	seen := map[string]bool{}
	for _, row := range s.Rows {
		seen[row.Path] = true
	}
	if !seen[a] || !seen[b] {
		t.Fatalf("search paths: %+v", s.Rows)
	}
	s = decode[treeReport](t, invoke(t, ctx, ExitOK, "search", "--root", root, "--query", "*.go", "--limit", "1", "--json"))
	if !s.Truncated || len(s.Rows) != 1 {
		t.Fatalf("limit: %+v", s)
	}
	r = decode[treeReport](t, invoke(t, ctx, ExitIncomplete, "disk", "analyze", "--root", root, "--max-entries", "2", "--json"))
	if !r.Partial || len(r.Blocked) == 0 {
		t.Fatalf("budget must report coverage gap: %+v", r)
	}
}

func TestTreeCancellationStillWritesJSON(t *testing.T) {
	root := t.TempDir()
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	r := decode[treeReport](t, invoke(t, ctx, ExitCancelled, "disk", "analyze", "--root", root, "--json"))
	if !r.Cancelled || !r.Partial {
		t.Fatalf("cancelled report: %+v", r)
	}
	expired, stop := context.WithDeadline(context.Background(), time.Now().Add(-time.Second))
	defer stop()
	r = decode[treeReport](t, invoke(t, expired, ExitTimeout, "search", "--root", root, "--query", "*", "--json"))
	if !r.Cancelled || !r.Partial {
		t.Fatalf("timeout report: %+v", r)
	}
}

func TestTreeDoesNotFollowLinks(t *testing.T) {
	root := t.TempDir()
	other := t.TempDir()
	writeFixture(t, other, "hidden.txt", "outside")
	if err := os.Symlink(other, filepath.Join(root, "link")); err != nil {
		t.Skipf("symlinks unavailable: %v", err)
	}
	r := decode[treeReport](t, invoke(t, context.Background(), ExitOK, "search", "--root", root, "--query", "hidden", "--json"))
	if r.Files != 0 || len(r.Rows) != 0 {
		t.Fatalf("followed link: %+v", r)
	}
	invoke(t, context.Background(), ExitUsage, "disk", "analyze", "--root", filepath.Join(root, "link"), "--json")
}

type brokenWriter struct{}

func (brokenWriter) Write([]byte) (int, error) { return 0, errors.New("broken pipe") }

func TestOutputErrorsAreFailures(t *testing.T) {
	for _, args := range [][]string{nil, {"junk"}, {"junk", "categories", "--json"}} {
		if code := Run(context.Background(), args, brokenWriter{}, io.Discard); code != ExitError {
			t.Errorf("%v: output failure returned %d", args, code)
		}
	}
}

func TestInvalidCommandsRefuseBeforeScanning(t *testing.T) {
	for _, args := range [][]string{
		{"unknown"}, {"disk"}, {"search", "--root", "."},
		{"disk", "analyze", "--root", ".", "--volume", "."},
		{"junk", "clean", "--execute"}, {"junk", "scan", "--execute", "--all"},
		{"junk", "clean", "--all", "--recommended"}, {"junk", "scan", "--category", "no-such-category"},
		{"junk", "clean", "--plan", "untrusted.json", "--execute", "--all"},
	} {
		invoke(t, context.Background(), ExitUsage, args...)
	}
}

func TestJunkSelectionRequiresFreshExactEligibleTarget(t *testing.T) {
	root := t.TempDir()
	items := []core.ScanItem{
		{Path: filepath.Join(root, "cache"), Recommended: true},
		{Path: filepath.Join(root, "busy"), Recommended: true, Busy: &core.Busy{Unknown: true}},
		{Path: filepath.Join(root, "optional")},
	}
	selected, err := selectJunk(items, junkOptions{})
	if err != nil || !selected[0] || !selected[1] || selected[2] {
		t.Fatalf("default selection must follow the recommendation, not occupancy: %v %v", selected, err)
	}
	for _, path := range []string{root, filepath.Join(root, "cache", "child")} {
		if _, err := selectJunk(items, junkOptions{paths: stringsFlag{path}}); err == nil {
			t.Errorf("accepted %s", path)
		}
	}
	selected, err = selectJunk(items, junkOptions{paths: stringsFlag{items[1].Path}})
	if err != nil || selected[0] || !selected[1] || selected[2] {
		t.Fatalf("busy explicit target must stay selectable: %v %v", selected, err)
	}
	selected, err = selectJunk(items, junkOptions{paths: stringsFlag{items[2].Path}})
	if err != nil || selected[0] || !selected[2] {
		t.Fatalf("explicit selection: %v %v", selected, err)
	}
	items = append(items, core.ScanItem{Path: filepath.Join(root, "locked"), Busy: &core.Busy{Open: true}})
	selected, err = selectJunk(items, junkOptions{all: true})
	if err != nil || !selected[0] || !selected[1] || !selected[2] || !selected[3] {
		t.Fatalf("select all must include optional and occupied items: %v %v", selected, err)
	}
	items = append(items, core.ScanItem{Path: filepath.Join(root, "blocked"), Rule: core.NewRuleRef(nil, "fixture", "")})
	selected, err = selectJunk(items, junkOptions{all: true})
	if err != nil || selected[4] {
		t.Fatalf("select all picked a rule-blocked item: %v %v", selected, err)
	}
}

// Isolate the trusted home and all settings/history writes, including Windows'
// cached original-user context. The actual scan and cleaner are not mocked.
func TestJunkScanPlanAndExplicitExecution(t *testing.T) {
	if os.Getenv("QC_CLI_TEST_CHILD") == "1" {
		testJunkLifecycle(t)
		return
	}
	base := t.TempDir()
	if core.IsDevelopmentInstallationPath(base) {
		home, err := os.UserHomeDir()
		if err != nil {
			t.Fatal(err)
		}
		base, err = os.MkdirTemp(home, ".quickcleaner-cli-test-")
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() {
			if err := os.RemoveAll(base); err != nil {
				t.Error(err)
			}
		})
	}
	cmd := exec.Command(os.Args[0], "-test.run=^TestJunkScanPlanAndExplicitExecution$", "--", "--orig-user-home", base)
	cmd.Env = append(os.Environ(), "QC_CLI_TEST_CHILD=1", "HOME="+base, "USERPROFILE="+base, "XDG_DATA_HOME="+filepath.Join(base, "data"))
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("isolated CLI lifecycle: %v\n%s", err, out)
	}
}

func testJunkLifecycle(t *testing.T) {
	home, ok := platform.UserHome()
	if !ok {
		t.Fatal("fixture home unavailable")
	}
	root := filepath.Join(home, "project")
	manifest := writeFixture(t, root, "package.json", `{"name":"cli-fixture","dependencies":{"fixture":"1"}}`)
	source := writeFixture(t, root, "src/main.js", "keep source")
	cache := writeFixture(t, root, "node_modules/fixture/data", "disposable build data")
	target := filepath.Dir(filepath.Dir(cache))
	ctx := context.Background()
	run := func(action string, want int, extra ...string) junkReport {
		t.Helper()
		args := append([]string{"junk", action, "--root", root, "--json"}, extra...)
		return decode[junkReport](t, invoke(t, ctx, want, args...))
	}
	for _, action := range []string{"scan", "plan", "clean"} {
		r := run(action, ExitOK, "--path", target)
		if !r.DryRun || r.Selected != 1 || r.Cleanup != nil || len(r.Items) != 1 || targetKey(r.Items[0].Path) != targetKey(target) {
			t.Fatalf("%s preview: %+v", action, r)
		}
		if action == "plan" && (len(r.Items[0].Plans) == 0 || len(r.Items[0].Plans[0].Steps) == 0 || len(r.Items[0].Plans[0].Targets) == 0) {
			t.Fatal("missing frozen plan explanation")
		}
		if _, err := os.Stat(cache); err != nil {
			t.Fatalf("preview mutated files: %v", err)
		}
	}
	cancelled, cancel := context.WithCancel(ctx)
	cancel()
	cancelArgs := []string{"junk", "clean", "--root", root, "--path", source, "--json"}
	if runtime.GOOS == "windows" || runtime.GOOS == "darwin" {
		cancelArgs = append(cancelArgs, "--execute")
	}
	interrupted := decode[junkReport](t, invoke(t, cancelled, ExitCancelled, cancelArgs...))
	if !interrupted.DryRun || !interrupted.Cancelled || !interrupted.Partial {
		t.Fatalf("cancelled cleanup: %+v", interrupted)
	}
	r := run("plan", ExitOK, "--exclude", target)
	if r.Selected != 0 {
		t.Fatalf("exclusion selected: %+v", r)
	}
	invoke(t, ctx, ExitUsage, "junk", "clean", "--root", root, "--path", source, "--execute", "--json")
	settingsPath, _ := core.SettingsPath()
	if _, err := os.Stat(settingsPath); !os.IsNotExist(err) {
		t.Fatalf("preview wrote settings: %v", err)
	}
	historyPath := filepath.Join(filepath.Dir(settingsPath), "history.jsonl")
	if _, err := os.Stat(historyPath); !os.IsNotExist(err) {
		t.Fatalf("preview wrote history: %v", err)
	}
	writeFixture(t, filepath.Dir(settingsPath), filepath.Base(settingsPath), `{"whitelist":null}`)
	invoke(t, ctx, ExitError, "junk", "plan", "--root", root, "--json")
	data, _ := json.Marshal(core.Settings{Language: core.English, Whitelist: []string{target}})
	if err := os.WriteFile(settingsPath, data, 0600); err != nil {
		t.Fatal(err)
	}
	r = run("plan", ExitOK)
	if r.Selected != 0 {
		t.Fatalf("persisted exclusion ignored: %+v", r)
	}
	if err := os.Remove(settingsPath); err != nil {
		t.Fatal(err)
	}
	if runtime.GOOS == "windows" {
		f, err := os.Open(cache)
		if err != nil {
			t.Fatal(err)
		}
		defer f.Close()
		// Occupancy does not deselect; the cleaner refuses the held tree and
		// reports it as a skip, not a failure.
		r = run("clean", ExitOK, "--path", target, "--execute")
		if r.Selected != 1 || r.Skipped != 0 || r.Cleanup == nil || len(r.Cleanup.Failed) != 0 || r.Cleanup.Skipped == 0 || r.Cleanup.DeletedFiles != 0 {
			t.Fatalf("held tree was not skipped at execution: %+v", r)
		}
		if _, err := os.Stat(historyPath); !os.IsNotExist(err) {
			t.Fatalf("no-op skip wrote history: %v", err)
		}
		clearProject := filepath.Join(root, "clear-project")
		clearManifest := writeFixture(t, clearProject, "package.json", `{"name":"clear-fixture"}`)
		clearSource := writeFixture(t, clearProject, "src/main.js", "keep clear project source")
		clearCache := writeFixture(t, clearProject, "node_modules/fixture/data", "disposable unlocked data")
		clearTarget := filepath.Dir(filepath.Dir(clearCache))
		r = run("clean", ExitOK, "--all", "--execute")
		if r.DryRun || r.Selected != 2 || r.Skipped != 0 || r.Cleanup == nil || r.Cleanup.DeletedFiles != 1 || len(r.Cleanup.Failed)+len(r.Cleanup.Manual) != 0 {
			t.Fatalf("select-all mixed cleanup: %+v", r)
		}
		if _, err := os.Stat(clearTarget); !os.IsNotExist(err) {
			t.Fatalf("select all did not clean unlocked target: %v", err)
		}
		for _, path := range []string{cache, clearManifest, clearSource} {
			if _, err := os.Stat(path); err != nil {
				t.Fatalf("select all mutated locked data or project source: %s: %v", path, err)
			}
		}
		history, err := os.ReadFile(historyPath)
		if err != nil {
			t.Fatal(err)
		}
		entry := decode[core.HistoryEntry](t, bytes.TrimSpace(history))
		if entry.Failed != 0 || entry.Skipped == 0 || len(entry.Targets) != 2 {
			t.Fatalf("select-all history counted the held target as failed: %+v", entry)
		}
		t.Logf("select all: deleted=%d skipped_at_execution=%d failed=%d; locked data and source preserved", r.Cleanup.DeletedFiles, r.Cleanup.Skipped, len(r.Cleanup.Failed))
		if err := f.Close(); err != nil {
			t.Fatal(err)
		}
	}
	if runtime.GOOS == "windows" || runtime.GOOS == "darwin" {
		r = run("clean", ExitOK, "--path", target, "--execute")
		if r.DryRun || r.Cleanup == nil || r.Cleanup.DeletedFiles == 0 {
			t.Fatalf("execution: %+v", r)
		}
		if _, err := os.Stat(target); !os.IsNotExist(err) {
			t.Fatalf("target retained: %v", err)
		}
		if _, err := os.Stat(historyPath); err != nil {
			t.Fatalf("execution history: %v", err)
		}
	} else {
		invoke(t, ctx, ExitError, "junk", "clean", "--root", root, "--path", target, "--execute", "--json")
		if _, err := os.Stat(cache); err != nil {
			t.Fatal("unsupported execution mutated files")
		}
	}
	for _, path := range []string{source, manifest} {
		if _, err := os.Stat(path); err != nil {
			t.Fatalf("source was touched: %s: %v", path, err)
		}
	}
}
