package core

import (
	"os"
	"path/filepath"
	"runtime"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// EmptyTrash keeps filesystem deletion in core: native adapters supply only
// trusted user roots and the Windows shell notification, not recursive policy.
func EmptyTrash(p *CleanProgress) CleanReport {
	var report CleanReport
	if p.Cancelled() {
		return report
	}
	if err := platform.EmptyTrashShell(); err != nil {
		report.RecordTarget(CleanFailure{ID: "recycle_bin"}, CleanFailed)
	}
	for _, dir := range platform.TrashDirectories() {
		if p.Cancelled() {
			break
		}
		if runtime.GOOS != "windows" {
			if _, err := os.Lstat(dir); os.IsNotExist(err) {
				continue
			}
			report.Merge(CleanDirContents(dir, p))
			continue
		}
		md, err := os.Lstat(dir)
		if os.IsNotExist(err) {
			continue
		}
		if err != nil || !md.IsDir() || ruleEvidenceLink(md) {
			report.Record(dir, p.fail(dir, FailUnverified))
			continue
		}
		children, parent, hasParent, err := enumerateCleanEntries(dir)
		if err != nil {
			report.Record(dir, p.fail(dir, ioFailReason(err)))
			continue
		}
		checked := cleanEntryBindings(dir, parent, hasParent, children)
		for i, c := range children {
			if p.Cancelled() {
				return report
			}
			if !platform.IsRecycleJunkEntry(filepath.Base(c.path)) {
				continue
			}
			if !checked[i] {
				report.Record(c.path, p.fail(c.path, FailChanged))
				continue
			}
			report.Record(c.path, DeleteTree(c.path, p))
		}
	}
	return report
}
