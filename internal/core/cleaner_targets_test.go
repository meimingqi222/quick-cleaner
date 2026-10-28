package core

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestLaunchAgentPlistPredicate(t *testing.T) {
	home := "/var/lib/qc-home"
	winHome := "D:" + `\fixture\a`
	cases := []struct {
		path, home string
		want       bool
	}{
		{winHome + `\Library\LaunchAgents\com.example.broken.plist`, winHome, true},
		{home + "/Library/LaunchAgents/com.example.broken.PLIST", home, true},
		{"/Library/LaunchAgents/com.example.broken.plist", "", true},
		{"/Library/LaunchAgents/com.example.broken.plist", home, true},
		{home + "/Library/LaunchAgents/notes.txt", home, false},
		{"/Library/LaunchDaemons/com.example.plist", home, false},
		{"/Library/LaunchAgents/nested/com.example.plist", home, false},
		{"/Library/LaunchAgents/../LaunchAgents/com.example.plist", home, false},
		{home + "/Library/LaunchAgents/.plist", home, false},
		{home + "/Library/LaunchAgents/com.example.plist", "", false},
		{"com.example.plist", home, false},
	}
	for _, tc := range cases {
		if got := launchAgentPlist(tc.path, tc.home); got != tc.want {
			t.Errorf("launchAgentPlist(%q, %q) = %v, want %v", tc.path, tc.home, got, tc.want)
		}
	}
}

func TestSelectedScanTargetsCannotChangeDisposalOrIdentity(t *testing.T) {
	root := t.TempDir()
	path := filepath.Join(root, "cache")
	if err := os.Mkdir(path, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(path, "sentinel"), []byte("retain me"), 0600); err != nil {
		t.Fatal(err)
	}
	snapshot, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	target := ProviderTarget(snapshot, "engine", "cache_candidate", path, SameText("cache"), UserCache, Operation{Kind: OpTree}, true)
	cats := ScanFixed(context.Background(), []ScanTarget{target}, nil)
	var selected CleanTarget
	for _, cat := range cats {
		if len(cat.Items) > 0 {
			selected = cat.Items[0].CleanTarget()
		}
	}
	if selected.Identity == nil || len(selected.Plans) != 1 {
		t.Fatal("selection did not carry scan authority")
	}
	conflict := selected
	conflict.Disposal = RecycleBin
	merged := MergeCleanTargets([]CleanTarget{selected, conflict})
	if len(merged) != 1 {
		t.Fatalf("merged count = %d", len(merged))
	}
	if report := CleanTargets([]CleanTarget{selected, conflict}, &CleanProgress{}); len(report.Failed) != 1 || len(report.FailInfo) != 1 || report.FailInfo[0].Reason != FailUnverified || len(report.RemainingLive) != 1 || report.RemainingLive[0].FileCount != 1 {
		t.Fatalf("conflicting selection = %+v", report)
	}
	if _, err := os.Stat(filepath.Join(path, "sentinel")); err != nil {
		t.Fatal("conflicting selection mutated disk")
	}
	if err := os.Rename(path, path+"-old"); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(path, 0700); err != nil {
		t.Fatal(err)
	}
	if report := CleanTargets([]CleanTarget{selected}, &CleanProgress{}); len(report.Failed) != 1 {
		t.Fatalf("replacement selection = %+v", report)
	}
	if _, err := os.Stat(path); err != nil {
		t.Fatal("replacement was removed")
	}
	cancel := &CleanProgress{}
	cancel.RequestCancel()
	if report := CleanTargets([]CleanTarget{selected}, cancel); len(report.Failed) != 0 || report.OK != 0 || len(report.RemainingLive) != 0 {
		t.Fatalf("cancelled cleanup = %+v", report)
	}
}
