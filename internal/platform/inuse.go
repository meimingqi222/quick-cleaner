package platform

import (
	"os"
	"sync"
)

type InUseState uint8

const (
	InUseClear InUseState = iota
	InUseBusy
	InUseUnknown
)

type InUseBusyInfo struct {
	App           string
	Open, Unknown bool
}

func SpotCheckInUse(paths []string) map[string]InUseState {
	out := make(map[string]InUseState, len(paths))
	var mu sync.Mutex
	var wg sync.WaitGroup
	jobs := make(chan string)
	// Directory queries are independent: a slow recursive lsof must not turn
	// unrelated files into Unknown or serialize every directory deadline.
	for range min(8, len(paths)) {
		wg.Go(func() {
			for path := range jobs {
				state := InUseUnknown
				if _, err := os.Lstat(path); os.IsNotExist(err) {
					state = InUseClear
				} else {
					open, known := IsOpen(path)
					switch {
					case !known:
						state = InUseUnknown
					case open:
						state = InUseBusy
					default:
						state = InUseClear
					}
				}
				mu.Lock()
				out[path] = state
				mu.Unlock()
			}
		})
	}
	for _, path := range paths {
		jobs <- path
	}
	close(jobs)
	wg.Wait()
	return out
}
