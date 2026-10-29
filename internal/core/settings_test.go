package core_test

import (
	"bytes"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func TestSettingsRustJSONCompatibility(t *testing.T) {
	text := `{"language":"En","delete_to_recycle_bin":true,"macos_fda_dismissed":true,"whitelist":["/fixture/MixedCase"],"brew_cleanup_at":1700000000,"auto_check_updates":false,"skipped_update_version":"0.0.8","last_update_check_at":1700000001}`
	s := core.MergeSettingsJSON(text)
	if s.Language != core.English || !s.DeleteToRecycleBin || !s.MacOSFDADismissed || s.AutoCheckUpdates || len(s.Whitelist) != 1 || s.Whitelist[0] != "/fixture/MixedCase" || s.BrewCleanupAt == nil || *s.BrewCleanupAt != 1700000000 || s.LastUpdateCheckAt == nil || *s.LastUpdateCheckAt != 1700000001 || s.SkippedUpdateVersion == nil || *s.SkippedUpdateVersion != "0.0.8" {
		t.Fatalf("Rust settings not preserved: %+v", s)
	}
	data, err := json.Marshal(s)
	if err != nil || !reflect.DeepEqual(core.MergeSettingsJSON(string(data)), s) {
		t.Fatalf("round trip: %s %v", data, err)
	}
	for _, text := range []string{`{}`, `{"future_option":{"nested":[null,1]}}`} {
		if got := core.MergeSettingsJSON(text); !reflect.DeepEqual(got, core.DefaultSettings()) {
			t.Errorf("legacy/default %s: %+v", text, got)
		}
	}
	s = core.MergeSettingsJSON(`{"language":"Zh","future_option":42}`)
	if s.Language != core.Chinese || s.DeleteToRecycleBin || !s.AutoCheckUpdates || s.Whitelist == nil {
		t.Fatalf("legacy defaults: %+v", s)
	}
	for _, bad := range []string{
		``, `   `, `{`, `not json`, `[1,2,3]`, `null`, `{} {}`,
		`{"language":"Klingon","delete_to_recycle_bin":true}`,
		`{"language":null,"delete_to_recycle_bin":true}`,
		`{"delete_to_recycle_bin":null,"macos_fda_dismissed":true}`,
		`{"auto_check_updates":null,"delete_to_recycle_bin":true}`,
		`{"whitelist":null,"delete_to_recycle_bin":true}`,
		`{"whitelist":[null],"delete_to_recycle_bin":true}`,
		`{"whitelist":[7],"delete_to_recycle_bin":true}`,
		`{"brew_cleanup_at":1.5,"delete_to_recycle_bin":true}`,
		`{"last_update_check_at":9223372036854775808,"delete_to_recycle_bin":true}`,
		`{"delete_to_recycle_bin":false,"delete_to_recycle_bin":true}`,
	} {
		if got := core.MergeSettingsJSON(bad); !reflect.DeepEqual(got, core.DefaultSettings()) {
			t.Errorf("invalid settings must wholly default: %q -> %+v", bad, got)
		}
	}
}

// A child process isolates Windows' cached original-user context and keeps all
// persistence checks away from the developer's actual configuration directory.
func TestConfigPersistence(t *testing.T) {
	if os.Getenv("QC_CONFIG_CHILD") == "1" {
		testConfigPersistence(t)
		return
	}
	base := t.TempDir()
	cmd := exec.Command(os.Args[0], "-test.run=^TestConfigPersistence$", "--", "--orig-user-home", base)
	cmd.Env = append(os.Environ(), "QC_CONFIG_CHILD=1", "HOME="+base, "USERPROFILE="+base, "XDG_DATA_HOME="+filepath.Join(base, "data"))
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("isolated config test: %v\n%s", err, out)
	}
}

