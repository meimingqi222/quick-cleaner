package core

import (
	"math"
	"testing"
	"time"
)

func TestBrewPreviewSupportsBothFormatsAndUnknownCompletion(t *testing.T) {
	for _, tc := range []struct {
		text         string
		bytes, files uint64
		present      bool
		completion   Evidence
	}{
		{"Pruning symlinks...\nWould remove: /opt/homebrew/Cellar/foo/1.0\nWould remove: /opt/homebrew/Cellar/bar/2.0\n==> This operation would free approximately 140.9MB of disk space.\n", 140900000, 2, true, Absent},
		{"Prune: /usr/local/Cellar/foo/1.0\nThis operation has freed approximately 512KB of disk space.\n", 512000, 1, true, Absent},
		{"==> This operation would free approximately 2.1MB of disk space.\n", 2100000, 1, true, Absent},
		{"Removing: /opt/homebrew/Cellar/foo/1.0\n", 0, 1, true, Absent},
		{"", 0, 0, false, Confirmed},
		{"Pruning symlinks...\n", 0, 0, false, Confirmed},
		{"This operation has freed approximately 0KB of disk space.\n", 0, 0, false, Confirmed},
		{"Would remove: /opt/homebrew/Cellar/foo/1.0\n==> This operation would free approximately 0B of disk space.\n", 0, 0, false, Absent},
		{"changed output format", 0, 0, false, Unknown},
		{"==> This operation would free approximately invalid", 0, 0, false, Unknown},
		{"==> This operation would free approximately -1B of disk space.", 0, 0, false, Unknown},
		{"==> This operation would free approximately 0.1B of disk space.", 0, 0, false, Unknown},
		{"\xff", 0, 0, false, Unknown},
	} {
		bytes, files, present := ParseBrewCleanupOutput(tc.text)
		if bytes != tc.bytes || files != tc.files || present != tc.present || BrewPreviewCompletion([]byte(tc.text)) != tc.completion {
			t.Fatalf("preview %q: %d %d %v %v", tc.text, bytes, files, present, BrewPreviewCompletion([]byte(tc.text)))
		}
	}
	now := time.Now().Unix()
	for _, tc := range []struct {
		last int64
		want bool
	}{{now + 1, false}, {now - BrewThrottleDays*86400 + 1, false}, {now - BrewThrottleDays*86400, true}, {math.MinInt64, true}, {math.MaxInt64, false}} {
		if got := ShouldOfferBrewCleanup(&tc.last); got != tc.want {
			t.Fatal(tc, got)
		}
	}
	if !ShouldOfferBrewCleanup(nil) {
		t.Fatal("no previous cleanup must be offered")
	}
}
