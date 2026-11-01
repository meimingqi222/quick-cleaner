package core

import (
	"errors"
	"log"
	"path/filepath"
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
	Name        Text
	Processes   []string
	Executables []string
	Live        []platform.CacheOccupant
	Paths       []string
	Bytes       uint64
}

var runningOwnerProcesses = platform.RunningAppOwners
var lockedCacheOwners = platform.FindLockedCacheOwners

func dynamicLockProbeEligible(item ScanItem) bool {
	return item.CleanupAvailable() && item.Disposal == Permanent &&
		(item.Operation.Kind == OpContents || item.Operation.Kind == OpFile)
}

// FindRunningApps groups items by declared owner and by sampled active file
// locks, returning closable same-user applications. System, other-user,
// critical, Explorer and cleaner-tree processes are never returned; their
// files are simply skipped by the per-file cleaner.
func FindRunningApps(items []ScanItem) []RunningApp {
	var declaredOrder []string
	declaredApps := map[string]*RunningApp{}
	declaredPathSeen := map[string]map[string]bool{}
	var names []string
	sizeByPath := make(map[string]uint64, len(items))
	var probePaths []string
	seenProbe := make(map[string]bool, len(items))

	for _, item := range items {
		norm := Norm(item.Path)
		sizeByPath[norm] = item.Size
		if dynamicLockProbeEligible(item) && !seenProbe[norm] {
			seenProbe[norm] = true
			probePaths = append(probePaths, item.Path)
		}
		if item.Owner == nil || len(item.Owner.Processes) == 0 {
			continue
		}
		key := item.Owner.Name.Get(English)
		app := declaredApps[key]
		if app == nil {
			app = &RunningApp{Name: item.Owner.Name, Processes: append([]string(nil), item.Owner.Processes...)}
			declaredApps[key] = app
			declaredPathSeen[key] = map[string]bool{}
			declaredOrder = append(declaredOrder, key)
			names = append(names, item.Owner.Processes...)
		}
		if !declaredPathSeen[key][norm] {
			declaredPathSeen[key][norm] = true
			app.Paths = append(app.Paths, item.Path)
			app.Bytes = saturatingAdd(app.Bytes, item.Size)
		}
	}

	var listErr error
	if len(declaredOrder) > 0 {
		var live []platform.CacheOccupant
		live, listErr = runningOwnerProcesses(names)
		for _, key := range declaredOrder {
			app := declaredApps[key]
			for _, owner := range live {
				if processNamed(owner.Name, app.Processes) {
					appendUniqueOccupant(&app.Live, owner)
				}
			}
		}
	}

	var dynamicOrder []string
	dynamicApps := map[string]*RunningApp{}
	dynamicPathSeen := map[string]map[string]bool{}
	if len(probePaths) > 0 {
		if locked, err := lockedCacheOwners(probePaths); err == nil {
			for _, hit := range locked {
				procName := hit.Owner.Name
				if procName == "" && hit.Owner.Executable != "" {
					procName = filepath.Base(hit.Owner.Executable)
				}
				normTarget := Norm(hit.Target)
				mergedDeclared := false
				for _, key := range declaredOrder {
					app := declaredApps[key]
					if processNamed(procName, app.Processes) || (hit.Owner.Executable != "" && processNamed(filepath.Base(hit.Owner.Executable), app.Processes)) {
						appendUniqueOccupant(&app.Live, hit.Owner)
						if !declaredPathSeen[key][normTarget] {
							declaredPathSeen[key][normTarget] = true
							app.Paths = append(app.Paths, hit.Target)
							app.Bytes = saturatingAdd(app.Bytes, sizeByPath[normTarget])
						}
						mergedDeclared = true
						break
					}
				}
				if mergedDeclared {
					continue
				}
				dynKey := strings.ToLower(hit.Owner.Executable)
				if dynKey == "" {
					dynKey = strings.ToLower(hit.Display)
				}
				if dynKey == "" {
					continue
				}
				app := dynamicApps[dynKey]
				if app == nil {
					display := hit.Display
					if display == "" {
						display = procName
					}
					app = &RunningApp{Name: SameText(display)}
					if procName != "" {
						app.Processes = []string{procName}
					}
					if hit.Owner.Executable != "" {
						app.Executables = []string{hit.Owner.Executable}
					}
					dynamicApps[dynKey] = app
					dynamicPathSeen[dynKey] = map[string]bool{}
					dynamicOrder = append(dynamicOrder, dynKey)
				}
				appendUniqueOccupant(&app.Live, hit.Owner)
				if !dynamicPathSeen[dynKey][normTarget] {
					dynamicPathSeen[dynKey][normTarget] = true
					app.Paths = append(app.Paths, hit.Target)
					app.Bytes = saturatingAdd(app.Bytes, sizeByPath[normTarget])
				}
			}
		}
	}

	var out []RunningApp
	for _, key := range declaredOrder {
		app := declaredApps[key]
		// An unreadable list cannot establish that declared caches have no owner.
		if listErr != nil || len(app.Live) > 0 {
			out = append(out, *app)
		}
	}
	for _, key := range dynamicOrder {
		app := dynamicApps[key]
		if len(app.Live) > 0 && len(app.Paths) > 0 {
			out = append(out, *app)
		}
	}
	return out
}

