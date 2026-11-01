package cli

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type junkOptions struct {
	commonOptions
	root                                string
	paths, categories, excludes         stringsFlag
	recommended, all, execute, discover bool
	closeApps                           bool
	discoveryBudget                     time.Duration
}

type planRow struct {
	Path        string                 `json:"path"`
	Label       string                 `json:"label"`
	Category    core.CategoryID        `json:"category"`
	Bytes       uint64                 `json:"bytes"`
	Files       uint64                 `json:"files"`
	Recommended bool                   `json:"recommended"`
	Selected    bool                   `json:"selected"`
	Blocked     string                 `json:"blocked,omitempty"`
	Busy        *core.Busy             `json:"busy,omitempty"`
	Operation   core.Operation         `json:"operation"`
	Disposal    core.Disposal          `json:"disposal"`
	Rules       []core.RuleObservation `json:"rules,omitempty"`
	Plans       []planPreview          `json:"plans,omitempty"`
}

type planPreview struct {
	Targets []targetPreview `json:"targets"`
	Steps   []core.PlanStep `json:"steps"`
}

type targetPreview struct {
	Path       string                   `json:"path"`
	Scope      core.TargetScope         `json:"scope"`
	Completion core.CompletionCondition `json:"completion"`
	Disposal   core.Disposal            `json:"disposal"`
}

type cleanupReport struct {
	OK             uint                  `json:"ok"`
	Skipped        uint                  `json:"skipped"`
	Failed         []core.CleanFailure   `json:"failed"`
	Manual         []core.CleanFailure   `json:"manual"`
	SkippedItems   []core.CleanFailure   `json:"skipped_items"`
	Failures       []core.TargetFailInfo `json:"target_failures"`
	SkippedTargets []core.TargetFailInfo `json:"skipped_targets"`
	Remaining      []core.RemainingLive  `json:"remaining"`
	Executions     []core.PlanExecution  `json:"executions"`
	DeletedFiles   uint64                `json:"deleted_files"`
	FreedBytes     uint64                `json:"freed_bytes"`
	// RunningApps lists owner apps found running; Closed says whether they
	// were asked to quit (--close-apps) or their caches were left out.
	RunningApps []runningAppReport `json:"running_apps,omitempty"`
}

type runningAppReport struct {
	Name         string   `json:"name"`
	Paths        []string `json:"paths"`
	Closed       bool     `json:"closed"`
	StillRunning bool     `json:"still_running,omitempty"`
}

type junkReport struct {
	Schema    int            `json:"schema_version"`
	Command   string         `json:"command"`
	DryRun    bool           `json:"dry_run"`
	Partial   bool           `json:"partial"`
	Cancelled bool           `json:"cancelled"`
	ElapsedMS int64          `json:"elapsed_ms"`
	Selected  int            `json:"selected_count"`
	Skipped   int            `json:"skipped_count"`
	Bytes     uint64         `json:"selected_bytes"`
	Items     []planRow      `json:"items"`
	Cleanup   *cleanupReport `json:"cleanup,omitempty"`
}

func loadSettings() (core.Settings, error) {
	s := core.DefaultSettings()
	path, ok := core.SettingsPath()
	if !ok {
		return s, errors.New("original user settings directory unavailable")
	}
	data, err := os.ReadFile(path)
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		return s, err
	}
	if err == nil {
		if err := json.Unmarshal(data, &s); err != nil {
			return s, fmt.Errorf("invalid settings; refusing to ignore exclusions: %w", err)
		}
	}
	return s, nil
}

