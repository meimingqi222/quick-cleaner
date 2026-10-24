package core

import (
	"fmt"
	"runtime"
	"strconv"
	"strings"
)

const snapshotPrefix = "tmutil://snapshot/"
const dockerPrefix = "docker://image/"

// Virtual paths carry opaque command arguments, not filesystem paths.
func SnapshotPath(name string) string   { return snapshotPrefix + name }
func DockerImagePath(ref string) string { return dockerPrefix + ref }
func IsVirtualPath(path string) bool {
	return strings.HasPrefix(path, snapshotPrefix) || strings.HasPrefix(path, dockerPrefix) || path == "brew://cleanup"
}
func SnapshotName(path string) (string, bool) { return strings.CutPrefix(path, snapshotPrefix) }
func DockerRmiRef(path string) (string, bool) { return strings.CutPrefix(path, dockerPrefix) }

type Check uint8

const (
	Off Check = iota
	On
	Partial
)

func CheckFromCounts(selected, total uint) Check {
	if total == 0 || selected == 0 {
		return Off
	}
	if selected >= total {
		return On
	}
	return Partial
}

// FmtSize follows Explorer's binary disk units on Windows and SI elsewhere.
func FmtSize(bytes uint64) string {
	base := float64(1000)
	if runtime.GOOS == "windows" {
		base = 1024
	}
	return formatBytes(bytes, base)
}

// FmtMem always uses binary units, independently of the host's disk convention.
func FmtMem(bytes uint64) string { return formatBytes(bytes, 1024) }

func formatBytes(bytes uint64, base float64) string {
	b := float64(bytes)
	kb, mb, gb, tb := base, base*base, base*base*base, base*base*base*base
	switch {
	case b >= tb:
		return fmt.Sprintf("%.2f TB", b/tb)
	case b >= gb:
		return fmt.Sprintf("%.2f GB", b/gb)
	case b >= mb:
		return fmt.Sprintf("%.1f MB", b/mb)
	case b >= kb:
		return fmt.Sprintf("%.0f KB", b/kb)
	default:
		return strconv.FormatUint(bytes, 10) + " B"
	}
}

func Commas(n uint64) string {
	digits := strconv.FormatUint(n, 10)
	var out strings.Builder
	for i, c := range digits {
		if i > 0 && (len(digits)-i)%3 == 0 {
			out.WriteByte(',')
		}
		out.WriteRune(c)
	}
	return out.String()
}

// Truncate counts Unicode scalar values, not bytes or grapheme clusters.
func Truncate(s string, n uint) string {
	var count uint
	for i := range s {
		if count == n {
			return s[:i] + "…"
		}
		count++
	}
	return s
}
