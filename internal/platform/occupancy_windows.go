package platform

import (
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"
	"unsafe"

	"github.com/shirou/gopsutil/v4/process"
	"golang.org/x/sys/windows"
)

const (
	occupancyMaxEntries = 8192
	occupancyDeadline   = 3 * time.Second
	occupancyMaxLocks   = 32
	occupancyMaxOwners  = 16

	fastLockSampleEntries   = 64
	fastLockPerDirBudget    = 100 * time.Millisecond
	fastLockSampleBudget    = 500 * time.Millisecond
	fastLockAttributeBudget = 2 * time.Second
	fastLockWorkers         = 8
)

// occupancyOwners walks until it finds locked files, then asks Restart Manager
// who holds them. It reports system owners instead of refusing the whole probe,
// because the residual panel must explain a skipped system lock. Termination
// authority stays in the strict cache-release path.
func occupancyOwners(root string) ([]Occupant, error) {
	if !filepath.IsAbs(root) {
		return nil, errors.New("occupancy-path-not-absolute")
	}
	md, err := os.Lstat(root)
	if err != nil {
		return nil, err
	}
	if md.Mode()&os.ModeSymlink != 0 {
		return nil, errors.New("occupancy-link")
	}
	var locked []string
	if md.IsDir() {
		locked, err = occupancyLockedFiles(root)
		if err != nil {
			return nil, err
		}
	} else {
		if !md.Mode().IsRegular() {
			return nil, errors.New("occupancy-special-file")
		}
		busy, known := inUseFileOpen(root)
		if !known {
			return nil, errors.New("occupancy-unknown")
		}
		if busy {
			locked = []string{root}
		}
	}
	if len(locked) == 0 {
		return nil, nil
	}
	return occupancyRestartManager(locked)
}

func occupancyLockedFiles(root string) ([]string, error) {
	locked, known := scanDirLocks(root, occupancyMaxEntries, occupancyDeadline, occupancyMaxLocks)
	if len(locked) == 0 {
		if !known {
			return nil, errors.New("occupancy-unknown")
		}
		return nil, nil
	}
	if len(locked) > occupancyMaxLocks {
		locked = locked[:occupancyMaxLocks]
	}
	return locked, nil
}

func occupancyRestartManager(files []string) ([]Occupant, error) {
	var session uint32
	var key [33]uint16
	code, _, _ := cacheRMStart.Call(uintptr(unsafe.Pointer(&session)), 0, uintptr(unsafe.Pointer(&key[0])))
	if code != 0 {
		return nil, fmt.Errorf("RmStartSession=%d", code)
	}
	defer cacheRMEnd.Call(uintptr(session))
	ptrs := make([]*uint16, 0, len(files))
	for _, f := range files {
		p, err := windows.UTF16PtrFromString(f)
		if err != nil {
			return nil, err
		}
		ptrs = append(ptrs, p)
	}
	code, _, _ = cacheRMRegister.Call(uintptr(session), uintptr(len(ptrs)), uintptr(unsafe.Pointer(&ptrs[0])), 0, 0, 0, 0)
	runtime.KeepAlive(ptrs)
	if code != 0 {
		return nil, fmt.Errorf("RmRegisterResources=%d", code)
	}
	var needed, n, reboot uint32
	code, _, _ = cacheRMList.Call(uintptr(session), uintptr(unsafe.Pointer(&needed)), uintptr(unsafe.Pointer(&n)), 0, uintptr(unsafe.Pointer(&reboot)))
	if code == 0 {
		if n != 0 || needed != 0 || reboot != 0 {
			return nil, errors.New("occupancy-incomplete")
		}
		return nil, nil
	}
	if code != 234 || needed == 0 || needed > 128 {
		return nil, fmt.Errorf("RmGetList=%d; needed=%d", code, needed)
	}
	infos := make([]cacheRMInfo, needed)
	n = needed
	code, _, _ = cacheRMList.Call(uintptr(session), uintptr(unsafe.Pointer(&needed)), uintptr(unsafe.Pointer(&n)), uintptr(unsafe.Pointer(&infos[0])), uintptr(unsafe.Pointer(&reboot)))
	if code != 0 || int(n) > len(infos) || reboot != 0 {
		return nil, fmt.Errorf("RmGetList=%d; incomplete-owner-list", code)
	}
	seen := map[uint32]bool{}
	out := make([]Occupant, 0, len(infos))
	for _, info := range infos[:n] {
		if info.PID == 0 || seen[info.PID] {
			continue
		}
		seen[info.PID] = true
		name, exe, system := occupancyOwner(info)
		out = append(out, Occupant{PID: info.PID, Name: name, Executable: exe, System: system})
	}
	sort.Slice(out, func(i, j int) bool { return out[i].PID < out[j].PID })
	if len(out) > occupancyMaxOwners {
		out = out[:occupancyMaxOwners]
	}
	return out, nil
}