func runJunk(parent context.Context, args []string, out, errOut io.Writer) (int, error) {
	if len(args) == 0 || args[0] == "--help" || args[0] == "-h" {
		_, err := fmt.Fprint(out, help)
		return ExitOK, err
	}
	action := args[0]
	if !slices.Contains([]string{"scan", "plan", "clean", "categories"}, action) {
		return ExitUsage, fmt.Errorf("unknown junk command %q", action)
	}
	f := flags("junk "+action, errOut)
	o := junkOptions{}
	o.bind(f)
	f.StringVar(&o.root, "root", "", "restrict filesystem targets and build discovery to this directory; excludes native resources")
	f.Var(&o.paths, "path", "select an exact scanned target path (repeatable)")
	f.Var(&o.categories, "category", "filter by category ID from junk categories (repeatable)")
	f.Var(&o.excludes, "exclude", "add an in-memory exclusion without changing settings (repeatable)")
	f.BoolVar(&o.recommended, "recommended", false, "select recommended, non-blocked items (preview default)")
	f.BoolVar(&o.closeApps, "close-apps", false, "ask running owner apps (e.g. browsers) to quit before cleaning their caches; without it those caches are skipped")
	f.BoolVar(&o.all, "all", false, "select all eligible scan results, including non-recommended items")
	f.BoolVar(&o.execute, "execute", false, "perform cleanup; requires clean and an explicit selection")
	f.BoolVar(&o.discover, "discover", true, "include build-artifact discovery")
	f.DurationVar(&o.discoveryBudget, "discovery-budget", core.DiscoveryBudget, "bounded discovery time; expiry reports partial coverage")
	if code, err := parse(f, args[1:]); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return ExitOK, nil
		}
		return code, err
	}
	if o.timeout <= 0 || o.discoveryBudget <= 0 || o.execute && action != "clean" || o.all && o.recommended || len(o.paths) > 0 && (o.all || o.recommended) {
		return ExitUsage, errors.New("invalid timeout, execution mode, or conflicting selection; use --help")
	}
	if o.execute && len(o.paths) == 0 && !o.all && !o.recommended {
		return ExitUsage, errors.New("--execute requires explicit --path, --recommended or --all")
	}
	for _, category := range o.categories {
		if !slices.Contains(core.AllCategories(), core.CategoryID(category)) {
			return ExitUsage, fmt.Errorf("unknown category %q", category)
		}
	}
	if action == "categories" {
		type categoryRow struct {
			ID     core.CategoryID     `json:"id"`
			Policy core.CategoryPolicy `json:"policy"`
		}
		rows := []categoryRow{}
		for _, id := range core.AllCategories() {
			rows = append(rows, categoryRow{id, id.Policy()})
		}
		if o.json {
			return ExitOK, writeJSON(out, struct {
				Schema     int           `json:"schema_version"`
				Categories []categoryRow `json:"categories"`
			}{1, rows})
		}
		for _, row := range rows {
			if _, err := fmt.Fprintf(out, "%s\t%s\t%s\n", row.ID, row.Policy.Safety, row.Policy.Group); err != nil {
				return ExitError, err
			}
		}
		return ExitOK, nil
	}
	if o.root != "" {
		root, err := cleanRoot(o.root)
		if err != nil {
			return ExitUsage, err
		}
		o.root = root
	}
	if o.execute && runtime.GOOS != "windows" && runtime.GOOS != "darwin" {
		return ExitError, errors.New("cleanup execution requires Windows or macOS")
	}
	settings, err := loadSettings()
	if err != nil {
		return ExitError, err
	}
	oldWhitelist := core.CurrentWhitelist()
	defer core.ReloadWhitelist(oldWhitelist)
	if !core.ReloadWhitelist(append(slices.Clone(settings.Whitelist), o.excludes...)) {
		return ExitError, errors.New("exclusions could not be fully resolved")
	}
	home, ok := platform.UserHome()
	if !ok {
		return ExitError, errors.New("original user home unavailable")
	}
	ctx, cancel := context.WithTimeout(parent, o.timeout)
	defer cancel()
	started := time.Now()
	items, partial, err := scanJunk(ctx, home, settings, o)
	if err != nil {
		if code := contextCode(ctx); code != ExitOK {
			return code, err
		}
		return ExitError, err
	}
	selected := make([]bool, len(items))
	if ctx.Err() == nil {
		selected, err = selectJunk(items, o)
		if err != nil {
			return ExitUsage, err
		}
	}
	r := junkReport{Schema: 1, Command: "junk " + action, DryRun: true, Partial: partial, Items: []planRow{}}
	for i, item := range items {
		row := planRow{Path: item.Path, Label: item.Label.Get(settings.Language), Category: item.Category, Bytes: item.Size, Files: item.FileCount, Recommended: item.Recommended, Selected: selected[i], Blocked: item.Rule.BlockedReason(), Busy: item.Busy, Operation: item.Operation, Disposal: item.Disposal}
		if action != "scan" {
			row.Rules = item.Rule.Observations()
			for _, plan := range item.Plans {
				preview := planPreview{Steps: plan.Steps(), Targets: []targetPreview{}}
				for i, target := range plan.Targets() {
					scope, _ := plan.Scope(i)
					preview.Targets = append(preview.Targets, targetPreview{Path: target.Path, Scope: scope, Completion: target.Completion(), Disposal: target.Disposal})
				}
				row.Plans = append(row.Plans, preview)
			}
		}
		r.Items = append(r.Items, row)
		categoryMatches := len(o.categories) == 0 || slices.Contains(o.categories, string(item.Category))
		pathMatches := len(o.paths) == 0 || slices.ContainsFunc(o.paths, func(path string) bool {
			return targetKey(path) == targetKey(item.Path)
		})
		if !item.CleanupAvailable() && categoryMatches && pathMatches {
			r.Skipped++
		}
		if selected[i] {
			r.Selected++
			r.Bytes += item.Size
		}
	}
	code := contextCode(ctx)
	var operationErr error
	if o.execute && code == ExitOK && partial {
		code = ExitIncomplete
		operationErr = errors.New("discovery incomplete; refusing to execute a partial scan; narrow --root or increase the budget")
	}
	if o.execute && code == ExitOK {
		r.Cleanup = executeJunk(ctx, items, selected, r.Bytes, o.closeApps)
		r.DryRun = false
		code = contextCode(ctx)
		if code == ExitOK && len(r.Cleanup.Failed)+len(r.Cleanup.Manual) > 0 {
			code = ExitIncomplete
		}
	}
	r.Cancelled = ctx.Err() != nil
	r.Partial = r.Partial || r.Cancelled
	r.ElapsedMS = time.Since(started).Milliseconds()
	if o.json {
		err = writeJSON(out, r)
	} else {
		err = printJunk(out, r)
	}
	if err != nil {
		return ExitError, err
	}
	if code != ExitOK {
		return code, errors.Join(operationErr, ctx.Err())
	}
	if partial {
		return ExitIncomplete, nil
	}
	return ExitOK, nil
}

