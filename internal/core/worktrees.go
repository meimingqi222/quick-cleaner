package core

import (
	"bytes"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"time"
	"unicode/utf8"
)

type WorktreeRegistration struct{ Admin, common, target, backlink string }

func worktreeText(path string) (string, error) {
	md, err := os.Lstat(path)
	if err != nil {
		return "", err
	}
	if !md.Mode().IsRegular() || ruleEvidenceLink(md) || md.Size() > 8192 {
		return "", errors.New("Invalid worktree metadata")
	}
	f, err := os.Open(path)
	if err != nil {
		return "", err
	}
	defer f.Close()
	data, err := io.ReadAll(io.LimitReader(f, 8193))
	if err != nil {
		return "", err
	}
	if len(data) > 8192 || !utf8.Valid(data) {
		return "", errors.New("Invalid worktree metadata encoding or size")
	}
	return strings.TrimRight(string(data), "\r\n"), nil
}
func canonicalWorktreePath(path string) (string, error) {
	if runtime.GOOS == "windows" && (len(filepath.VolumeName(path)) != 2 || strings.HasPrefix(path, `\\`)) {
		return "", errors.New("Worktree must be on a local drive")
	}
	resolved, err := filepath.EvalSymlinks(path)
	if err != nil {
		return "", err
	}
	return filepath.Abs(resolved)
}
func resolveWorktreePath(base, value string) (string, error) {
	if value == "" || strings.ContainsAny(value, "\r\n\x00") {
		return "", errors.New("Invalid worktree path")
	}
	if !filepath.IsAbs(value) {
		value = filepath.Join(base, value)
	}
	return canonicalWorktreePath(value)
}

func InspectWorktree(path string) (WorktreeRegistration, error) {
	var entry WorktreeRegistration
	target, err := canonicalWorktreePath(path)
	if err != nil {
		return entry, err
	}
	marker := filepath.Join(target, ".git")
	text, err := worktreeText(marker)
	if err != nil {
		return entry, err
	}
	if !strings.HasPrefix(text, "gitdir: ") {
		return entry, errors.New("Not a linked worktree")
	}
	admin, err := resolveWorktreePath(target, strings.TrimPrefix(text, "gitdir: "))
	if err != nil {
		return entry, err
	}
	commonText, err := worktreeText(filepath.Join(admin, "commondir"))
	if err != nil {
		return entry, err
	}
	common, err := resolveWorktreePath(admin, commonText)
	if err != nil {
		return entry, err
	}
	backlink, err := worktreeText(filepath.Join(admin, "gitdir"))
	if err != nil {
		return entry, err
	}
	resolvedBacklink, err := resolveWorktreePath(admin, backlink)
	regular := func(path string) bool { md, err := os.Stat(path); return err == nil && md.Mode().IsRegular() }
	if err != nil || Norm(filepath.Dir(admin)) != Norm(filepath.Join(common, "worktrees")) || Norm(resolvedBacklink) != Norm(marker) || !regular(filepath.Join(common, "HEAD")) || !regular(filepath.Join(admin, "HEAD")) {
		return entry, errors.New("Worktree registration mismatch")
	}
	return WorktreeRegistration{admin, common, target, backlink}, nil
}

func DiscoverWorktrees(container string) []string {
	budget := 4096
	var found []string
	var visit func(string, int)
	visit = func(path string, depth int) {
		if budget <= 0 {
			return
		}
		budget--
		md, err := os.Lstat(path)
		if err != nil || !md.IsDir() || ruleEvidenceLink(md) {
			return
		}
		if _, err := os.Lstat(filepath.Join(path, ".git")); err == nil {
			if _, err := InspectWorktree(path); err == nil {
				found = append(found, path)
			}
			return
		} else if !os.IsNotExist(err) {
			return
		}
		if depth == 0 {
			return
		}
		file, err := os.Open(path)
		if err != nil {
			return
		}
		defer file.Close()
		for budget > 0 {
			entries, err := file.ReadDir(min(128, budget))
			if err != nil && err != io.EOF {
				return
			}
			for _, child := range entries {
				if budget <= 0 {
					break
				}
				budget--
				if child.IsDir() && child.Type()&os.ModeSymlink == 0 {
					visit(filepath.Join(path, child.Name()), depth-1)
				}
			}
			if err == io.EOF {
				return
			}
		}
	}
	visit(container, 3)
	return found
}

func worktreeGit(common string, args []string, timeout time.Duration) (ProcRun, bool) {
	argv := []string{"--no-optional-locks", "-c", "core.fsmonitor=false", "--git-dir", common}
	return RunToolWithTimeout("git", append(argv, args...), timeout)
}

func worktreeMetadataRemovable(path string, budget *int) bool {
	if *budget <= 0 || IsProtected(path) {
		return false
	}
	*budget--
	md, err := os.Lstat(path)
	if err != nil || ruleEvidenceLink(md) {
		return false
	}
	if md.Mode().IsRegular() {
		return filepath.Ext(path) != ".lock"
	}
	if !md.IsDir() {
		return false
	}
	f, err := os.Open(path)
	if err != nil {
		return false
	}
	defer f.Close()
	for {
		entries, err := f.ReadDir(128)
		if err != nil && err != io.EOF {
			return false
		}
		for _, child := range entries {
			if !worktreeMetadataRemovable(filepath.Join(path, child.Name()), budget) {
				return false
			}
		}
		if err == io.EOF {
			return true
		}
	}
}