func occupancyOwner(info cacheRMInfo) (name, executable string, system bool) {
	app := windows.UTF16ToString(info.AppName[:])
	service := windows.UTF16ToString(info.Service[:])
	if service != "" || info.Type == 3 || info.Type == 4 || info.Type == 1000 {
		return occupancyFallbackName(app, info.PID), "", true
	}
	var ownerSession, ownSession uint32
	if windows.ProcessIdToSessionId(info.PID, &ownerSession) == nil &&
		windows.ProcessIdToSessionId(uint32(os.Getpid()), &ownSession) == nil && ownerSession != ownSession {
		return occupancyFallbackName(app, info.PID), "", true
	}
	p, err := process.NewProcess(int32(info.PID))
	if err != nil {
		return occupancyFallbackName(app, info.PID), "", true
	}
	user, err := p.Username()
	me, meErr := process.NewProcess(int32(os.Getpid()))
	if err != nil || user == "" || meErr != nil {
		return occupancyFallbackName(app, info.PID), "", true
	}
	own, err := me.Username()
	if err != nil || !strings.EqualFold(user, own) {
		return occupancyFallbackName(app, info.PID), "", true
	}
	exe, exeErr := p.Exe()
	if exeErr == nil {
		executable = exe
		if strings.EqualFold(filepath.Base(exe), "explorer.exe") {
			return occupancyFallbackName(app, info.PID), executable, true
		}
	}
	if app != "" {
		return app, executable, false
	}
	if name, err := p.Name(); err == nil && name != "" {
		return name, executable, false
	}
	if executable != "" {
		return filepath.Base(executable), executable, false
	}
	return occupancyFallbackName(app, info.PID), executable, false
}

func occupancyFallbackName(app string, pid uint32) string {
	if app != "" {
		return app
	}
	return "PID " + strconv.Itoa(int(pid))
}

type targetLockSample struct {
	target string
	file   string
}

type verifiedRMOwner struct {
	display string
	owner   CacheOccupant
}

func findLockedCacheOwners(paths []string) ([]LockedTargetOwner, error) {
	if len(paths) == 0 {
		return nil, nil
	}
	sampleDeadline := time.Now().Add(fastLockSampleBudget)
	samples := sampleLockedTargets(paths, sampleDeadline)
	if len(samples) == 0 {
		return nil, nil
	}
	uniqueFiles := make([]string, 0, len(samples))
	seenFile := map[string]bool{}
	for _, s := range samples {
		if !seenFile[s.file] {
			seenFile[s.file] = true
			uniqueFiles = append(uniqueFiles, s.file)
		}
	}
	infos, err := rmQueryInfos(uniqueFiles)
	if err != nil || len(infos) == 0 {
		return nil, err
	}
	verified, hadOmitted := filterClosableRMOwners(infos)
	if len(verified) == 0 {
		return nil, nil
	}
	exeSet := map[string]bool{}
	for _, v := range verified {
		exeSet[strings.ToLower(v.owner.Executable)] = true
	}
	// Fast path only when every locking process in the session belongs to the
	// same verified application; if any system/self owner was omitted, per-bucket
	// attribution is required so omitted locks are not misattributed to that app.
	if !hadOmitted && len(exeSet) == 1 {
		var out []LockedTargetOwner
		for _, s := range samples {
			for _, v := range verified {
				owner := v.owner
				owner.Evidence = s.target
				out = append(out, LockedTargetOwner{Target: s.target, Display: v.display, Owner: owner})
			}
		}
		sortLockedTargetOwners(out)
		return out, nil
	}
	// Attribution gets its own bounded budget once real closable locks are
	// confirmed, so an initial RM query under CPU load cannot starve bucket checks.
	return attributeMultiAppLocks(samples, verified, time.Now().Add(fastLockAttributeBudget)), nil
}

