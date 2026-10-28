package core

import (
	"strings"
	"time"
	"unicode"
	"unicode/utf8"
)

func MacOSSnapshotTargets(s *RuleSnapshot) []ScanTarget {
	result, ok := RunToolWithTimeout("tmutil", []string{"listlocalsnapshots", "/"}, 5*time.Second)
	if !ok || !result.OK {
		return nil
	}
	var out []ScanTarget
	for line := range strings.SplitSeq(strings.ToValidUTF8(string(result.Stdout), "�"), "\n") {
		name := strings.TrimSpace(line)
		if name == "" || strings.ContainsFunc(name, unicode.IsSpace) {
			continue
		}
		out = append(out, ProviderTarget(s, "engine", "local_snapshot", SnapshotPath(name), NewText("本地快照 · "+name, "Local snapshot · "+name), LocalSnapshots, Operation{Kind: OpSnapshot, Name: name}, true))
	}
	return out
}

func removeSnapshotAction(name string) bool {
	date, valid := SnapshotDate(name)
	if !valid {
		return false
	}
	result, ok := RunToolWithTimeout("tmutil", []string{"deletelocalsnapshots", date}, 60*time.Second)
	return ok && result.OK
}

func snapshotAbsence(name string) Evidence {
	date, valid := SnapshotDate(name)
	if !valid {
		return Unknown
	}
	result, ok := RunToolWithTimeout("tmutil", []string{"listlocalsnapshots", "/"}, 60*time.Second)
	if !ok || !result.OK || !utf8.Valid(result.Stdout) {
		return Unknown
	}
	for line := range strings.SplitSeq(string(result.Stdout), "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "Snapshots for disk") {
			continue
		}
		if !strings.HasPrefix(line, "com.apple.") || strings.ContainsFunc(line, unicode.IsSpace) {
			return Unknown
		}
		if line == "com.apple.TimeMachine."+date+".local" {
			return Absent
		}
	}
	return Confirmed
}
