package core

import (
	"errors"
	"strings"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// AppOwner names the application whose cache a target is. It is display and
// consent data only: it never grants deletion, and closing an owner still
// goes through the cache-release identity checks.
type AppOwner struct {
	Name      Text
	Processes []string
}

const ownerProcessesPrefix = "owner_processes_"

func ownerProcessesKey(en string) string { return ownerProcessesPrefix + strings.ToLower(en) }

// browserOwner reads the declared process names for a browsers catalog row.
func browserOwner(s *RuleSnapshot, row Layout) *AppOwner {
	names := s.List("browsers", ownerProcessesKey(row.En))
	if len(names) == 0 {
		return nil
	}
	return &AppOwner{Name: NewText(row.Zh, row.En), Processes: names}
}

// validateOwnerProcesses keeps owner lists bound to an existing Windows
// browser row and to bare image names, so a list can never name a path or a
// process the rule does not describe.
func validateOwnerProcesses(r *RuleDefinition) error {
	rows := map[string]bool{}
	for _, row := range r.Catalogs["windows_user_data"] {
		rows[ownerProcessesKey(row.En)] = true
	}
	for key, names := range r.Lists {
		if !strings.HasPrefix(key, ownerProcessesPrefix) {
			continue
		}
		if r.ID != "browsers" || !rows[key] || len(names) == 0 || len(names) > 8 {
			return errors.New("invalid owner processes")
		}
		for _, name := range names {
			if len(name) > 64 || !strings.HasSuffix(strings.ToLower(name), ".exe") || strings.Trim(name, "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-") != "" {
				return errors.New("invalid owner process name")
			}
		}
	}
	return nil
}

// RunningApp is one owner application with live processes this user may ask
// to quit, plus the selected targets that belong to it.
type RunningApp struct {
	Name      Text
	Processes []string
	Live      []platform.CacheOccupant
	Paths     []string
	Bytes     uint64
}

var runningOwnerProcesses = platform.RunningAppOwners

// FindRunningApps groups items by owner and keeps owners with a live,
// closable process. System, other-user and critical processes are never
// returned; their files are simply skipped by the per-file cleaner.
func FindRunningApps(items []ScanItem) []RunningApp {
	var order []string
	apps := map[string]*RunningApp{}
	var names []string
	for _, item := range items {
		if item.Owner == nil || len(item.Owner.Processes) == 0 {
			continue
		}
		key := item.Owner.Name.Get(English)
		app := apps[key]
		if app == nil {
			app = &RunningApp{Name: item.Owner.Name, Processes: item.Owner.Processes}
			apps[key] = app
			order = append(order, key)
			names = append(names, item.Owner.Processes...)
		}
		app.Paths = append(app.Paths, item.Path)
		app.Bytes = saturatingAdd(app.Bytes, item.Size)
	}
	if len(order) == 0 {
		return nil
	}
	live, err := runningOwnerProcesses(names)
	if err != nil {
		return nil
	}
	var out []RunningApp
	for _, key := range order {
		app := apps[key]
		for _, owner := range live {
			if processNamed(owner.Name, app.Processes) {
				app.Live = append(app.Live, owner)
			}
		}
		if len(app.Live) > 0 {
			out = append(out, *app)
		}
	}
	return out
}

func processNamed(name string, names []string) bool {
	for _, n := range names {
		if strings.EqualFold(n, name) {
			return true
		}
	}
	return false
}

func anyLive(live []platform.CacheOccupant, names []string) bool {
	for _, owner := range live {
		if processNamed(owner.Name, names) {
			return true
		}
	}
	return false
}

// RunningOwnerBusy is the scan-time badge: cheap, process-list only.
func RunningOwnerBusy(targets []ScanTarget) map[string]Busy {
	var names []string
	for _, t := range targets {
		if t.Owner != nil {
			names = append(names, t.Owner.Processes...)
		}
	}
	out := map[string]Busy{}
	if len(names) == 0 {
		return out
	}
	live, err := runningOwnerProcesses(names)
	if err != nil {
		return out
	}
	for _, t := range targets {
		if t.Owner != nil && anyLive(live, t.Owner.Processes) {
			out[t.Path] = Busy{App: t.Owner.Name.Get(English)}
		}
	}
	return out
}

var requestAppExit = func(owner platform.CacheOccupant) error { return platform.RequestCacheOwnerExit(owner, false) }

// CloseRunningApps asks each app to quit normally and waits up to wait for
// its processes to go away. It never force-terminates: files an app still
// holds are skipped by the per-file cleaner. Consent is per application, so
// processes are re-listed fresh; each exit request still re-verifies identity.
// It returns the apps that were still running at the deadline.
func CloseRunningApps(apps []RunningApp, wait time.Duration, cancelled func() bool) []Text {
	var names []string
	for _, app := range apps {
		names = append(names, app.Processes...)
	}
	if len(names) == 0 {
		return nil
	}
	if live, err := runningOwnerProcesses(names); err == nil {
		for _, owner := range live {
			// A child process without windows or one that already exited
			// is not a failure; the deadline check below is the verdict.
			_ = requestAppExit(owner)
		}
	}
	deadline := time.Now().Add(wait)
	for {
		live, err := runningOwnerProcesses(names)
		var still []Text
		for _, app := range apps {
			// An unreadable process list is not evidence that an app quit.
			if err != nil || anyLive(live, app.Processes) {
				still = append(still, app.Name)
			}
		}
		if len(still) == 0 || !time.Now().Before(deadline) || cancelled != nil && cancelled() {
			return still
		}
		time.Sleep(min(300*time.Millisecond, time.Until(deadline)))
	}
}
