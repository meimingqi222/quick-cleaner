package app

import (
	"sync"
	"testing"
)

func TestTrashSeenConcurrentClaims(t *testing.T) {
	r, _ := testRoot(t)
	var workers sync.WaitGroup
	claims := make(chan bool, 32)
	start := make(chan struct{})
	for range 32 {
		workers.Go(func() { <-start; claims <- r.markTrashSeen("com.example.App") })
	}
	close(start)
	workers.Wait()
	close(claims)
	count := 0
	for claimed := range claims {
		if claimed {
			count++
		}
	}
	if count != 1 {
		t.Fatalf("notification claimed %d times, want 1", count)
	}
}