func sampleLockedTargets(paths []string, totalDeadline time.Time) []targetLockSample {
	jobs := make(chan string, len(paths))
	for _, p := range paths {
		jobs <- p
	}
	close(jobs)
	var mu sync.Mutex
	var samples []targetLockSample
	var wg sync.WaitGroup
	for range min(fastLockWorkers, len(paths)) {
		wg.Go(func() {
			for root := range jobs {
				if time.Now().After(totalDeadline) {
					return
				}
				if file, ok := sampleFirstLockedFile(root, totalDeadline); ok {
					mu.Lock()
					samples = append(samples, targetLockSample{target: root, file: file})
					mu.Unlock()
				}
			}
		})
	}
	wg.Wait()
	return samples
}

func sampleFirstLockedFile(root string, totalDeadline time.Time) (string, bool) {
	if !filepath.IsAbs(root) || appRejectStagingLinks(root) != nil {
		return "", false
	}
	md, err := os.Lstat(root)
	if err != nil || md.Mode()&os.ModeSymlink != 0 {
		return "", false
	}
	if !md.IsDir() {
		if !md.Mode().IsRegular() {
			return "", false
		}
		busy, known := inUseFileOpen(root)
		return root, known && busy
	}
	dirDeadline := minTime(totalDeadline, time.Now().Add(fastLockPerDirBudget))
	checked := 0
	locked := ""
	_ = filepath.WalkDir(root, func(p string, d fs.DirEntry, err error) error {
		if err != nil {
			return fs.SkipDir
		}
		if checked >= 1 && time.Now().After(dirDeadline) {
			return fs.SkipAll
		}
		if d.Type()&os.ModeSymlink != 0 {
			if d.IsDir() {
				return fs.SkipDir
			}
			return nil
		}
		if ptr, perr := windows.UTF16PtrFromString(p); perr == nil {
			if attr, aerr := windows.GetFileAttributes(ptr); aerr == nil && attr&windows.FILE_ATTRIBUTE_REPARSE_POINT != 0 {
				if d.IsDir() {
					return fs.SkipDir
				}
				return nil
			}
		}
		if d.IsDir() {
			return nil
		}
		if !d.Type().IsRegular() {
			return nil
		}
		checked++
		if checked > fastLockSampleEntries {
			return fs.SkipAll
		}
		if busy, known := inUseFileOpen(p); known && busy {
			locked = p
			return fs.SkipAll
		}
		return nil
	})
	return locked, locked != ""
}

func minTime(a, b time.Time) time.Time {
	if a.Before(b) {
		return a
	}
	return b
}

func rmQueryInfos(files []string) ([]cacheRMInfo, error) {
	if len(files) == 0 {
		return nil, nil
	}
	var session uint32
	var key [33]uint16
	code, _, _ := cacheRMStart.Call(uintptr(unsafe.Pointer(&session)), 0, uintptr(unsafe.Pointer(&key[0])))
	if code != 0 {
		return nil, fmt.Errorf("RmStartSession=%d", code)
	}
	defer cacheRMEnd.Call(uintptr(session))
	ptrs := make([]*uint16, 0, len(files))
	for _, f := range files {
		p, err := windows.UTF16PtrFromString(f)
		if err != nil {
			return nil, err
		}
		ptrs = append(ptrs, p)
	}
	for i := 0; i < len(ptrs); i += 256 {
		batch := ptrs[i:min(i+256, len(ptrs))]
		code, _, _ = cacheRMRegister.Call(uintptr(session), uintptr(len(batch)), uintptr(unsafe.Pointer(&batch[0])), 0, 0, 0, 0)
		if code != 0 {
			runtime.KeepAlive(ptrs)
			return nil, fmt.Errorf("RmRegisterResources=%d", code)
		}
	}
	runtime.KeepAlive(ptrs)
	var needed, n, reboot uint32
	code, _, _ = cacheRMList.Call(uintptr(session), uintptr(unsafe.Pointer(&needed)), uintptr(unsafe.Pointer(&n)), 0, uintptr(unsafe.Pointer(&reboot)))
	if code == 0 {
		return nil, nil
	}
	if code != 234 || needed == 0 || needed > 128 {
		return nil, fmt.Errorf("RmGetList=%d; needed=%d", code, needed)
	}
	infos := make([]cacheRMInfo, needed)
	n = needed
	code, _, _ = cacheRMList.Call(uintptr(session), uintptr(unsafe.Pointer(&needed)), uintptr(unsafe.Pointer(&n)), uintptr(unsafe.Pointer(&infos[0])), uintptr(unsafe.Pointer(&reboot)))
	if code != 0 || int(n) > len(infos) {
		return nil, fmt.Errorf("RmGetList=%d; incomplete-owner-list", code)
	}
	return infos[:n], nil
}