func testConfigPersistence(t *testing.T) {
	path, ok := core.SettingsPath()
	if !ok || filepath.Base(path) != "settings.json" || filepath.Base(filepath.Dir(path)) != "QuickCleaner" {
		t.Fatalf("settings path: %q, %v", path, ok)
	}
	if got := core.LoadSettings(); !reflect.DeepEqual(got, core.DefaultSettings()) {
		t.Fatal("missing settings must default")
	}
	s := core.MergeSettingsJSON(`{"language":"En","delete_to_recycle_bin":true,"whitelist":["~/KeepCase"],"auto_check_updates":false}`)
	s.Save()
	if got := core.LoadSettings(); !reflect.DeepEqual(got, s) {
		t.Fatalf("save/load: %+v != %+v", got, s)
	}
	home := os.Getenv("HOME")
	if !core.IsProtected(filepath.Join(home, "KeepCase", "child")) || core.IsWhitelisted(filepath.Join(home, "KeepCaseExtra")) {
		t.Fatal("loading settings must install the whitelist in central safety")
	}
	if _, err := os.Stat(path + ".tmp"); !os.IsNotExist(err) {
		t.Fatal("temporary settings file left behind", err)
	}
	if runtime.GOOS != "windows" {
		if err := os.Chmod(path, 0644); err != nil {
			t.Fatal(err)
		}
	}
	s.Language = core.Chinese
	s.Save()
	if runtime.GOOS != "windows" {
		info, err := os.Stat(path)
		if err != nil || info.Mode().Perm() != 0600 {
			t.Fatalf("settings permissions: %v %v", info, err)
		}
	}
	if got := core.LoadSettings(); !reflect.DeepEqual(got, s) {
		t.Fatal("overwriting existing settings lost values")
	}
	if err := os.WriteFile(path, []byte("{"), 0600); err != nil {
		t.Fatal(err)
	}
	if got := core.LoadSettings(); !reflect.DeepEqual(got, core.DefaultSettings()) || len(core.CurrentWhitelist()) != 0 {
		t.Fatal("corrupt settings must default and reload the resulting whitelist")
	}

	history := filepath.Join(filepath.Dir(path), "history.jsonl")
	core.RecordHistory("arbitrary_clean", []string{filepath.Join(home, "a"), "中文目录"}, 3, 1, 2, 12345)
	if runtime.GOOS != "windows" {
		if err := os.Chmod(history, 0644); err != nil {
			t.Fatal(err)
		}
	}
	core.RecordHistory("residual_clean", nil, 0, 4, 1, 678)
	if runtime.GOOS != "windows" {
		info, err := os.Stat(history)
		if err != nil || info.Mode().Perm() != 0600 {
			t.Fatalf("history permissions: %v %v", info, err)
		}
	}
	data, err := os.ReadFile(history)
	if err != nil {
		t.Fatal(err)
	}
	lines := bytes.Split(bytes.TrimSuffix(data, []byte("\n")), []byte("\n"))
	if len(lines) != 2 {
		t.Fatal("history must append complete JSON lines")
	}
	var first, second core.HistoryEntry
	if json.Unmarshal(lines[0], &first) != nil || json.Unmarshal(lines[1], &second) != nil {
		t.Fatal("invalid history JSON")
	}
	if first.Action != "arbitrary_clean" || first.OK != 3 || first.Skipped != 1 || first.Failed != 2 || first.FreedBytes != 12345 || len(first.Targets) != 2 || first.Targets[1] != "中文目录" || second.Action != "residual_clean" || second.FreedBytes != 678 || second.Targets == nil {
		t.Fatalf("history payload: %+v / %+v", first, second)
	}
	if stamp, err := time.Parse(time.RFC3339, first.Timestamp); err != nil || time.Since(stamp) > time.Minute {
		t.Fatalf("history local timestamp: %q %v", first.Timestamp, err)
	}
	// Each line is 128 bytes; exactly 1 MiB must NOT rotate until the next call.
	line := []byte(`{"padding":"` + strings.Repeat("x", 113) + "\"}\n")
	if len(line) != 128 {
		t.Fatal("incorrect fixture line size", len(line))
	}
	existing := bytes.Repeat(line, 8192)
	if err := os.WriteFile(history, existing, 0600); err != nil {
		t.Fatal(err)
	}
	core.RecordHistory("boundary", nil, 1, 0, 0, 1)
	data, err = os.ReadFile(history)
	if err != nil || !bytes.HasPrefix(data, existing) {
		t.Fatal("rotation happened at the threshold instead of above it", err)
	}
	core.RecordHistory("rotated", nil, 2, 0, 0, 2)
	data, err = os.ReadFile(history)
	if err != nil || len(data) >= 600*1024 || !bytes.HasPrefix(data, []byte("{")) || !bytes.Contains(data, []byte(`"action":"boundary"`)) || !bytes.Contains(data, []byte(`"action":"rotated"`)) {
		t.Fatal("rotation lost recent events or kept oversized history", err)
	}
	for _, line := range bytes.Split(bytes.TrimSuffix(data, []byte("\n")), []byte("\n")) {
		if !json.Valid(line) {
			t.Fatal("rotation left a partial JSON line")
		}
	}
}