// Ignored files and submodules also count as user data, not regenerable output.
func (w WorktreeRegistration) CheckReady() FailReason {
	if IsManagedAgentWorktree(w.target) {
		return FailWorktreeManaged
	}
	if _, err := os.Lstat(filepath.Join(w.Admin, "locked")); err == nil {
		return FailWorktreeLocked
	} else if !os.IsNotExist(err) {
		return FailWorktreeUnverified
	}
	budget := 1024
	current, err := InspectWorktree(w.target)
	if IsProtected(w.target) || !worktreeMetadataRemovable(w.Admin, &budget) || err != nil || current != w {
		return FailWorktreeUnverified
	}
	if _, err := os.Lstat(filepath.Join(w.Admin, "locked")); !os.IsNotExist(err) {
		return FailWorktreeUnverified
	}
	status, ok := worktreeGit(w.Admin, []string{"--work-tree", w.target, "status", "--porcelain", "--untracked-files=all", "--ignored", "--ignore-submodules=none"}, 10*time.Second)
	if !ok || !status.OK {
		return FailWorktreeUnverified
	}
	if len(status.Stdout) != 0 {
		return FailWorktreeDirty
	}
	index, ok := worktreeGit(w.Admin, []string{"ls-files", "--stage", "-z"}, 10*time.Second)
	if !ok || !index.OK {
		return FailWorktreeUnverified
	}
	for _, item := range bytes.Split(index.Stdout, []byte{0}) {
		if bytes.HasPrefix(item, []byte("160000 ")) {
			return FailWorktreeUnverified
		}
	}
	return ""
}

func (w WorktreeRegistration) unregister() error {
	if _, err := os.Lstat(w.target); !os.IsNotExist(err) {
		return errors.New("Worktree checkout still present or unverified")
	}
	budget := 1024
	backlink, err := worktreeText(filepath.Join(w.Admin, "gitdir"))
	commonText, commonErr := worktreeText(filepath.Join(w.Admin, "commondir"))
	common, resolveErr := resolveWorktreePath(w.Admin, commonText)
	if !worktreeMetadataRemovable(w.Admin, &budget) || err != nil || commonErr != nil || resolveErr != nil || backlink != w.backlink || common != w.common {
		return errors.New("Exact worktree registration could not be revalidated")
	}
	result, ok := worktreeGit(w.common, []string{"worktree", "remove", "--", w.target}, 30*time.Second)
	if !ok {
		return errors.New("Exact worktree unregister unavailable or timed out")
	}
	if !result.OK {
		return fmt.Errorf("Exact worktree unregister failed: %s", strings.TrimSpace(string(result.Stderr)))
	}
	return nil
}

func (w WorktreeRegistration) completion() Evidence {
	checkout := FilesystemCompletion(CompletionCondition{Kind: "path_absent", Path: w.target})
	registration := FilesystemCompletion(CompletionCondition{Kind: "path_absent", Path: w.Admin})
	if checkout == Confirmed && registration == Confirmed {
		return Confirmed
	}
	if checkout == Unknown || registration == Unknown {
		return Unknown
	}
	return Absent
}

type worktreeExecutor struct {
	filesystemExecutor
	entry *WorktreeRegistration
}

func (e *worktreeExecutor) supports(t PlannedTarget) bool {
	return t.Operation.Kind == OpGitWorktree && t.Disposal == Permanent
}
func (e *worktreeExecutor) prepare(t PlannedTarget) (string, error) {
	entry, err := InspectWorktree(t.Path)
	reason := FailWorktreeUnverified
	if err == nil && Norm(entry.Admin) == Norm(t.Operation.Registration) {
		reason = entry.CheckReady()
	}
	if reason != "" {
		e.progress.mu.Lock()
		if e.progress.reasons == nil {
			e.progress.reasons = map[string]FailReason{}
		}
		e.progress.reasons[t.Path] = reason
		e.progress.mu.Unlock()
		return "", fmt.Errorf("Worktree preflight: %s", reason)
	}
	e.entry = &entry
	return "", nil
}
func (e *worktreeExecutor) apply(t PlannedTarget) CleanReport {
	current, err := InspectWorktree(t.Path)
	if t.Identity == nil || !t.Identity.Recheck(t.Path) || IsProtected(t.Path) || err != nil || e.entry == nil || current != *e.entry {
		var report CleanReport
		report.Record(t.Path, e.progress.fail(t.Path, FailWorktreeUnverified))
		return report
	}
	return e.filesystemExecutor.apply(t)
}
func (e *worktreeExecutor) unregister(t PlannedTarget, registration string) error {
	if e.entry == nil || e.entry.Admin != registration {
		return errors.New("Exact worktree registration was not prepared")
	}
	return e.entry.unregister()
}
func (e *worktreeExecutor) verify(c CompletionCondition) Evidence {
	if c.Kind == "path_absent" {
		return FilesystemCompletion(c)
	}
	if c.Kind == "worktree_and_registration_absent" && e.entry != nil && e.entry.Admin == c.Registration {
		return e.entry.completion()
	}
	return Unknown
}
func ExecuteWorktreePlan(plan *CleanupPlan, index int, p *CleanProgress, occupancy SpotCheck) CleanReport {
	return executeCapability(plan, index, p, occupancy, &worktreeExecutor{filesystemExecutor: filesystemExecutor{progress: p}})
}
