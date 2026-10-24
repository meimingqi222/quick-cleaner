package platform

import (
	"errors"
	"time"
)

const MFTRootRecord uint64 = 5

var (
	ErrMFTUnsupported = errors.New("MFT requires an NTFS Windows volume")
	ErrMFTCorrupt     = errors.New("invalid NTFS MFT metadata")
)

// MFTEntry is a raw record, indexed by record number in MFTScanResult.Entries.
// Extension records retain BaseRecord and Size but are not Used tree nodes.
// Size is the unnamed stream's logical length, not allocation or named ADS size.
// Consumers merge extension sizes into their base record before aggregation.
type MFTEntry struct {
	Parent      uint64
	Name        string
	IsDir       bool
	Size        uint64
	Used        bool
	Mtime       uint64 // Unix seconds from STANDARD_INFORMATION; zero if unavailable.
	BaseRecord  uint64
	LinkParents []uint64 // Distinct FILE_NAME parents, including the selected parent.
}

// MFTTimings matches the Rust volume-scan phases so the two logs can be compared.
// Read and Parse are wall time of the chunk loop, not a sum across cores.
type MFTTimings struct {
	Open, Volume, First, Runs, Read, Parse time.Duration
}

type MFTScanResult struct {
	Volume                                   VolumeID
	RootRecord                               uint64
	Entries                                  []MFTEntry
	RecordsExpected, MFTRunBytes, ExtRecords uint64
	BytesRead                                uint64
	Timings                                  MFTTimings
}
