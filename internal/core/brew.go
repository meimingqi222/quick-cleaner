package core

import (
	"errors"
	"math"
	"os"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"
)

const BrewThrottleDays = 7

func brewExecutable() string {
	// Do not discover user wrappers through PATH; match the native prefixes.
	for _, path := range []string{"/opt/homebrew/bin/brew", "/usr/local/bin/brew"} {
		if _, err := os.Lstat(path); err == nil {
			return path
		}
	}
	return ""
}

func ShouldOfferBrewCleanup(last *int64) bool {
	if last == nil {
		return true
	}
	now := time.Now().Unix()
	return now >= *last && uint64(now)-uint64(*last) >= BrewThrottleDays*86400
}

func parseBrewHumanSize(text string) (uint64, bool) {
	text = strings.TrimSpace(text)
	i := strings.IndexFunc(text, func(c rune) bool { return c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' })
	if i < 0 {
		return 0, false
	}
	value, err := strconv.ParseFloat(text[:i], 64)
	if err != nil {
		return 0, false
	}
	multiplier, ok := map[string]float64{"B": 1, "KB": 1e3, "MB": 1e6, "GB": 1e9, "TB": 1e12}[text[i:]]
	if !ok {
		return 0, false
	}
	value *= multiplier
	if value >= math.MaxUint64 {
		return math.MaxUint64, true
	}
	if value <= 0 {
		return 0, true
	}
	return uint64(value), true
}

func ParseBrewCleanupOutput(stdout string) (bytes, files uint64, present bool) {
	var hasSize bool
	for line := range strings.SplitSeq(stdout, "\n") {
		line = strings.TrimSuffix(line, "\r")
		if !hasSize {
			if _, rest, ok := strings.Cut(line, "approximately "); ok {
				if size, ok := strings.CutSuffix(rest, " of disk space."); ok {
					bytes, hasSize = parseBrewHumanSize(size)
				}
			}
		}
		if strings.HasPrefix(line, "Would remove") || strings.HasPrefix(line, "Prune:") || strings.HasPrefix(line, "Removing:") {
			files++
		}
	}
	if hasSize && bytes > 0 {
		return bytes, max(files, 1), true
	}
	if !hasSize && files > 0 {
		return 0, files, true
	}
	return 0, 0, false
}

func BrewScanTargets(s *RuleSnapshot, lastCleanup *int64) []ScanTarget {
	if !ShouldOfferBrewCleanup(lastCleanup) {
		return nil
	}
	exe := brewExecutable()
	if exe == "" {
		return nil
	}
	preview, ok := RunWithTimeout(exe, []string{"cleanup", "-n"}, 15*time.Second)
	if !ok || !preview.OK {
		return nil
	}
	bytes, _, present := ParseBrewCleanupOutput(strings.ToValidUTF8(string(preview.Stdout), "�"))
	if !present {
		return nil
	}
	target := ProviderTarget(s, "engine", "brew_cleanup", "brew://cleanup", NewText("Homebrew 清理", "Homebrew cleanup"), PackageCache, Operation{Kind: OpBrew}, true)
	target.SizeHint = &bytes
	return []ScanTarget{target}
}

func brewCleanupAction() error {
	exe := brewExecutable()
	if exe == "" {
		return errors.New("Homebrew executable unavailable")
	}
	result, ok := RunWithTimeout(exe, []string{"cleanup"}, 60*time.Second)
	if !ok {
		return errors.New("Homebrew cleanup unavailable or timed out")
	}
	if !result.OK {
		return errors.New(strings.TrimSpace(string(result.Stderr)))
	}
	return nil
}

func brewCleanupCompletion() Evidence {
	exe := brewExecutable()
	if exe == "" {
		return Unknown
	}
	result, ok := RunWithTimeout(exe, []string{"cleanup", "-n"}, 15*time.Second)
	if !ok || !result.OK {
		return Unknown
	}
	return BrewPreviewCompletion(result.Stdout)
}

func BrewPreviewCompletion(stdout []byte) Evidence {
	if !utf8.Valid(stdout) {
		return Unknown
	}
	if _, _, present := ParseBrewCleanupOutput(string(stdout)); present {
		return Absent
	}
	unknown := false
	for line := range strings.SplitSeq(string(stdout), "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "Would remove") || strings.HasPrefix(line, "Prune:") || strings.HasPrefix(line, "Removing:") {
			return Absent
		}
		if line == "" || line == "Pruning symlinks..." {
			continue
		}
		summary := strings.TrimPrefix(line, "==> ")
		empty := false
		for _, prefix := range []string{"This operation would free approximately ", "This operation has freed approximately "} {
			if size, ok := strings.CutPrefix(summary, prefix); ok {
				if size, ok = strings.CutSuffix(size, " of disk space."); ok {
					bytes, valid := parseBrewHumanSize(size)
					i := strings.IndexFunc(size, func(c rune) bool { return c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' })
					if valid && bytes == 0 && i >= 0 {
						value, err := strconv.ParseFloat(size[:i], 64)
						empty = err == nil && value == 0
					}
				}
			}
		}
		if !empty {
			unknown = true
		}
	}
	if unknown {
		return Unknown
	}
	return Confirmed
}

func recordBrewCleanup() {
	settings := LoadSettings()
	now := time.Now().Unix()
	settings.BrewCleanupAt = &now
	settings.Save()
}