func appendUniqueOccupant(dst *[]platform.CacheOccupant, cand platform.CacheOccupant) {
	for _, existing := range *dst {
		if existing.PID != 0 && existing.PID == cand.PID {
			return
		}
	}
	*dst = append(*dst, cand)
}

func appMatchesOccupant(app RunningApp, owner platform.CacheOccupant) bool {
	if len(app.Executables) > 0 && owner.Executable != "" {
		for _, exe := range app.Executables {
			if strings.EqualFold(exe, owner.Executable) {
				return true
			}
		}
		return false
	}
	name := owner.Name
	if name == "" && owner.Executable != "" {
		name = filepath.Base(owner.Executable)
	}
	return processNamed(name, app.Processes)
}

func anyAppMatches(apps []RunningApp, owner platform.CacheOccupant) bool {
	for _, app := range apps {
		if appMatchesOccupant(app, owner) {
			return true
		}
	}
	return false
}

func anyLiveForApp(live []platform.CacheOccupant, app RunningApp) bool {
	for _, owner := range live {
		if appMatchesOccupant(app, owner) {
			return true
		}
	}
	return false
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
var forceAppExit = func(owner platform.CacheOccupant) error { return platform.RequestCacheOwnerExit(owner, true) }

// CloseRunningApps asks each app to quit normally and waits up to wait for
// its processes to go away. It never force-terminates: files an app still
// holds are skipped by the per-file cleaner. Consent is per application, so
// processes are re-listed fresh; each exit request still re-verifies identity.
// It returns the apps that were still running at the deadline.
func CloseRunningApps(apps []RunningApp, wait time.Duration, cancelled func() bool) []Text {
	return closeRunningApps(apps, wait, cancelled, false)
}

// CloseRunningAppsForCleanup requires consent to force termination in the
// cleanup confirmation. CLI's normal-quit-only contract remains separate.
func CloseRunningAppsForCleanup(apps []RunningApp, wait time.Duration, cancelled func() bool) []Text {
	return closeRunningApps(apps, wait, cancelled, true)
}

func closeRunningApps(apps []RunningApp, wait time.Duration, cancelled func() bool, force bool) []Text {
	var names []string
	seenName := map[string]bool{}
	for _, app := range apps {
		for _, n := range app.Processes {
			lower := strings.ToLower(n)
			if !seenName[lower] {
				seenName[lower] = true
				names = append(names, n)
			}
		}
		for _, exe := range app.Executables {
			base := filepath.Base(exe)
			lower := strings.ToLower(base)
			if base != "" && !seenName[lower] {
				seenName[lower] = true
				names = append(names, base)
			}
		}
	}
	if len(names) == 0 {
		return nil
	}
	isCancelled := func() bool { return cancelled != nil && cancelled() }
	remaining := func(live []platform.CacheOccupant, err error) []Text {
		var still []Text
		for _, app := range apps {
			if err != nil || anyLiveForApp(live, app) {
				still = append(still, app.Name)
			}
		}
		return still
	}
	if live, err := runningOwnerProcesses(names); err != nil {
		return remaining(live, err)
	} else {
		for _, owner := range live {
			if !anyAppMatches(apps, owner) {
				continue
			}
			if isCancelled() {
				return remaining(live, nil)
			}
			// A child process without windows or one that already exited
			// is not a failure; the deadline check below is the verdict.
			if err := requestAppExit(owner); err != nil {
				log.Printf("[关闭应用] 正常退出 %s (PID %d): %v", owner.Name, owner.PID, err)
			}
		}
	}
	deadline := time.Now().Add(wait)
	forced := false
	for {
		live, err := runningOwnerProcesses(names)
		// An unreadable process list is not evidence that an app quit.
		still := remaining(live, err)
		if err != nil || len(still) == 0 || isCancelled() {
			return still
		}
		if !time.Now().Before(deadline) {
			if !force || forced || err != nil {
				return still
			}
			for _, owner := range live {
				if !anyAppMatches(apps, owner) {
					continue
				}
				if isCancelled() {
					return still
				}
				// Each request rechecks PID creation time, executable and user.
				if err := forceAppExit(owner); err != nil {
					log.Printf("[关闭应用] 强制结束 %s (PID %d): %v", owner.Name, owner.PID, err)
				}
			}
			forced = true
			deadline = time.Now().Add(2 * time.Second)
			continue
		}
		time.Sleep(min(300*time.Millisecond, time.Until(deadline)))
	}
}
