package core

import (
	"cmp"
	"os"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"time"

	"golang.org/x/text/cases"
	"golang.org/x/text/language"
)

type AppRegRoot uint8

const (
	AppHKLM AppRegRoot = iota
	AppHKLM32
	AppHKCU
	AppSystem
	AppUnregistered
)

func (r AppRegRoot) Label() string {
	return [...]string{"HKLM", "HKLM32", "HKCU", "SystemApp", "Unregistered"}[r]
}
func (r AppRegRoot) LabelLang(l Language) string {
	return [...]Text{NewText("HKLM (64位)", "HKLM (64-bit)"), NewText("HKLM (32位)", "HKLM (32-bit)"), NewText("HKCU (当前用户)", "HKCU (Current User)"), NewText("系统/UWP", "System / UWP"), NewText("未登记应用", "Unregistered app")}[r].Get(l)
}

type AppDiscovery struct {
	Plan                    *CleanupPlan
	Rule                    *RuleRef
	Executable              string
	Shortcuts, ProgramPaths []string
	Uninstaller             *OfficialUninstaller
}
type UninstallOutcome struct {
	Result         error
	PlanExecutions []PlanExecution
}
type InstalledApp struct {
	Discovery                                                           *AppDiscovery
	ID, Name, Version, Publisher                                        string
	LastUsedDate, InstallDate                                           *string
	LastUsedRaw, InstallDateRaw                                         uint64
	InstallLocation, DisplayIcon, UninstallString, QuietUninstallString *string
	EstimatedSize                                                       uint64
	RegistryRoot                                                        AppRegRoot
	RegistrySubpath                                                     string
	IsSystemComponent, UninstallerMissing                               bool
}

func (a InstalledApp) CanUninstall() bool {
	if a.IsSystemComponent {
		return false
	}
	if runtime.GOOS == "darwin" {
		return true
	}
	if a.Discovery != nil {
		return a.Discovery.Uninstaller != nil && !a.UninstallerMissing
	}
	return (a.UninstallString != nil || a.QuietUninstallString != nil) && !a.UninstallerMissing
}
func (a InstalledApp) IconCacheKey() (string, bool) {
	if a.DisplayIcon != nil {
		if p, ok := DisplayIconPath(*a.DisplayIcon); ok {
			return p, true
		}
	}
	if a.InstallLocation != nil {
		return *a.InstallLocation, true
	}
	return "", false
}
func DisplayIconPath(raw string) (string, bool) {
	s := strings.Trim(strings.TrimSpace(raw), "\"")
	if i := strings.LastIndexByte(s, ','); i >= 0 {
		suffix := strings.TrimSpace(s[i+1:])
		if suffix != "" && strings.Trim(suffix, "0123456789-") == "" {
			s = strings.Trim(strings.TrimSpace(s[:i]), "\"")
		}
	}
	s = strings.TrimSpace(s)
	return s, s != ""
}

type AppSortColumn string

const (
	AppSortName        AppSortColumn = "name"
	AppSortPublisher   AppSortColumn = "publisher"
	AppSortLastUsed    AppSortColumn = "last-used"
	AppSortInstallDate AppSortColumn = "install-date"
	AppSortSize        AppSortColumn = "size"
)

func (c AppSortColumn) ID() string { return string(c) }

type SortDirection uint8

const (
	Ascending SortDirection = iota
	Descending
)

type AppSortState struct {
	Column    AppSortColumn
	Direction SortDirection
}

func DefaultAppSortState() AppSortState { return AppSortState{AppSortSize, Descending} }
func (s *AppSortState) Toggle(c AppSortColumn) {
	if s.Column == c {
		if s.Direction == Ascending {
			s.Direction = Descending
		} else {
			s.Direction = Ascending
		}
	} else {
		s.Column = c
		s.Direction = Descending
		if c == AppSortName || c == AppSortPublisher {
			s.Direction = Ascending
		}
	}
}
func (s AppSortState) Indicator(c AppSortColumn) string {
	if s.Column != c {
		return ""
	}
	if s.Direction == Ascending {
		return " ▲"
	}
	return " ▼"
}

type AppFilterPreset uint8

const (
	AppFilterAll AppFilterPreset = iota
	AppFilterLarge
	AppFilterUnused
	AppFilterOrphan
)
const RarelyUsedSecs uint64 = 90 * 86400