func scanJunk(ctx context.Context, home string, settings core.Settings, o junkOptions) ([]core.ScanItem, bool, error) {
	rules, err := core.CurrentRules()
	if err != nil {
		return nil, false, err
	}
	osName := runtime.GOOS
	if osName == "darwin" {
		osName = "macos"
	}
	targets := core.FilesystemScanTargets(rules, core.UserRuleRoots(home), osName)
	if o.root == "" {
		targets = append(targets, core.DockerScanTargets(rules)...)
		if runtime.GOOS == "darwin" {
			targets = append(targets, core.MacOSSnapshotTargets(rules)...)
			targets = append(targets, core.BrewScanTargets(rules, settings.BrewCleanupAt)...)
		}
	} else {
		targets = slices.DeleteFunc(targets, func(t core.ScanTarget) bool { return core.IsVirtualPath(t.Path) || !under(t.Path, o.root) })
	}
	cats := core.ScanFixed(ctx, targets, nil)
	partial := false
	if o.discover && ctx.Err() == nil {
		bounded, cancel := context.WithTimeout(ctx, o.discoveryBudget)
		var discovered []core.ScanItem
		if o.root != "" {
			discovered = core.DiscoverBuildArtifacts(bounded, rules, []core.CodeRoot{{Path: o.root, MaxDepth: 12}}, nil)
		} else {
			plan := core.BeginJunkIndex(bounded, false, home, platform.ListVolumes(), nil, targets, nil)
			discovered = plan.Discover(bounded, rules)
		}
		partial = ctx.Err() == nil && errors.Is(bounded.Err(), context.DeadlineExceeded)
		cancel()
		core.MergeDiscovered(cats, core.DropVanishedDiscovered(discovered), partial)
	}
	var items []core.ScanItem
	for _, cat := range cats {
		items = append(items, cat.Items...)
	}
	if ctx.Err() != nil {
		return items, true, nil
	}
	paths := make([]string, len(items))
	for i, item := range items {
		paths[i] = item.Path
	}
	core.ApplyBusy(cats, core.DetectInUse(paths))
	items = nil
	for _, cat := range cats {
		items = append(items, cat.Items...)
	}
	return items, partial, nil
}

