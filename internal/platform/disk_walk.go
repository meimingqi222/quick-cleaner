package platform

import "errors"

var ErrMacWalkUnsupported = errors.New("native bulk volume walking requires macOS")
var ErrMacWalkMalformed = errors.New("malformed getattrlistbulk record")

const MacWalkRootIndex uint64 = 0

// MacWalkEntry is acquisition data, not an aggregated tree. Size is allocated
// bytes (ATTR_FILE_ALLOCSIZE, or st_blocks * 512 in the fallback), never logical
// length. Symlink/special nodes have zero size; hard links are counted per path.
// Unknown is not zero evidence.
type MacWalkEntry struct {
	Parent                 uint64
	Name                   string
	IsDir, IsRegular, Used bool
	Size, Mtime            uint64
	SizeKnown              bool
	Flags                  uint32
	AttributeError         uint32
}

type MacWalkCoverageReason string

const (
	MacWalkDataless   MacWalkCoverageReason = "dataless"
	MacWalkHung       MacWalkCoverageReason = "previous-timeout"
	MacWalkTimeout    MacWalkCoverageReason = "hard-timeout"
	MacWalkBudget     MacWalkCoverageReason = "soft-budget"
	MacWalkUnreadable MacWalkCoverageReason = "unreadable"
	MacWalkMalformed  MacWalkCoverageReason = "malformed-record"
	MacWalkMetadata   MacWalkCoverageReason = "incomplete-metadata"
	MacWalkCancelled  MacWalkCoverageReason = "cancelled"
	MacWalkCapacity   MacWalkCoverageReason = "blocked-worker-capacity"
)

// An incomplete directory's missing descendants must never prove absence.
// Index identifies the directory itself, which remains present in Entries.
type MacWalkCoverage struct {
	Index  uint64
	Path   string
	Reason MacWalkCoverageReason
	Err    error
}

type MacWalkProgress struct {
	Entries, Files, Directories, OtherEntries                                uint64
	AllocatedBytes                                                           uint64 // Regular files only; no hard-link dedup.
	IncompleteDirectories, SkippedDataless, SkippedHung, TimedOutDirectories uint64
	ExcludedMirrors, FallbackDirectories                                     uint64
}

type MacWalkResult struct {
	Volume     VolumeID
	RootIndex  uint64
	Entries    []MacWalkEntry
	Incomplete []MacWalkCoverage
	Counts     MacWalkProgress
	// Spill is set once the scan crosses the Rust 400,000-entry threshold.
	// Entries then holds only the root; the rest is the spill file, in scan
	// order, and the caller streams it into the v7 build. Nil on small scans.
	Spill *MacWalkSpill
}

// MacWalkSpill is the on-disk raw scan. Its record layout matches Rust's
// qc-spill files, so a build reads it without a second in-memory copy.
type MacWalkSpill struct{ inner macWalkSpill }

// OpenMacWalkSpill wraps a spill file another package wrote. Tests use it to
// feed the streaming build without going through a real volume walk.
func OpenMacWalkSpill(path string, count, nameBytes uint64) *MacWalkSpill {
	return &MacWalkSpill{inner: macWalkSpill{path: path, n: count, nameBytes: nameBytes}}
}

func (s *MacWalkSpill) Path() string      { return s.inner.path }
func (s *MacWalkSpill) Count() uint64     { return s.inner.n }
func (s *MacWalkSpill) NameBytes() uint64 { return s.inner.nameBytes }
func (s *MacWalkSpill) Read() (MacWalkEntry, error) {
	return s.inner.Read()
}

// Remove drops the spill file. A finished build and every abandoned scan call it.
func (s *MacWalkSpill) Remove() {
	if s != nil {
		s.inner.remove()
	}
}
