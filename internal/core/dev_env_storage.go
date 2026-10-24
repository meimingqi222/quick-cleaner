package core

import (
	"os"
	"path/filepath"
	"runtime"
	"sync"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// AssetStorageSize distinguishes logical path size from exclusively reclaimable bytes.
type AssetStorageSize struct {
	LogicalBytes              uint64
	ExclusiveReclaimableBytes uint64
	FileCount                 uint64
}

type devStorageFact struct {
	length uint64
	links  uint32
	id     [2]uint64
	hasID  bool
}

type devStorageJob struct {
	path        string
	root, depth int
}

type devStorageResult struct {
	root     int
	children []devStorageJob
	fact     devStorageFact
	isFile   bool
}

// MeasureBatchStorage preserves input order for per-directory sizes. The total
// counts each native file object once, even across hardlinks or overlapping roots.
// Shared hardlinks are never treated as exclusively reclaimable, even when all
// their names occur in the batch. Unavailable identity counts as exclusive.
func MeasureBatchStorage(dirs []string) (AssetStorageSize, []AssetStorageSize) {
	perDir := make([]AssetStorageSize, len(dirs))
	facts := make([][]devStorageFact, len(dirs))
	if len(dirs) == 0 {
		return AssetStorageSize{}, perDir
	}
	workers := min(max(runtime.GOMAXPROCS(0), 2), 8)
	jobs := make(chan devStorageJob)
	results := make(chan devStorageResult, workers)
	var wg sync.WaitGroup
	for range workers {
		wg.Go(func() {
			for job := range jobs {
				results <- scanDevStorageEntry(job)
			}
		})
	}
	pending := make([]devStorageJob, len(dirs))
	for i, dir := range dirs {
		pending[i] = devStorageJob{path: dir, root: i}
	}
	// Only the coordinator queues children. Workers never wait to submit work
	// to their own pool, so wide/deep trees cannot deadlock a full queue.
	active := 0
	for len(pending) > 0 || active > 0 {
		var dispatch chan devStorageJob
		var next devStorageJob
		if len(pending) > 0 {
			dispatch = jobs
			next = pending[len(pending)-1]
		}
		select {
		case dispatch <- next:
			pending = pending[:len(pending)-1]
			active++
		case result := <-results:
			active--
			pending = append(pending, result.children...)
			if result.isFile {
				facts[result.root] = append(facts[result.root], result.fact)
			}
		}
	}
	close(jobs)
	wg.Wait()

	var total AssetStorageSize
	seen := make(map[[2]uint64]struct{})
	for i, files := range facts {
		localSeen := make(map[[2]uint64]struct{})
		for _, file := range files {
			perDir[i].LogicalBytes += file.length
			perDir[i].FileCount++
			if !file.hasID {
				perDir[i].ExclusiveReclaimableBytes += file.length
			} else if file.links <= 1 {
				if _, exists := localSeen[file.id]; !exists {
					perDir[i].ExclusiveReclaimableBytes += file.length
					localSeen[file.id] = struct{}{}
				}
			}
			if file.hasID {
				if _, exists := seen[file.id]; exists {
					continue
				}
				seen[file.id] = struct{}{}
			}
			total.LogicalBytes += file.length
			total.FileCount++
			if !file.hasID || file.links <= 1 {
				total.ExclusiveReclaimableBytes += file.length
			}
		}
	}
	return total, perDir
}

func scanDevStorageEntry(job devStorageJob) devStorageResult {
	result := devStorageResult{root: job.root}
	info, err := os.Lstat(job.path)
	if err != nil || platform.DevEnvStorageIsReparse(info) {
		return result
	}
	if info.IsDir() {
		if job.depth > 32 {
			return result
		}
		entries, _ := os.ReadDir(job.path)
		for _, entry := range entries {
			result.children = append(result.children, devStorageJob{
				path: filepath.Join(job.path, entry.Name()), root: job.root, depth: job.depth + 1,
			})
		}
		return result
	}
	if job.depth == 0 || !info.Mode().IsRegular() {
		return result
	}
	links, id, hasID := platform.DevEnvStorageFileInfo(job.path)
	result.fact = devStorageFact{uint64(info.Size()), links, id, hasID}
	result.isFile = true
	return result
}
