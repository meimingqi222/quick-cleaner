package platform

import (
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
)

const (
	fseventMustScan uint32 = 1 << iota
	fseventUserDropped
	fseventKernelDropped
	fseventIDsWrapped
	fseventHistoryDone
	fseventRootChanged
)

const fseventSinceNow = ^uint64(0)

// FSEventChanges contains delivered history, not file sizes or deletion authority.
// An incomplete batch advances only to its largest delivered ID.
type FSEventChanges struct {
	Paths, MustRescan                  []string
	LastEventID                        uint64
	RequiresFullScan                   bool
	FullScanReason                     string
	FilteredCacheEvents, RawEventCount int
	Incomplete                         bool
}

// CanonicalFSEventPath folds the Data-volume mirror, without resolving symlinks
// or changing unrelated paths. The synthetic root must not acquire mirror nodes.
func CanonicalFSEventPath(path string) string {
	const mirror = "/System/Volumes/Data"
	if path == mirror {
		return "/"
	}
	if strings.HasPrefix(path, mirror+"/") {
		return strings.TrimPrefix(path, mirror)
	}
	return path
}

type fseventEntry struct {
	path  string
	flags uint32
}

type fseventCollector struct {
	root                    string
	events                  []fseventEntry
	maxID                   uint64
	historyDone, rootRescan bool
}

func (c *fseventCollector) add(path string, flags uint32, id uint64) {
	c.maxID = max(c.maxID, id)
	if path != "" {
		if flags&(fseventMustScan|fseventUserDropped|fseventKernelDropped) != 0 && CanonicalFSEventPath(path) == c.root {
			c.rootRescan = true
		}
		if !c.rootRescan {
			c.events = append(c.events, fseventEntry{path, flags})
		}
	}
	c.historyDone = c.historyDone || flags&fseventHistoryDone != 0
}

func (c *fseventCollector) changes(since, latest uint64, cacheDir string, children []string, checkpoint uint64) (*FSEventChanges, error) {
	result := &FSEventChanges{LastEventID: latest, RawEventCount: len(c.events)}
	if c.rootRescan {
		result.LastEventID = since
		if len(children) == 0 {
			result.RequiresFullScan, result.FullScanReason = true, "RootMustScanSubDirs"
		} else {
			result.MustRescan, result.LastEventID = sortedFSEventPaths(children), checkpoint
		}
		return result, nil
	}
	if since != fseventSinceNow && !c.historyDone {
		if c.maxID <= since {
			return nil, errors.New("FSEvents history replay made no progress")
		}
		result.Incomplete, result.LastEventID = true, c.maxID
	}
	for _, event := range c.events {
		if cacheDir != "" && (event.path == cacheDir || strings.HasPrefix(event.path, strings.TrimRight(cacheDir, "/")+"/")) {
			result.FilteredCacheEvents++
			continue
		}
		if event.flags&(fseventMustScan|fseventUserDropped|fseventKernelDropped) != 0 {
			result.MustRescan = append(result.MustRescan, event.path)
		}
		if event.flags&fseventIDsWrapped != 0 && result.FullScanReason == "" {
			result.RequiresFullScan, result.FullScanReason = true, "EventIdsWrapped"
		}
		if event.flags&fseventRootChanged != 0 && result.FullScanReason == "" {
			result.RequiresFullScan, result.FullScanReason = true, "RootChanged"
		}
		result.Paths = append(result.Paths, event.path)
	}
	result.Paths = sortedFSEventPaths(result.Paths)
	result.MustRescan = sortedFSEventPaths(result.MustRescan)
	return result, nil
}

func sortedFSEventPaths(paths []string) []string {
	for i := range paths {
		paths[i] = CanonicalFSEventPath(paths[i])
	}
	slices.Sort(paths)
	return slices.Compact(paths)
}

func fseventDirectChildren(root string) []string {
	entries, err := os.ReadDir(root)
	if err != nil {
		return nil
	}
	var paths []string
	for _, entry := range entries {
		path := filepath.Join(root, entry.Name())
		// Rust Path::is_dir follows the final link for this root expansion.
		if info, err := os.Stat(path); err == nil && info.IsDir() {
			paths = append(paths, path)
		}
	}
	return sortedFSEventPaths(paths)
}
