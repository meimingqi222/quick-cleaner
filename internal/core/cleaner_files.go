package core

import (
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"syscall"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func ioFailReason(err error) FailReason {
	if runtime.GOOS == "windows" && errors.Is(err, syscall.Errno(32)) {
		return FailInUse
	}
	if errors.Is(err, os.ErrPermission) {
		return FailAccessDenied
	}
	return FailOther
}

func removeWithReadonlyRetry(path string) error {
	err := os.Remove(path)
	if err == nil {
		return nil
	}
	if md, statErr := os.Lstat(path); statErr == nil && !ruleEvidenceLink(md) && md.Mode().Perm()&0200 == 0 {
		_ = os.Chmod(path, md.Mode().Perm()|0200)
	}
	return os.Remove(path)
}

// Access recovery is local to the failed object and its unprotected parent;
// a nonempty directory never authorizes recursively rewriting its ACLs.
func removeForcing(path string) error {
	err := removeWithReadonlyRetry(path)
	if err == nil || !errors.Is(err, os.ErrPermission) {
		return err
	}
	_ = platform.ForceDeleteAccess(path)
	if retry := removeWithReadonlyRetry(path); retry == nil {
		return nil
	}
	parent := filepath.Dir(path)
	if parent != path && !IsProtected(parent) {
		_ = platform.ForceDeleteAccess(parent)
		if retry := removeWithReadonlyRetry(path); retry == nil {
			return nil
		}
	}
	return err
}

func cleanFile(path string, size uint64, p *CleanProgress) CleanResult {
	if p.Cancelled() || IsProtected(path) {
		return CleanSkipped
	}
	p.Note(path)
	if err := removeForcing(path); err != nil {
		return p.fail(path, ioFailReason(err))
	}
	p.Files.Add(1)
	p.Bytes.Add(size)
	return CleanOK
}

type cleanEntry struct {
	path        string
	size        uint64
	identity    TargetIdentity
	hasIdentity bool
	link, dir   bool
}

func enumerateCleanEntries(dir string) ([]cleanEntry, TargetIdentity, bool, error) {
	entries, err := os.ReadDir(dir)
	if errors.Is(err, os.ErrPermission) {
		_ = platform.ForceDeleteAccess(dir)
		entries, err = os.ReadDir(dir)
	}
	if err != nil {
		return nil, TargetIdentity{}, false, err
	}
	parent, hasParent := CaptureIdentity(dir)
	children := make([]cleanEntry, 0, len(entries))
	for _, entry := range entries {
		path := filepath.Join(dir, entry.Name())
		md, err := os.Lstat(path)
		if err != nil {
			return nil, parent, hasParent, err
		}
		id, ok := IdentityFromMetadata(md)
		children = append(children, cleanEntry{path: path, size: platform.AllocatedFileSize(md), identity: id, hasIdentity: ok, link: ruleEvidenceLink(md), dir: md.IsDir()})
	}
	return children, parent, hasParent, nil
}

func cleanEntryBindings(dir string, parent TargetIdentity, hasParent bool, children []cleanEntry) []bool {
	checked := make([]bool, len(children))
	// All bindings are checked before deletion changes Windows directory mtimes.
	for i, c := range children {
		checked[i] = hasParent && c.hasIdentity && parent.Recheck(dir) && c.identity.Recheck(c.path)
	}
	return checked
}

func cleanFileFamilies(files []cleanEntry, p *CleanProgress) uint {
	groups := make(map[string][]cleanEntry)
	for _, f := range files {
		key, ok := SQLiteFamilyKey(filepath.Base(f.path))
		if !ok {
			key = f.path
		}
		groups[key] = append(groups[key], f)
	}
	var failed uint
	for _, members := range groups {
		sort.SliceStable(members, func(i, j int) bool {
			return IsSQLiteCompanionName(filepath.Base(members[i].path)) && !IsSQLiteCompanionName(filepath.Base(members[j].path))
		})
		hasCompanion, hasMain := false, false
		mainPath := ""
		for _, f := range members {
			if IsSQLiteCompanionName(filepath.Base(f.path)) {
				hasCompanion = true
			} else {
				hasMain = true
				mainPath = f.path
			}
		}
		if hasCompanion && hasMain && IsLiveDatabase(mainPath) {
			for _, f := range members {
				p.fail(f.path, FailInUse)
				failed++
			}
			continue
		}
		companionFailed := false
		for _, f := range members {
			companion := IsSQLiteCompanionName(filepath.Base(f.path))
			if !companion && companionFailed {
				failed++
				continue
			}
			if cleanFile(f.path, f.size, p) == CleanFailed {
				failed++
				if companion {
					companionFailed = true
				}
			}
		}
	}
	return failed
}

// DeleteTree is the single recursive permanent-delete exit. Links are unlinked,
// never traversed; every recursive level asks the central safety policy again.
func DeleteTree(path string, p *CleanProgress) CleanResult {
	if p.Cancelled() || IsProtected(path) || IsVirtualPath(path) {
		return CleanSkipped
	}
	md, err := os.Lstat(path)
	if err != nil {
		return CleanSkipped
	}
	if ruleEvidenceLink(md) {
		if err := os.Remove(path); err != nil {
			return p.fail(path, ioFailReason(err))
		}
		p.Files.Add(1)
		return CleanOK
	}
	if !md.IsDir() {
		return cleanFile(path, platform.AllocatedFileSize(md), p)
	}
	p.Note(path)
	children, parent, hasParent, err := enumerateCleanEntries(path)
	if err != nil {
		return p.fail(path, ioFailReason(err))
	}
	checked := cleanEntryBindings(path, parent, hasParent, children)
	var files, dirs []cleanEntry
	var failed uint
	for i, c := range children {
		if !checked[i] {
			p.fail(c.path, FailChanged)
			failed++
			continue
		}
		if c.dir || c.link {
			dirs = append(dirs, c)
		} else {
			files = append(files, c)
		}
	}
	failed += cleanFileFamilies(files, p)
	for _, d := range dirs {
		if DeleteTree(d.path, p) == CleanFailed {
			failed++
		}
	}
	if p.Cancelled() {
		return CleanSkipped
	}
	if err := removeForcing(path); err != nil {
		return p.fail(path, ioFailReason(err))
	}
	if failed > 0 {
		return p.fail(path, FailOther)
	}
	return CleanOK
}

func CleanPath(path string, p *CleanProgress) CleanResult {
	if IsVirtualPath(path) || p.Cancelled() {
		return CleanSkipped
	}
	if _, err := os.Lstat(path); err != nil {
		return CleanSkipped
	}
	if IsProtected(path) {
		return CleanSkipped
	}
	if IsLiveDatabase(path) {
		return p.fail(path, FailInUse)
	}
	return DeleteTree(path, p)
}

func RootIdentityHolds(path string, identity *TargetIdentity) bool {
	return identity != nil && identity.Recheck(path)
}

func CleanDirContents(dir string, p *CleanProgress) CleanReport {
	var report CleanReport
	md, err := os.Lstat(dir)
	if err != nil {
		report.Record(dir, CleanSkipped)
		return report
	}
	if ruleEvidenceLink(md) || !md.IsDir() {
		report.Record(dir, p.fail(dir, FailChanged))
		return report
	}
	if IsContentsProtected(dir) || p.Cancelled() {
		report.Record(dir, CleanSkipped)
		return report
	}
	children, parent, hasParent, err := enumerateCleanEntries(dir)
	if err != nil {
		report.Record(dir, p.fail(dir, ioFailReason(err)))
		return report
	}
	checked := cleanEntryBindings(dir, parent, hasParent, children)
	for i, c := range children {
		if p.Cancelled() {
			report.Record(c.path, CleanSkipped)
			continue
		}
		if !checked[i] {
			report.Record(c.path, p.fail(c.path, FailChanged))
			continue
		}
		report.Record(c.path, CleanPath(c.path, p))
	}
	return report
}

func Dispose(path string, disposal Disposal, p *CleanProgress) CleanResult {
	switch disposal {
	case Permanent:
		return CleanPath(path, p)
	case RecycleBin:
		return recyclePathWith(path, p, platform.MoveToTrash)
	default:
		return p.fail(path, FailOther)
	}
}

func recyclePathWith(path string, p *CleanProgress, move func(string) error) CleanResult {
	if p.Cancelled() || IsVirtualPath(path) || IsProtected(path) {
		return CleanSkipped
	}
	if _, err := os.Lstat(path); err != nil {
		return CleanSkipped
	}
	if IsLiveDatabase(path) {
		return p.fail(path, FailInUse)
	}
	p.Note(path)
	if err := move(path); err != nil {
		return p.fail(path, ioFailReason(err))
	}
	if _, err := os.Lstat(path); !os.IsNotExist(err) {
		return p.fail(path, FailUnverified)
	}
	// Recycling relocates bytes; it never counts as released disk space.
	p.Files.Add(1)
	return CleanOK
}

type ArbitraryTarget struct {
	Path      string
	Identity  *TargetIdentity
	Operation Operation
}

func CaptureArbitraryTarget(path string) ArbitraryTarget {
	t := ArbitraryTarget{Path: path, Operation: Operation{Kind: OpFile}}
	if id, ok := CaptureIdentity(path); ok {
		t.Identity = &id
	}
	if md, err := os.Lstat(path); err == nil && md.IsDir() && !ruleEvidenceLink(md) {
		t.Operation.Kind = OpTree
	}
	return t
}

func CleanArbitrary(paths []string, disposal Disposal, p *CleanProgress) CleanReport {
	items := make([]ArbitraryTarget, len(paths))
	for i, path := range paths {
		items[i] = CaptureArbitraryTarget(path)
	}
	return CleanArbitraryItems(items, disposal, p)
}

func CleanArbitraryItems(items []ArbitraryTarget, disposal Disposal, p *CleanProgress) CleanReport {
	paths := make([]string, len(items))
	for i, t := range items {
		paths[i] = t.Path
	}
	return cleanArbitraryItems(items, disposal, p, SpotCheckInUse(paths))
}

func cleanArbitraryItems(items []ArbitraryTarget, disposal Disposal, p *CleanProgress, spot map[string]SpotCheck) CleanReport {
	var report CleanReport
	for _, t := range items {
		if p.Cancelled() {
			break
		}
		path := t.Path
		p.Note(path)
		state, known := spot[path]
		if !known || state != SpotClear {
			reason := FailUnverified
			if known && state == SpotBusy {
				reason = FailInUse
			}
			report.Record(path, p.fail(path, reason))
			continue
		}
		if IsProtected(path) {
			report.Record(path, CleanSkipped)
			continue
		}
		if md, err := os.Lstat(path); err == nil && md.IsDir() != (t.Operation.Kind == OpTree) {
			report.Record(path, p.fail(path, FailChanged))
			continue
		}
		if t.Identity != nil && !t.Identity.Recheck(path) {
			report.Record(path, p.fail(path, FailChanged))
			continue
		}
		report.Record(path, Dispose(path, disposal, p))
	}
	return report
}
