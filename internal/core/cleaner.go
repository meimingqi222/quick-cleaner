package core

import (
	"sync"
	"sync/atomic"
)

type Disposal string

const (
	Permanent  Disposal = "permanent"
	RecycleBin Disposal = "recycle_bin"
)

type CleanResult uint8

const (
	CleanOK CleanResult = iota
	CleanSkipped
	CleanFailed
	CleanManualAction
)

type FailReason string

const (
	FailInUse              FailReason = "in_use"
	FailAccessDenied       FailReason = "access_denied"
	FailUnverified         FailReason = "unverified"
	FailChanged            FailReason = "changed"
	FailWorktreeDirty      FailReason = "worktree_dirty"
	FailWorktreeLocked     FailReason = "worktree_locked"
	FailWorktreeUnverified FailReason = "worktree_unverified"
	FailWorktreeManaged    FailReason = "worktree_managed"
	FailOther              FailReason = "other"
)

// CleanFailure distinguishes real filesystem paths from opaque native resource IDs.
type CleanFailure struct {
	Path string
	ID   string
}

func (f CleanFailure) Label() string {
	if f.Path != "" {
		return f.Path
	}
	return f.ID
}

type CleanSnapshot struct {
	TotalFiles, TotalBytes, Files, Bytes, Failed uint64
	Cancelled                                    bool
	Current                                      string
}

func (s CleanSnapshot) Ratio() float64 {
	if s.TotalFiles > 0 {
		return min(1, float64(s.Files)/float64(s.TotalFiles))
	}
	if s.TotalBytes > 0 {
		return min(1, float64(s.Bytes)/float64(s.TotalBytes))
	}
	return 0
}

type CleanProgress struct {
	TotalFiles, TotalBytes, Files, Bytes, Failed atomic.Uint64
	cancel                                       atomic.Bool
	mu                                           sync.Mutex
	current                                      string
	reasons                                      map[string]FailReason
}

func NewCleanProgress(files, bytes uint64) *CleanProgress {
	p := &CleanProgress{}
	p.TotalFiles.Store(files)
	p.TotalBytes.Store(bytes)
	return p
}

func (p *CleanProgress) RequestCancel()  { p.cancel.Store(true) }
func (p *CleanProgress) Cancelled() bool { return p.cancel.Load() }
func (p *CleanProgress) Note(path string) {
	if p.mu.TryLock() {
		p.current = path
		p.mu.Unlock()
	}
}
func (p *CleanProgress) Snapshot() CleanSnapshot {
	p.mu.Lock()
	current := p.current
	p.mu.Unlock()
	return CleanSnapshot{TotalFiles: p.TotalFiles.Load(), TotalBytes: p.TotalBytes.Load(), Files: p.Files.Load(), Bytes: p.Bytes.Load(), Failed: p.Failed.Load(), Cancelled: p.Cancelled(), Current: current}
}

func (p *CleanProgress) fail(path string, reason FailReason) CleanResult {
	p.mu.Lock()
	if p.reasons == nil {
		p.reasons = make(map[string]FailReason)
	}
	p.reasons[path] = reason
	p.mu.Unlock()
	p.Failed.Add(1)
	return CleanFailed
}

type TargetFailInfo struct {
	Path         string
	FailedLeaves uint
	Reason       FailReason
}

type RemainingLive struct {
	Path             string
	Bytes, FileCount uint64
}

type CleanReport struct {
	PlanExecutions               []PlanExecution
	OK, Skipped                  uint
	SkippedItems, Failed, Manual []CleanFailure
	RemainingLive                []RemainingLive
	FailInfo                     []TargetFailInfo
}

func (r *CleanReport) Record(path string, result CleanResult) {
	r.RecordTarget(CleanFailure{Path: path}, result)
}
func (r *CleanReport) RecordTarget(target CleanFailure, result CleanResult) {
	switch result {
	case CleanOK:
		r.OK++
	case CleanSkipped:
		r.Skipped++
		r.SkippedItems = append(r.SkippedItems, target)
	case CleanFailed:
		r.Failed = append(r.Failed, target)
	case CleanManualAction:
		r.Manual = append(r.Manual, target)
	}
}
func (r *CleanReport) Merge(other CleanReport) {
	r.PlanExecutions = append(r.PlanExecutions, other.PlanExecutions...)
	r.OK += other.OK
	r.Skipped += other.Skipped
	r.SkippedItems = append(r.SkippedItems, other.SkippedItems...)
	r.Failed = append(r.Failed, other.Failed...)
	r.Manual = append(r.Manual, other.Manual...)
	r.RemainingLive = append(r.RemainingLive, other.RemainingLive...)
	r.FailInfo = append(r.FailInfo, other.FailInfo...)
}
func (r CleanReport) WasSkipped(path string) bool {
	for _, item := range r.SkippedItems {
		if item.Path == path {
			return true
		}
	}
	return false
}
