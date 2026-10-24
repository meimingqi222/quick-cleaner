package core

import (
	"bytes"
	"crypto/rand"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"unicode/utf8"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

const diskIndexCoverageMagic = "QCCOV001"

type diskIndexCoverage struct {
	Version       int      `json:"version"`
	BaseChecksum  uint64   `json:"base_checksum"`
	DeltaChecksum uint64   `json:"delta_checksum"`
	LastEventID   uint64   `json:"last_event_id"`
	Blocked       []string `json:"blocked"`
}

func diskIndexCoveragePath(path string, checksum uint64) string {
	return fmt.Sprintf("%s.coverage-%016x.json", path, checksum)
}

// Coverage is a snapshot of the EFFECTIVE tree, even for a metadata-only delta.
// Its canonical digest lives in reserved header bytes (base 96..104, delta
// 112..120), with QCCOV001 at 120..128. Rust ignores those bytes, but its normal
// full-file checksum still protects them. The sidecar binds that final checksum,
// the base checksum, and the event watermark without a circular hash dependency.
func canonicalDiskIndexCoverage(mount string, blocked []string) ([]string, []byte, error) {
	paths := append([]string{}, blocked...)
	for _, path := range paths {
		if !utf8.ValidString(path) || strings.ContainsRune(path, 0) || !filepath.IsAbs(path) || filepath.Clean(path) != path || !pathWithin(path, mount) {
			return nil, nil, fmt.Errorf("%w: invalid blocked subtree", ErrDiskIndexIncomplete)
		}
	}
	sort.Strings(paths)
	unique := paths[:0]
	for _, path := range paths {
		if len(unique) == 0 || unique[len(unique)-1] != path {
			unique = append(unique, path)
		}
	}
	payload, err := json.Marshal(unique)
	return unique, payload, err
}

func prepareDiskIndexCoverage(path string, index []byte, tree *SizeTree, baseChecksum, event uint64, delta bool) (string, []byte, error) {
	blocked, payload, err := canonicalDiskIndexCoverage(tree.volume.Mount, tree.blocked)
	if err != nil {
		return "", nil, err
	}
	checksumOff, digestOff := 72, 96
	if delta {
		checksumOff, digestOff = 100, 112
	} else if cacheU64(index, 104) == 0 {
		// Distinguish equal-watermark full rewrites too: an old delta must not
		// become applicable during the base-rename / delta-unlink crash window.
		if _, err := rand.Read(index[104:112]); err != nil {
			return "", nil, err
		}
	}
	copy(index[120:128], diskIndexCoverageMagic)
	cachePut64(index, digestOff, cacheChecksum(payload, len(payload)))
	checksum := cacheChecksum(index, checksumOff)
	cachePut64(index, checksumOff, checksum)
	cov := diskIndexCoverage{Version: 1, BaseChecksum: baseChecksum, LastEventID: event, Blocked: blocked}
	if delta {
		cov.DeltaChecksum = checksum
	} else {
		cov.BaseChecksum = checksum
	}
	b, err := json.Marshal(cov)
	return diskIndexCoveragePath(path, checksum), b, err
}

func publishDiskIndexCovered(path string, index []byte, tree *SizeTree, baseChecksum, event uint64, delta bool) error {
	coveragePath, coverage, err := prepareDiskIndexCoverage(path, index, tree, baseChecksum, event, delta)
	if err != nil {
		return err
	}
	// Content-addressed sidecars publish FIRST. Until the index rename, the old
	// index still selects its old immutable sidecar. A crash at any boundary sees
	// the old complete pair or the new complete pair, never new data + old coverage.
	if err := publishDiskIndex(coveragePath, coverage); err != nil {
		return err
	}
	// Do not delete the sidecar on error: rename may have succeeded before a
	// directory fsync error. Unreferenced sidecars are harmless and pruned with
	// their retired index; retaining them also preserves rollback snapshots.
	return publishDiskIndex(path, index)
}

func readDiskIndexRegular(path string) ([]byte, error) {
	before, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if !before.Mode().IsRegular() || platform.DevEnvStorageIsReparse(before) {
		return nil, cacheInvalid("cache path is not a regular file")
	}
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	after, err := f.Stat()
	if err != nil {
		return nil, err
	}
	if !os.SameFile(before, after) {
		return nil, cacheInvalid("cache file changed while opening")
	}
	// Read from the verified descriptor, not by reopening the path.
	var b bytes.Buffer
	if _, err := b.ReadFrom(f); err != nil {
		return nil, err
	}
	return b.Bytes(), nil
}

func restoreDiskIndexCoverage(path string, index []byte, tree *SizeTree, baseChecksum, event uint64, delta bool) error {
	checksumOff, digestOff := 72, 96
	if delta {
		checksumOff, digestOff = 100, 112
	}
	if string(index[120:128]) != diskIndexCoverageMagic {
		if !bytes.Equal(index[120:128], make([]byte, 8)) || cacheU64(index, digestOff) != 0 {
			return fmt.Errorf("%w: unknown coverage extension", ErrDiskIndexIncomplete)
		}
		// Legacy Rust indexes carry no evidence about skipped trees. They remain
		// readable for display/replay, but cannot establish complete coverage.
		tree.SetBlockedSubtrees([]string{tree.volume.Mount})
		return nil
	}
	checksum := cacheU64(index, checksumOff)
	b, err := readDiskIndexRegular(diskIndexCoveragePath(path, checksum))
	if err != nil {
		return fmt.Errorf("%w: required coverage unavailable: %v", ErrDiskIndexIncomplete, err)
	}
	var cov diskIndexCoverage
	decoder := json.NewDecoder(bytes.NewReader(b))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&cov); err != nil {
		return fmt.Errorf("%w: coverage JSON", ErrDiskIndexIncomplete)
	}
	// Require canonical encoding too: reject duplicate fields, trailing data,
	// null-vs-empty ambiguity, unsorted paths, and changes outside the path digest.
	canonical, err := json.Marshal(cov)
	if err != nil || !bytes.Equal(canonical, b) {
		return fmt.Errorf("%w: noncanonical coverage", ErrDiskIndexIncomplete)
	}
	expectedBase, expectedDelta := checksum, uint64(0)
	if delta {
		expectedBase, expectedDelta = baseChecksum, checksum
	}
	if cov.Version != 1 || cov.BaseChecksum != expectedBase || cov.DeltaChecksum != expectedDelta || cov.LastEventID != event {
		return fmt.Errorf("%w: coverage binding mismatch", ErrDiskIndexIncomplete)
	}
	blocked, payload, err := canonicalDiskIndexCoverage(tree.volume.Mount, cov.Blocked)
	if err != nil {
		return err
	}
	canonicalBlocked, _ := json.Marshal(cov.Blocked)
	if !bytes.Equal(payload, canonicalBlocked) || cacheChecksum(payload, len(payload)) != cacheU64(index, digestOff) {
		return fmt.Errorf("%w: coverage digest mismatch", ErrDiskIndexIncomplete)
	}
	tree.SetBlockedSubtrees(blocked)
	return nil
}