func under(path, root string) bool {
	rel, err := filepath.Rel(root, path)
	return err == nil && rel != ".." && !filepath.IsAbs(rel) && !strings.HasPrefix(rel, ".."+string(filepath.Separator))
}

func targetKey(path string) string {
	if core.IsVirtualPath(path) {
		return path
	}
	if abs, err := filepath.Abs(path); err == nil {
		path = abs
	}
	if runtime.GOOS == "windows" {
		return core.Norm(path)
	}
	return filepath.Clean(path)
}

func selectJunk(items []core.ScanItem, o junkOptions) ([]bool, error) {
	requested := map[string]bool{}
	for _, path := range o.paths {
		requested[targetKey(path)] = false
	}
	selected := make([]bool, len(items))
	for i, item := range items {
		if len(o.categories) > 0 && !slices.Contains(o.categories, string(item.Category)) {
			continue
		}
		key := targetKey(item.Path)
		_, explicit := requested[key]
		if explicit {
			requested[key] = true
		}
		pick := o.all || explicit || len(o.paths) == 0 && item.Recommended
		if !pick {
			continue
		}
		if !item.CleanupAvailable() {
			if explicit {
				return nil, fmt.Errorf("path is blocked from cleanup: %s", item.Path)
			}
			continue
		}
		selected[i] = true
	}
	for path, found := range requested {
		if !found {
			return nil, fmt.Errorf("path is not a scanned target in the selected categories: %s", path)
		}
	}
	return selected, nil
}

func printJunk(out io.Writer, r junkReport) error {
	if _, err := fmt.Fprintf(out, "%s: preview=%v partial=%v selected=%d (%s) skipped=%d\n", r.Command, r.DryRun, r.Partial, r.Selected, core.FmtSize(r.Bytes), r.Skipped); err != nil {
		return err
	}
	for _, item := range r.Items {
		mark := " "
		if item.Selected {
			mark = "*"
		}
		status := item.Blocked
		if status != "" {
			mark = "-"
		} else if item.Busy != nil {
			if label, _, ok := item.Busy.Badge(); ok {
				status = label.Get(core.English)
			}
		}
		if _, err := fmt.Fprintf(out, "%s %-16s %12s %s [%s; %s] %s\n", mark, item.Category, core.FmtSize(item.Bytes), item.Path, item.Operation.Kind, item.Disposal, status); err != nil {
			return err
		}
	}
	if r.Cleanup != nil {
		_, err := fmt.Fprintf(out, "Deleted %d files; freed %s; skipped=%d; unresolved=%d\n", r.Cleanup.DeletedFiles, core.FmtSize(r.Cleanup.FreedBytes), r.Cleanup.Skipped, len(r.Cleanup.Failed)+len(r.Cleanup.Manual))
		return err
	}
	return nil
}