func NowUnixSecs() uint64 {
	n := time.Now().Unix()
	if n < 0 {
		return 0
	}
	return uint64(n)
}
func IsRarelyUsed(a InstalledApp, now uint64) bool {
	return a.LastUsedRaw == 0 || now > a.LastUsedRaw && now-a.LastUsedRaw > RarelyUsedSecs
}
func (p AppFilterPreset) Label() string { return p.LabelLang(Chinese) }
func (p AppFilterPreset) LabelLang(l Language) string {
	return [...]Text{NewText("全部软件", "All Apps"), NewText("大型软件 (>500MB)", "Large Apps (>500MB)"), NewText("长期未用 (>90天)", "Rarely Used (>90d)"), NewText("卸载器失效", "Invalid Uninstaller")}[p].Get(l)
}
func (p AppFilterPreset) Matches(a InstalledApp, now uint64) bool {
	switch p {
	case AppFilterLarge:
		return a.EstimatedSize >= 500*1024*1024
	case AppFilterUnused:
		return IsRarelyUsed(a, now)
	case AppFilterOrphan:
		if a.Discovery != nil {
			return a.UninstallerMissing
		}
		return a.UninstallerMissing || a.UninstallString == nil && a.QuietUninstallString == nil
	}
	return true
}
func FilterAndSortApps(apps []InstalledApp, p AppFilterPreset, keyword string, s AppSortState) []int {
	lower := cases.Lower(language.Und)
	kw := lower.String(strings.TrimSpace(keyword))
	now := NowUnixSecs()
	out := []int{}
	for i, a := range apps {
		if p.Matches(a, now) && (kw == "" || strings.Contains(lower.String(a.Name), kw) || strings.Contains(lower.String(a.Publisher), kw) || strings.Contains(lower.String(a.ID), kw)) {
			out = append(out, i)
		}
	}
	// Rust's comparator lowers individual chars, unlike its search strings.
	charLower := cases.Lower(language.Und, cases.HandleFinalSigma(false))
	ci := func(a, b string) int { return strings.Compare(charLower.String(a), charLower.String(b)) }
	sort.SliceStable(out, func(i, j int) bool {
		a, b := apps[out[i]], apps[out[j]]
		n := 0
		timed := false
		switch s.Column {
		case AppSortName:
			n = ci(a.Name, b.Name)
		case AppSortPublisher:
			n = ci(a.Publisher, b.Publisher)
			if n == 0 {
				n = ci(a.Name, b.Name)
			}
		case AppSortSize:
			n = cmp.Compare(a.EstimatedSize, b.EstimatedSize)
		case AppSortLastUsed, AppSortInstallDate:
			timed = true
			x, y := a.LastUsedRaw, b.LastUsedRaw
			if s.Column == AppSortInstallDate {
				x, y = a.InstallDateRaw, b.InstallDateRaw
			}
			if x == 0 && y != 0 {
				n = 1
			} else if y == 0 && x != 0 {
				n = -1
			} else {
				n = cmp.Compare(x, y)
				if s.Direction == Descending {
					n = -n
				}
			}
		}
		if !timed && s.Direction == Descending {
			n = -n
		}
		if n == 0 {
			n = ci(a.Name, b.Name)
		}
		return n < 0
	})
	return out
}
func IsSafeAppToken(name string) bool {
	s := cases.Lower(language.Und).String(strings.TrimSpace(name))
	if len(s) < 3 {
		return false
	}
	switch s {
	case "app", "apps", "microsoft", "windows", "system", "system32", "program files", "program files (x86)", "common files", "appdata", "local", "roaming", "locallow", "programdata", "temp", "tmp", "users", "google", "apple", "intel", "amd", "nvidia", "adobe", "tencent":
		return false
	}
	return true
}

// Backslashes are literal; this intentionally is not a shell parser.
func ParseCmdLine(cmd string) []string {
	out := []string{}
	var b strings.Builder
	q := false
	flush := func() {
		if b.Len() > 0 {
			out = append(out, b.String())
			b.Reset()
		}
	}
	for _, c := range cmd {
		switch {
		case c == '"':
			q = !q
		case (c == ' ' || c == '\t') && !q:
			flush()
		default:
			b.WriteRune(c)
		}
	}
	flush()
	return out
}
func SplitCommandWith(cmd string, isFile func(string) bool) (string, []string) {
	cmd = strings.TrimSpace(cmd)
	if cmd == "" {
		return "", nil
	}
	if strings.HasPrefix(cmd, "\"") {
		rest := cmd[1:]
		if end := strings.IndexByte(rest, '"'); end >= 0 {
			exe := rest[:end]
			ext := false
			for _, e := range []string{".exe", ".com", ".bat", ".cmd", ".msi", ".scr", ".ps1"} {
				ext = ext || strings.HasSuffix(strings.ToLower(exe), e)
			}
			if isFile(exe) || ext {
				return exe, ParseCmdLine(strings.TrimSpace(rest[end+1:]))
			}
		}
	}
	tokens := []string{}
	for _, t := range strings.Split(strings.Trim(cmd, "\""), " ") {
		if t != "" {
			tokens = append(tokens, t)
		}
	}
	for n := 1; n <= len(tokens); n++ {
		candidate := strings.Join(tokens[:n], " ")
		if isFile(candidate) {
			return candidate, append([]string{}, tokens[n:]...)
		}
	}
	if len(tokens) == 0 {
		return "", nil
	}
	return tokens[0], tokens[1:]
}
func SplitCommand(cmd string) (string, []string) {
	return SplitCommandWith(cmd, func(p string) bool { md, err := os.Stat(p); return err == nil && md.Mode().IsRegular() })
}
func ParseInstallDate(raw *string) (*string, uint64) {
	if raw == nil {
		return nil, 0
	}
	s := strings.TrimSpace(*raw)
	digits := strings.Map(func(r rune) rune {
		if r >= '0' && r <= '9' {
			return r
		}
		return -1
	}, s)
	var n uint64
	if len(s) >= 8 {
		var err error
		n, err = strconv.ParseUint(digits, 10, 64)
		if err != nil {
			n = 0
		}
	}
	if len(s) == 8 && digits == s {
		s = s[:4] + "-" + s[4:6] + "-" + s[6:]
	}
	return &s, n
}