func filterClosableRMOwners(infos []cacheRMInfo) (map[uint32]verifiedRMOwner, bool) {
	parents, err := snapshotProcessParents()
	if err != nil {
		return nil, false
	}
	selfPID := uint32(os.Getpid())
	ancestors := cleanerAncestorPIDs(selfPID, parents)
	verified := map[uint32]verifiedRMOwner{}
	hadOmitted := false
	seenPID := map[uint32]bool{}
	for _, info := range infos {
		if info.PID == 0 || seenPID[info.PID] {
			continue
		}
		seenPID[info.PID] = true
		if info.PID <= 4 || info.Type == 3 || info.Type == 4 || info.Type == 1000 || info.Service[0] != 0 {
			hadOmitted = true
			continue
		}
		// Never offer to close QuickCleaner itself, its child helpers, or the
		// host terminal/IDE ancestor process that launched QuickCleaner.
		if ancestors[info.PID] || cacheCheckAncestry(info.PID, selfPID, parents) != nil {
			hadOmitted = true
			continue
		}
		owner := CacheOccupant{PID: info.PID, Identity: cacheFiletime(info.Created)}
		h, err := cacheOpenVerified(owner, false, false)
		if err != nil {
			hadOmitted = true
			continue
		}
		owner.Name, owner.Executable, err = cacheProcessSnapshot(info.PID)
		if err == nil {
			err = cacheVerifyHandle(h, owner, true)
		}
		windows.CloseHandle(h)
		if err != nil {
			hadOmitted = true
			continue
		}
		display := friendlyRMAppName(windows.UTF16ToString(info.AppName[:]), owner.Name, owner.Executable)
		verified[info.PID] = verifiedRMOwner{display: display, owner: owner}
	}
	return verified, hadOmitted
}

func snapshotProcessParents() (map[uint32]uint32, error) {
	snapshot, err := windows.CreateToolhelp32Snapshot(windows.TH32CS_SNAPPROCESS, 0)
	if err != nil {
		return nil, err
	}
	defer windows.CloseHandle(snapshot)
	parents := map[uint32]uint32{}
	entry := windows.ProcessEntry32{Size: uint32(unsafe.Sizeof(windows.ProcessEntry32{}))}
	for err = windows.Process32First(snapshot, &entry); err == nil; err = windows.Process32Next(snapshot, &entry) {
		parents[entry.ProcessID] = entry.ParentProcessID
	}
	if !errors.Is(err, windows.ERROR_NO_MORE_FILES) {
		return nil, err
	}
	return parents, nil
}

func cleanerAncestorPIDs(self uint32, parents map[uint32]uint32) map[uint32]bool {
	out := map[uint32]bool{self: true}
	cur := self
	for range 64 {
		parent, ok := parents[cur]
		if !ok || parent <= 4 || out[parent] {
			break
		}
		if !processStartedNotAfter(parent, cur) {
			break
		}
		out[parent] = true
		cur = parent
	}
	return out
}

func processStartedNotAfter(olderPID, youngerPID uint32) bool {
	tOlder, ok1 := queryProcessCreateTime(olderPID)
	tYounger, ok2 := queryProcessCreateTime(youngerPID)
	return ok1 && ok2 && tOlder <= tYounger
}

func queryProcessCreateTime(pid uint32) (uint64, bool) {
	h, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
	if err != nil {
		return 0, false
	}
	defer windows.CloseHandle(h)
	var created, exited, kernel, user windows.Filetime
	if windows.GetProcessTimes(h, &created, &exited, &kernel, &user) != nil {
		return 0, false
	}
	return cacheFiletime(created), true
}

