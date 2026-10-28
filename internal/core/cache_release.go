package core

import (
	"errors"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type CacheOccupant = platform.CacheOccupant

// CacheReleasePlan is a confirmation capability. Its accessors return copies;
// changing the confirmation display must not change execution authority.
type CacheReleasePlan struct {
	item   ScanItem
	owners []CacheOccupant
}

func (p CacheReleasePlan) Item() ScanItem { return freezeCacheReleaseItem(p.item) }
func (p CacheReleasePlan) Occupants() []CacheOccupant {
	return append([]CacheOccupant(nil), p.owners...)
}

type CacheReleaseOutcome struct {
	// Cleaned is non-nil only when the ordinary cleaner ran. Its report can
	// still contain failures; sending Quit is never reported as a clean.
	Cleaned   *CleanReport
	StillBusy bool
}

func CacheReleaseEligible(item ScanItem) bool {
	category := item.Category == UserCache || item.Category == BrowserCache || item.Category == Logs
	recommended := item.Recommended || item.Busy != nil && item.Busy.RecommendedBeforeBusy
	return category && recommended && (item.Busy == nil || !item.Busy.Unknown) && item.Operation == (Operation{Kind: OpContents}) && item.Disposal == Permanent && item.Identity != nil && len(item.Plans) != 0 && item.Rule.blocked == ""
}

func freezeCacheReleaseItem(item ScanItem) ScanItem {
	item.Plans = append([]*CleanupPlan(nil), item.Plans...)
	if item.Identity != nil {
		id := *item.Identity
		item.Identity = &id
	}
	if item.Busy != nil {
		busy := *item.Busy
		item.Busy = &busy
	}
	return item
}

func validateCacheRelease(item ScanItem) error {
	if !CacheReleaseEligible(item) || IsContentsProtected(item.Path) {
		return errors.New("cache-release-not-authorized")
	}
	if !item.Identity.Recheck(item.Path) {
		return errors.New("cache-target-changed; rescan-required")
	}
	if err := item.Rule.Revalidate(); err != nil {
		return err
	}
	for _, p := range item.Plans {
		if p == nil {
			return errors.New("cache-plan-missing")
		}
		if err := p.ValidateBinding(item.Path, item.Operation, item.Disposal, item.Identity); err != nil {
			return err
		}
	}
	return nil
}

func PrepareCacheRelease(item ScanItem) (CacheReleasePlan, error) {
	item = freezeCacheReleaseItem(item)
	if err := validateCacheRelease(item); err != nil {
		return CacheReleasePlan{}, err
	}
	owners, err := platform.CacheReleaseOccupants(item.Path)
	if err != nil {
		return CacheReleasePlan{}, err
	}
	if len(owners) == 0 || len(owners) > 16 {
		return CacheReleasePlan{}, errors.New("no-confirmed-cache-owners; retry-or-rescan")
	}
	return CacheReleasePlan{item: item, owners: append([]CacheOccupant(nil), owners...)}, nil
}

func cacheConsentCovers(approved, current []CacheOccupant) bool {
	for _, owner := range current {
		found := false
		for _, a := range approved {
			if owner.PID == a.PID && owner.Identity != 0 && owner.Identity == a.Identity && owner.Executable != "" && owner.Executable == a.Executable {
				found = true
				break
			}
		}
		if !found {
			return false
		}
	}
	return true
}

func ExecuteCacheRelease(plan CacheReleasePlan, force bool, progress *CleanProgress) (CacheReleaseOutcome, error) {
	return executeCacheRelease(plan, force, progress, platform.CacheReleaseOccupants, platform.RequestCacheOwnerExit, 8*time.Second, CleanTargets)
}

func executeCacheRelease(plan CacheReleasePlan, force bool, progress *CleanProgress, probe func(string) ([]CacheOccupant, error), exit func(CacheOccupant, bool) error, wait time.Duration, clean func([]CleanTarget, *CleanProgress) CleanReport) (CacheReleaseOutcome, error) {
	if progress == nil {
		return CacheReleaseOutcome{}, errors.New("cache-progress-required")
	}
	check := func() error {
		if progress.Cancelled() {
			return errors.New("cancelled")
		}
		return validateCacheRelease(plan.item)
	}
	if err := check(); err != nil {
		return CacheReleaseOutcome{}, err
	}
	if len(plan.owners) == 0 || len(plan.owners) > 16 {
		return CacheReleaseOutcome{}, errors.New("cache-consent-missing")
	}
	current, err := probe(plan.item.Path)
	if err != nil {
		return CacheReleaseOutcome{}, err
	}
	if !cacheConsentCovers(plan.owners, current) {
		return CacheReleaseOutcome{}, errors.New("cache-owners-changed; review-required")
	}
	for _, owner := range current {
		if err := check(); err != nil {
			return CacheReleaseOutcome{}, err
		}
		if err := exit(owner, force); err != nil {
			return CacheReleaseOutcome{}, err
		}
	}
	deadline := time.Now().Add(wait)
	for {
		if err := check(); err != nil {
			return CacheReleaseOutcome{}, err
		}
		current, err = probe(plan.item.Path)
		if err != nil {
			return CacheReleaseOutcome{}, err
		}
		if !cacheConsentCovers(plan.owners, current) {
			return CacheReleaseOutcome{}, errors.New("cache-owners-changed; review-required")
		}
		if len(current) == 0 {
			break
		}
		if !time.Now().Before(deadline) {
			return CacheReleaseOutcome{StillBusy: true}, nil
		}
		time.Sleep(min(300*time.Millisecond, time.Until(deadline)))
	}
	if err := check(); err != nil {
		return CacheReleaseOutcome{}, err
	}
	target := plan.item.CleanTarget()
	target.SizeHint = nil
	report := clean([]CleanTarget{target}, progress)
	return CacheReleaseOutcome{Cleaned: &report}, nil
}
