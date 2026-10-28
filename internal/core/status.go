package core

import (
	"fmt"
	"math"
	"strconv"
	"strings"
	"unicode"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
	"golang.org/x/text/cases"
	"golang.org/x/text/language"
)

type FanInfo = platform.FanInfo

type FanModeKind = platform.FanModeKind

const (
	FanAuto    = platform.FanAuto
	FanPercent = platform.FanPercent
)

type FanMode = platform.FanMode
type FanErrorKind = platform.FanErrorKind

const (
	FanNeedsRoot    = platform.FanNeedsRoot
	FanCanceled     = platform.FanCanceled
	FanNotInstalled = platform.FanNotInstalled
	FanNeedsUpgrade = platform.FanNeedsUpgrade
	FanOther        = platform.FanOther
)

type FanError = platform.FanError

type GpuReading = platform.GpuReading

func GPULabels(gpus []GpuReading) []string {
	labels := make([]string, len(gpus))
	seen := make(map[string]bool, len(gpus))
	collision := false
	for i, gpu := range gpus {
		label := fmt.Sprintf("GPU %d", i+1)
		if gpu.Name != nil {
			if words := strings.Fields(*gpu.Name); len(words) > 0 {
				runes := []rune(words[0])
				label = string(runes[:min(10, len(runes))])
			}
		}
		collision = collision || seen[label]
		seen[label] = true
		labels[i] = label
	}
	if collision {
		for i := range labels {
			labels[i] = fmt.Sprintf("GPU %d", i+1)
		}
	}
	return labels
}

type BatteryReading = platform.BatteryReading

type ThermalReading = platform.ThermalReading

type ProcInfo struct {
	PID              uint32
	StartTime        uint64
	UniqueID         *uint64
	Name             string
	CPU              float32
	MemBytes         uint64
	IconSource       *string
	Ports            []uint16
	Threads          uint32
	CPUTimeMS        uint64
	User             *string
	ExePath          *string
	VirtualMemBytes  uint64
	DiskReadBytes    uint64
	DiskWrittenBytes uint64
	DiskReadBPS      float64
	DiskWrittenBPS   float64
	ParentPID        *uint32
	ParentName       *string
	CmdLine          *string
	StatusText       string
}

// Use this constructor for Rust's nonzero defaults; ProcInfo{} remains ordinary Go zero data.
func DefaultProcInfo() ProcInfo { return ProcInfo{Threads: 1, StatusText: "Running"} }

func FmtCPUTime(ms uint64) string {
	secs := ms / 1000
	if hours := secs / 3600; hours > 0 {
		return fmt.Sprintf("%d:%02d:%02d", hours, secs%3600/60, secs%60)
	}
	return fmt.Sprintf("%d:%02d.%02d", secs/60, secs%60, ms%1000/10)
}

type ProcFilterTab uint8

const (
	ProcFilterAll ProcFilterTab = iota
	ProcFilterApp
	ProcFilterUser
	ProcFilterSystem
)

func AllProcFilterTabs() [4]ProcFilterTab {
	return [4]ProcFilterTab{ProcFilterAll, ProcFilterApp, ProcFilterUser, ProcFilterSystem}
}

func statusLower(s string) string { return cases.Lower(language.Und).String(s) }