func friendlyRMAppName(rmApp, procName, exe string) string {
	for _, candidate := range []string{strings.TrimSpace(rmApp), strings.TrimSpace(procName), filepath.Base(exe)} {
		if candidate == "" {
			continue
		}
		if len(candidate) > 4 && strings.EqualFold(candidate[len(candidate)-4:], ".exe") {
			candidate = candidate[:len(candidate)-4]
		}
		if candidate != "" {
			return candidate
		}
	}
	return "Application"
}

func attributeMultiAppLocks(samples []targetLockSample, verified map[uint32]verifiedRMOwner, deadline time.Time) []LockedTargetOwner {
	buckets := map[string][]targetLockSample{}
	var keys []string
	for _, s := range samples {
		k := cacheAppBucketKey(s.target)
		if _, exists := buckets[k]; !exists {
			keys = append(keys, k)
		}
		buckets[k] = append(buckets[k], s)
	}
	jobs := make(chan string, len(keys))
	for _, k := range keys {
		jobs <- k
	}
	close(jobs)
	var mu sync.Mutex
	var out []LockedTargetOwner
	var wg sync.WaitGroup
	for range min(4, len(keys)) {
		wg.Go(func() {
			for k := range jobs {
				if time.Now().After(deadline) {
					return
				}
				group := buckets[k]
				files := make([]string, len(group))
				for i, s := range group {
					files[i] = s.file
				}
				infos, err := rmQueryInfos(files)
				if err != nil || len(infos) == 0 {
					continue
				}
				exes := map[string]bool{}
				var matched []verifiedRMOwner
				seenPID := map[uint32]bool{}
				for _, info := range infos {
					if v, ok := verified[info.PID]; ok && !seenPID[info.PID] {
						seenPID[info.PID] = true
						matched = append(matched, v)
						exes[strings.ToLower(v.owner.Executable)] = true
					}
				}
				if len(matched) == 0 {
					continue
				}
				if len(exes) == 1 || len(group) == 1 {
					mu.Lock()
					for _, s := range group {
						for _, v := range matched {
							owner := v.owner
							owner.Evidence = s.target
							out = append(out, LockedTargetOwner{Target: s.target, Display: v.display, Owner: owner})
						}
					}
					mu.Unlock()
					continue
				}
				// Mixed bucket: attribute each target individually.
				for _, s := range group {
					if time.Now().After(deadline) {
						break
					}
					singleInfos, err := rmQueryInfos([]string{s.file})
					if err != nil {
						continue
					}
					seenSingle := map[uint32]bool{}
					mu.Lock()
					for _, info := range singleInfos {
						if v, ok := verified[info.PID]; ok && !seenSingle[info.PID] {
							seenSingle[info.PID] = true
							owner := v.owner
							owner.Evidence = s.target
							out = append(out, LockedTargetOwner{Target: s.target, Display: v.display, Owner: owner})
						}
					}
					mu.Unlock()
				}
			}
		})
	}
	wg.Wait()
	sortLockedTargetOwners(out)
	return out
}

// cacheAppBucketKey groups sibling cache subdirectories of the same app root
// (e.g. AppData\Roaming\Notion\* or AppData\Local\Microsoft\Edge\*) so multi-app
// Restart Manager attribution needs one query per app instead of one per leaf dir.
func cacheAppBucketKey(target string) string {
	clean := filepath.Clean(target)
	lower := strings.ToLower(clean)
	for _, marker := range []string{`\appdata\roaming\`, `\appdata\local\`} {
		idx := strings.Index(lower, marker)
		if idx < 0 {
			continue
		}
		rest := clean[idx+len(marker):]
		parts := strings.Split(rest, `\`)
		if strings.Contains(marker, "roaming") && len(parts) >= 1 && parts[0] != "" {
			return lower[:idx+len(marker)] + strings.ToLower(parts[0])
		}
		if len(parts) >= 2 && parts[0] != "" && parts[1] != "" {
			return lower[:idx+len(marker)] + strings.ToLower(parts[0]) + `\` + strings.ToLower(parts[1])
		}
		if len(parts) >= 1 && parts[0] != "" {
			return lower[:idx+len(marker)] + strings.ToLower(parts[0])
		}
	}
	return lower
}

func sortLockedTargetOwners(out []LockedTargetOwner) {
	sort.Slice(out, func(i, j int) bool {
		if out[i].Target != out[j].Target {
			return out[i].Target < out[j].Target
		}
		return out[i].Owner.PID < out[j].Owner.PID
	})
}
