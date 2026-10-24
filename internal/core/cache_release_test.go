package core

import (
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func cacheReleaseFixture(t *testing.T) CacheReleasePlan {
	t.Helper()
	path := filepath.Join(t.TempDir(), "cache")
	if err := os.Mkdir(path, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(path, "sentinel"), []byte("retain until vacancy"), 0600); err != nil {
		t.Fatal(err)
	}
	id, ok := CaptureIdentity(path)
	if !ok {
		t.Fatal("identity unavailable")
	}
	rule := EngineRuleRef()
	item := ScanItem{Path: path, Operation: Operation{Kind: OpContents}, Disposal: Permanent, Category: UserCache, Recommended: true, Identity: &id, Rule: rule}
	item = item.FreezePlan()
	if err := validateCacheRelease(item); err != nil {
		t.Fatal(err)
	}
	return CacheReleasePlan{item: freezeCacheReleaseItem(item), owners: []CacheOccupant{{PID: 40, Identity: 1, Executable: "/Applications/Example.app/Contents/MacOS/Example", Name: "Example", Evidence: filepath.Join(path, "sentinel")}}}
}

func TestCacheReleaseKnownVacancyRetainsOriginalPlan(t *testing.T) {
	p := cacheReleaseFixture(t)
	for _, force := range []bool{false, true} {
		calls, exits := 0, 0
		out, err := executeCacheRelease(p, force, &CleanProgress{}, func(string) ([]CacheOccupant, error) {
			calls++
			if calls == 1 {
				return p.Occupants(), nil
			}
			return nil, nil
		}, func(o CacheOccupant, f bool) error {
			exits++
			if f != force || o != p.owners[0] {
				t.Fatal("changed consent")
			}
			return nil
		}, 0, func(targets []CleanTarget, _ *CleanProgress) CleanReport {
			if len(targets) != 1 || targets[0].Plans[0] != p.item.Plans[0] || targets[0].Identity == nil || *targets[0].Identity != *p.item.Identity || targets[0].Path != p.item.Path || targets[0].RemoveDir || targets[0].Disposal != Permanent || targets[0].Operation != p.item.Operation {
				t.Fatal("cleanup authority changed")
			}
			return CleanReport{}
		})
		if err != nil || out.Cleaned == nil || out.StillBusy || exits != 1 || calls != 2 {
			t.Fatalf("out=%+v err=%v exits=%d probes=%d", out, err, exits, calls)
		}
	}
}

func TestCacheReleaseOrdinaryCleanerRechecksDisposableContents(t *testing.T) {
	p := cacheReleaseFixture(t)
	progress := &CleanProgress{}
	occupancy := SpotCheckInUse([]string{p.item.Path})[p.item.Path]
	out, err := executeCacheRelease(p, false, progress, func(string) ([]CacheOccupant, error) {
		return nil, nil
	}, func(CacheOccupant, bool) error {
		t.Fatal("vacant cache requested an exit")
		return nil
	}, 0, CleanTargets)
	if err != nil || out.Cleaned == nil || out.StillBusy {
		t.Fatalf("ordinary clean: %+v %v", out, err)
	}
	// Linux /proc visibility can be incomplete in an orb. A release probe's
	// vacancy must not override the unchanged cleaner's independent uncertainty.
	if occupancy != SpotClear {
		if len(out.Cleaned.Failed) != 1 || len(out.Cleaned.FailInfo) != 1 {
			t.Fatalf("ordinary occupancy gate bypassed: %+v", out.Cleaned)
		}
		if _, err := os.Stat(filepath.Join(p.item.Path, "sentinel")); err != nil {
			t.Fatal("unknown ordinary vacancy lost sentinel", err)
		}
		return
	}
	if len(out.Cleaned.Failed) != 0 {
		t.Fatalf("known clear ordinary clean failed: %+v", out.Cleaned)
	}
	entries, err := os.ReadDir(p.item.Path)
	if err != nil || len(entries) != 0 {
		t.Fatalf("contents not cleaned, or root removed: %v %v", entries, err)
	}
}

func TestCacheReleaseRefusalsNeverCleanOrAutoForce(t *testing.T) {
	for _, scenario := range []string{"busy", "unknown", "new", "reused", "executable", "zero-id", "wait-unknown", "wait-new", "cancel-before", "cancel-probe", "cancel-exit", "cancel-vacancy", "changed-before", "changed-exit", "changed-vacancy", "invalid-plan", "exit-error"} {
		t.Run(scenario, func(t *testing.T) {
			p := cacheReleaseFixture(t)
			progress := &CleanProgress{}
			changed := false
			replace := func() {
				if changed {
					return
				}
				changed = true
				if err := os.Rename(p.item.Path, p.item.Path+"-old"); err != nil {
					t.Fatal(err)
				}
				if err := os.Mkdir(p.item.Path, 0700); err != nil {
					t.Fatal(err)
				}
			}
			if scenario == "cancel-before" {
				progress.RequestCancel()
			}
			if scenario == "changed-before" {
				replace()
			}
			if scenario == "invalid-plan" {
				p.item.Plans = []*CleanupPlan{NewCleanupPlan(EngineRuleRef(), nil)}
			}
			probes, exits := 0, 0
			out, err := executeCacheRelease(p, false, progress, func(string) ([]CacheOccupant, error) {
				probes++
				owners := p.Occupants()
				switch scenario {
				case "unknown":
					return nil, errors.New("probe unknown")
				case "new":
					owners[0].PID++
				case "reused":
					owners[0].Identity++
				case "executable":
					owners[0].Executable = "/other"
				case "zero-id":
					owners[0].Identity = 0
				case "cancel-probe":
					progress.RequestCancel()
				case "wait-unknown":
					if probes > 1 {
						return nil, errors.New("vacancy uncertain")
					}
				case "wait-new":
					if probes > 1 {
						owners[0].PID++
					}
				case "cancel-vacancy":
					if probes > 1 {
						progress.RequestCancel()
						return nil, nil
					}
				case "changed-vacancy":
					if probes > 1 {
						replace()
						return nil, nil
					}
				}
				return owners, nil
			}, func(_ CacheOccupant, force bool) error {
				exits++
				if force {
					t.Fatal("auto-force")
				}
				if scenario == "cancel-exit" {
					progress.RequestCancel()
				}
				if scenario == "changed-exit" {
					replace()
				}
				if scenario == "exit-error" {
					return errors.New("quit failed")
				}
				return nil
			}, 0, func([]CleanTarget, *CleanProgress) CleanReport { t.Fatal("unsafe clean"); return CleanReport{} })
			if scenario == "busy" {
				if err != nil || !out.StillBusy || exits != 1 {
					t.Fatalf("busy: %+v %v", out, err)
				}
			} else if err == nil {
				t.Fatalf("missing refusal: %+v", out)
			}
			if out.Cleaned != nil {
				t.Fatal("refusal reported clean")
			}
			if scenario == "new" || scenario == "reused" || scenario == "unknown" || scenario == "executable" || scenario == "zero-id" || scenario == "cancel-before" || scenario == "cancel-probe" || scenario == "changed-before" || scenario == "invalid-plan" {
				if exits != 0 {
					t.Fatal("refusal sent exit")
				}
			}
			root := p.item.Path
			if changed {
				root += "-old"
			}
			if _, err := os.Stat(filepath.Join(root, "sentinel")); err != nil {
				t.Fatal("sentinel lost", err)
			}
		})
	}
}

func TestCacheReleaseConfirmationCopiesCannotExpandAuthority(t *testing.T) {
	p := cacheReleaseFixture(t)
	item := p.Item()
	item.Path = "/other"
	item.Identity.ino++
	item.Busy = &Busy{Unknown: true}
	item.Plans[0] = nil
	owners := p.Occupants()
	owners[0].PID++
	if err := validateCacheRelease(p.item); err != nil {
		t.Fatal(err)
	}
	if p.owners[0].PID != 40 || p.item.Plans[0] == nil {
		t.Fatal("confirmation changed authority")
	}
	_, err := executeCacheRelease(CacheReleasePlan{}, false, &CleanProgress{}, func(string) ([]CacheOccupant, error) { t.Fatal("zero plan probed"); return nil, nil }, nil, time.Second, nil)
	if err == nil {
		t.Fatal("zero plan accepted")
	}
}