func MatchesProcTab(proc ProcInfo, tab ProcFilterTab, currentUser *string) bool {
	systemUser := false
	user := ""
	if proc.User != nil {
		user = statusLower(*proc.User)
		systemUser = strings.Contains(user, "system") || strings.Contains(user, "local service") ||
			strings.Contains(user, "network service") || user == "root" || strings.HasPrefix(user, "_")
	}
	path := ""
	if proc.ExePath != nil {
		path = statusLower(*proc.ExePath)
	}
	switch tab {
	case ProcFilterAll:
		return true
	case ProcFilterApp:
		return proc.IconSource != nil || strings.Contains(path, "program files") ||
			strings.Contains(path, "appdata") || strings.Contains(path, "/applications/")
	case ProcFilterUser:
		if proc.User == nil {
			return proc.PID > 100 && proc.IconSource != nil
		}
		return !systemUser && (currentUser == nil || strings.Contains(user, statusLower(*currentUser)))
	case ProcFilterSystem:
		return proc.PID <= 4 || systemUser || strings.Contains(path, `\system32\`) || strings.Contains(path, "/system/")
	default:
		return false
	}
}

func MatchProcQuery(proc ProcInfo, query string) bool {
	clauses := splitStatusQueryClauses(strings.TrimSpace(query))
	if len(clauses) == 0 {
		return true
	}
	for _, clause := range clauses {
		if matchStatusClause(proc, clause) {
			return true
		}
	}
	return false
}

func splitStatusQueryClauses(input string) []string {
	var clauses []string
	var current strings.Builder
	quoted := false
	flush := func() {
		s := strings.TrimSpace(strings.Trim(strings.TrimSpace(current.String()), `"`))
		if s != "" {
			clauses = append(clauses, s)
		}
		current.Reset()
	}
	for _, ch := range input {
		switch {
		case ch == '"':
			quoted = !quoted
		case (ch == ',' || ch == ';') && !quoted:
			flush()
		default:
			current.WriteRune(ch)
		}
	}
	flush()
	return clauses
}

func statusPort(s string) (uint16, bool) {
	// Rust's unsigned parser accepts a leading plus, unlike strconv.ParseUint.
	s = strings.TrimPrefix(s, "+")
	n, err := strconv.ParseUint(s, 10, 16)
	return uint16(n), err == nil
}

func matchStatusClause(proc ProcInfo, clause string) bool {
	c := strings.TrimSpace(clause)
	folded := asciiLower(c)
	name := statusLower(proc.Name)
	path := ""
	if proc.ExePath != nil {
		path = statusLower(*proc.ExePath)
	}
	containsPort := func(port uint16) bool {
		for _, p := range proc.Ports {
			if p == port {
				return true
			}
		}
		return false
	}
	if rest, ok := strings.CutPrefix(c, "-"); ok {
		if neg := strings.TrimSpace(rest); neg != "" {
			neg = statusLower(neg)
			return !strings.Contains(name, neg) && !strings.Contains(path, neg)
		}
	}
	if rest, ok := strings.CutPrefix(c, ":"); ok {
		rest = strings.TrimSpace(rest)
		if port, valid := statusPort(rest); valid {
			return containsPort(port)
		} else if rest != "" {
			for _, port := range proc.Ports {
				if strings.HasPrefix(strconv.Itoa(int(port)), rest) {
					return true
				}
			}
			return false
		}
	}
	if target, ok := strings.CutPrefix(folded, "user:"); ok {
		return proc.User != nil && strings.Contains(statusLower(*proc.User), statusLower(strings.TrimSpace(target)))
	}
	if rest, ok := strings.CutPrefix(folded, "cpu"); ok {
		if op, number, valid := splitStatusComparison(rest); valid {
			number = strings.TrimSpace(strings.TrimRight(strings.TrimSpace(number), "%"))
			if val, valid := statusNumber(number, 32); valid && val >= 0 {
				v := float32(val)
				switch op {
				case ">":
					return proc.CPU > v
				case ">=":
					return proc.CPU >= v
				case "<":
					return proc.CPU < v
				case "<=":
					return proc.CPU <= v
				}
			}
		}
	}
	if rest, ok := strings.CutPrefix(folded, "mem"); ok {
		if op, bytes, valid := parseStatusMemory(rest); valid {
			switch op {
			case ">":
				return proc.MemBytes > bytes
			case ">=":
				return proc.MemBytes >= bytes
			case "<":
				return proc.MemBytes < bytes
			case "<=":
				return proc.MemBytes <= bytes
			}
		}
	}
	lower := statusLower(c)
	if strings.Contains(name, lower) || strconv.FormatUint(uint64(proc.PID), 10) == c || strings.Contains(path, lower) && proc.ExePath != nil {
		return true
	}
	port, valid := statusPort(c)
	return valid && containsPort(port)
}

func splitStatusComparison(s string) (string, string, bool) {
	s = strings.TrimSpace(s)
	for _, op := range []string{">=", "<=", ">", "<"} {
		if rest, ok := strings.CutPrefix(s, op); ok {
			return op, rest, true
		}
	}
	return "", "", false
}

func statusNumber(s string, bits int) (float64, bool) {
	// Go additionally accepts hexadecimal floats and digit separators; Rust does not.
	if strings.ContainsAny(s, "_xXpP") {
		return 0, false
	}
	n, err := strconv.ParseFloat(s, bits)
	return n, err == nil && !math.IsNaN(n) && !math.IsInf(n, 0)
}

func parseStatusMemory(s string) (string, uint64, bool) {
	op, number, ok := splitStatusComparison(s)
	if !ok {
		return "", 0, false
	}
	number = asciiLower(strings.TrimSpace(number))
	factor := float64(1024 * 1024)
	for _, unit := range []struct {
		suffix string
		factor float64
	}{
		{"gib", 1 << 30}, {"gb", 1 << 30}, {"g", 1 << 30},
		{"mib", 1 << 20}, {"mb", 1 << 20}, {"m", 1 << 20},
		{"kib", 1024}, {"kb", 1024}, {"k", 1024}, {"b", 1},
	} {
		if rest, found := strings.CutSuffix(number, unit.suffix); found {
			number, factor = rest, unit.factor
			break
		}
	}
	n, valid := statusNumber(strings.TrimSpace(number), 64)
	bytes := n * factor
	if !valid || math.IsInf(bytes, 0) || bytes < 0 || bytes >= float64(math.MaxUint64) {
		return "", 0, false
	}
	return op, uint64(bytes), true
}

// CompareStatusProcesses orders CPU descending, then memory descending. NaN CPU
// comparisons are equal and still use memory, as in Rust's partial_cmp fallback.
func CompareStatusProcesses(a, b ProcInfo) int {
	if a.CPU > b.CPU {
		return -1
	}
	if a.CPU < b.CPU {
		return 1
	}
	if a.MemBytes > b.MemBytes {
		return -1
	}
	if a.MemBytes < b.MemBytes {
		return 1
	}
	return 0
}

type StatusSnapshot struct {
	CPUUsage           float32
	CoreCount          int
	MemUsed            uint64
	MemTotal           uint64
	SwapUsed           uint64
	SwapTotal          uint64
	RxBPS              float64
	TxBPS              float64
	Processes          []ProcInfo
	ProcessCount       int
	Thermal            ThermalReading
	UptimeSecs         uint64
	GPUs               []GpuReading
	Battery            *BatteryReading
	OSName             string
	MemTotalLabelBytes uint64
}

type HealthInputs struct {
	DiskFreeRatio *float32
	MemUsed       uint64
	MemTotal      uint64
	SwapUsed      uint64
	CPUAvg        float32
	UptimeSecs    uint64
}

type HealthFactor uint8

const (
	HealthDisk HealthFactor = iota
	HealthSwap
	HealthMemory
	HealthCPU
	HealthUptime
)

type HealthReport struct {
	Score uint32
	Worst *HealthFactor
}

func statusPenalty(value, start, full, weight float32) float32 {
	if math.IsNaN(float64(value)) || math.IsInf(float64(value), 0) || math.Abs(float64(full-start)) < 0x1p-23 {
		return 0
	}
	return float32(min(float32(1), max(float32(0), float32(float32(value-start)/float32(full-start))))) * weight
}

func ComputeHealthReport(input HealthInputs) HealthReport {
	var disk, memRatio, swapRatio float32
	if input.DiskFreeRatio != nil {
		disk = statusPenalty(float32(1-*input.DiskFreeRatio), .75, 1, 45)
	}
	if input.MemTotal > 0 {
		memRatio = float32(input.MemUsed) / float32(input.MemTotal)
		swapRatio = float32(input.SwapUsed) / float32(input.MemTotal)
	}
	memory := statusPenalty(memRatio, .80, 1, 10)
	swap := statusPenalty(swapRatio, .01, .25, 15)
	cpu := statusPenalty(input.CPUAvg, 50, 95, 20)
	uptime := statusPenalty(float32(input.UptimeSecs)/86400, 7, 30, 10)
	// Explicit intermediate rounding forbids Go's optional fused operations and
	// preserves Rust's left-associated f32 addition order (memory before swap).
	total := float32(float32(float32(float32(disk+memory)+swap)+cpu) + uptime)
	score := min(float32(100), max(float32(0), float32(100-total)))
	report := HealthReport{Score: uint32(math.Round(float64(score)))}
	worstPenalty := float32(3)
	for _, item := range []struct {
		factor  HealthFactor
		penalty float32
	}{
		{HealthDisk, disk}, {HealthSwap, swap}, {HealthCPU, cpu}, {HealthMemory, memory}, {HealthUptime, uptime},
	} {
		// Rust Iterator::max_by selects the LAST equal maximum.
		if item.penalty >= worstPenalty {
			factor := item.factor
			report.Worst, worstPenalty = &factor, item.penalty
		}
	}
	return report
}

// ComposeProcessName is the platform-independent bundle-label composition step.
// Native bundle/path discovery remains the sampler's responsibility.
func ComposeProcessName(exeName, app string, nestedHelper *string) string {
	if app == "" {
		return exeName
	}
	contains := func(a, b string) bool {
		a, b = foldStatusLabel(a), foldStatusLabel(b)
		return a == b || b != "" && strings.Contains(a, b)
	}
	if nestedHelper != nil && *nestedHelper != "" {
		if contains(*nestedHelper, app) {
			return *nestedHelper
		}
		return app + " " + *nestedHelper
	}
	if contains(exeName, app) {
		return app
	}
	return app + " " + exeName
}

func foldStatusLabel(s string) string {
	var out strings.Builder
	// Rust char::to_lowercase is context-free, unlike str::to_lowercase.
	lower := cases.Lower(language.Und, cases.HandleFinalSigma(false))
	for _, r := range s {
		if unicode.IsLetter(r) || unicode.IsNumber(r) || unicode.Is(unicode.Other_Alphabetic, r) {
			out.WriteString(lower.String(string(r)))
		}
	}
	return out.String()
}

func ShortOSName(long string) string {
	words := strings.Fields(long)
	for i, word := range words {
		if word[0] >= '0' && word[0] <= '9' {
			return strings.Join(words[:i+1], " ")
		}
	}
	return strings.Join(words, " ")
}
