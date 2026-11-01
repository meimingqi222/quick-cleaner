package app

import (
	"errors"
	"strings"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func TestCacheReleaseFailureTreatsSystemOwnersAsSkipped(t *testing.T) {
	r, _ := testRoot(t)
	r.settings.Language = core.English
	for _, c := range []struct{ err, want string }{
		{"service-or-critical-owner", "system, another user"},
		{"system-or-other-user-process", "system, another user"},
		{"no-confirmed-cache-owners; retry-or-rescan", "rescan"},
		{"cache-owners-changed; review-required", "rescan"},
		{"boom", "boom"},
	} {
		r.cacheReleaseFailure(errors.New(c.err))
		if !strings.Contains(strings.ToLower(r.dashboard.StatusText), c.want) {
			t.Fatalf("%q -> %q; want %q", c.err, r.dashboard.StatusText, c.want)
		}
	}
}
