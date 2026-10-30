// Command indexbench measures repeated product index loads without starting the UI.
package main

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"log"
	"os"
	"os/signal"
	"path/filepath"
	"runtime"
	"runtime/pprof"
	"sort"
	"strings"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type indexOwner interface {
	Load(context.Context, core.VolumeID) (*core.ScanResult, error)
	Close()
	Flush(context.Context) error
}

type measurement struct {
	Milliseconds float64 `json:"ms"`
	AllocatedMiB float64 `json:"allocated_mib"`
}

type result struct {
	Round       int         `json:"round"`
	Mode        string      `json:"mode"`
	Load        measurement `json:"load"`
	Discover    measurement `json:"discover"`
	PersistWait measurement `json:"persist_wait"`
	Verify      measurement `json:"verify"`
	Files       uint64      `json:"files"`
	Dirs        uint64      `json:"dirs"`
	Items       int         `json:"items"`
	Fingerprint string      `json:"fingerprint,omitempty"`
}

func main() {
	log.SetFlags(log.Ltime | log.Lmicroseconds)
	if err := run(os.Args[1:], os.Stdout); err != nil && !errors.Is(err, flag.ErrHelp) {
		fmt.Fprintln(os.Stderr, "indexbench:", err)
		os.Exit(1)
	}
}

func run(args []string, out io.Writer) error {
	flags := flag.NewFlagSet("indexbench", flag.ContinueOnError)
	mode := flags.String("mode", "cache", "cache: load saved index; refresh: product load, event replay and persistence (macOS)")
	root := flags.String("root", "/", "absolute volume or scan root")
	sourceDir := flags.String("cache-dir", "", "source cache directory (default: product cache)")
	rounds := flags.Int("rounds", 2, "rounds in the same process")
	timeout := flags.Duration("timeout", 5*time.Minute, "timeout per round")
	productDiscovery := flags.Bool("product-discovery", false, "include the GUI discovery plan and uncovered live fallback")
	discover := flags.Bool("discover", true, "include build-artifact discovery")
	fresh := flags.Bool("fresh", false, "start without a cache snapshot (refresh mode only)")
	jsonOutput := flags.Bool("json", false, "write one JSON result per round")
	profile := flags.String("cpu-profile", "", "optional CPU profile output file")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if flags.NArg() != 0 || (*mode != "cache" && *mode != "refresh") || *rounds < 1 || *timeout <= 0 || (*fresh && *mode != "refresh") {
		return errors.New("invalid options; use -h for usage")
	}
	if !filepath.IsAbs(*root) || filepath.Clean(*root) != *root {
		return errors.New("root must be a clean absolute path")
	}
	if *sourceDir == "" {
		var err error
		*sourceDir, err = core.DiskIndexCacheRoot()
		if err != nil {
			return err
		}
	}
	volume := core.VolumeFromMountPoint(*root)
	source, err := core.NewDiskIndexCache(*sourceDir)
	if err != nil {
		return err
	}
	scratch, err := os.MkdirTemp("", "qc-indexbench-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(scratch)
	scratch, err = filepath.EvalSymlinks(scratch)
	if err != nil {
		return err
	}
	if !*fresh {
		if err := copyCacheGroup(source, scratch, volume); err != nil {
			return fmt.Errorf("cache snapshot: %w (use -mode refresh -fresh for a full scan)", err)
		}
	}
	cache, err := core.NewDiskIndexCache(scratch)
	if err != nil {
		return err
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	defer stop()
	var owner indexOwner
	if *mode == "refresh" {
		owner, err = newNativeOwner(ctx, cache)
		if err != nil {
			return err
		}
		defer func() { owner.Close(); _ = owner.Flush(context.Background()) }()
	}
	if *profile != "" {
		file, err := os.OpenFile(*profile, os.O_CREATE|os.O_WRONLY, 0600)
		if err != nil {
			return err
		}
		if err = file.Chmod(0600); err != nil {
			file.Close()
			return err
		}
		if err = file.Truncate(0); err != nil {
			file.Close()
			return err
		}
		if err = pprof.StartCPUProfile(file); err != nil {
			file.Close()
			return err
		}
		defer func() { pprof.StopCPUProfile(); file.Close() }()
	}
	rules, err := core.CurrentRules()
	if err != nil {
		return err
	}
	if !*jsonOutput {
		fmt.Fprintln(out, "round  load         discover     verify       persist wait load MiB    discover MiB  items")
	}
	for round := 1; round <= *rounds; round++ {
		roundCtx, cancel := context.WithTimeout(ctx, *timeout)
		entry, err := runRound(roundCtx, *mode, round, volume, cache, owner, rules, *discover, *productDiscovery)
		cancel()
		if err != nil {
			return fmt.Errorf("round %d: %w", round, err)
		}
		if *jsonOutput {
			if err = json.NewEncoder(out).Encode(entry); err != nil {
				return err
			}
		} else {
			fmt.Fprintf(out, "%-6d %-12s %-12s %-12s %-12s %-11.1f %-13.1f %d\n", round, time.Duration(entry.Load.Milliseconds*float64(time.Millisecond)).Round(time.Millisecond), time.Duration(entry.Discover.Milliseconds*float64(time.Millisecond)).Round(time.Millisecond), time.Duration(entry.Verify.Milliseconds*float64(time.Millisecond)).Round(time.Millisecond), time.Duration(entry.PersistWait.Milliseconds*float64(time.Millisecond)).Round(time.Millisecond), entry.Load.AllocatedMiB, entry.Discover.AllocatedMiB, entry.Items)
			if entry.Fingerprint != "" {
				fmt.Fprintln(out, "       fingerprint", entry.Fingerprint)
			}
		}
	}
	return nil
}

func measure(ctx context.Context, phase string, fn func() error) (measurement, error) {
	var before, after runtime.MemStats
	runtime.ReadMemStats(&before)
	started := time.Now()
	var err error
	pprof.Do(ctx, pprof.Labels("phase", phase), func(context.Context) { err = fn() })
	elapsed := time.Since(started)
	runtime.ReadMemStats(&after)
	return measurement{float64(elapsed) / float64(time.Millisecond), float64(after.TotalAlloc-before.TotalAlloc) / (1 << 20)}, err
}

func runRound(ctx context.Context, mode string, round int, volume core.VolumeID, cache *core.DiskIndexCache, owner indexOwner, rules *core.RuleSnapshot, discover, productDiscovery bool) (entry result, err error) {
	entry = result{Round: round, Mode: mode}
	var scan *core.ScanResult
	if owner != nil {
		defer func() {
			var flushErr error
			entry.PersistWait, flushErr = measure(ctx, "persist-wait", func() error { return owner.Flush(ctx) })
			err = errors.Join(err, flushErr)
		}()
	}
	entry.Load, err = measure(ctx, "load", func() error {
		if owner != nil {
			scan, err = owner.Load(ctx, volume)
			return err
		}
		loaded, e := cache.Load(volume)
		if e != nil {
			return e
		}
		scan = &loaded.Scan
		return nil
	})
	if err != nil {
		return entry, err
	}
	if err = ctx.Err(); err != nil {
		return entry, err
	}
	if scan == nil || scan.Tree == nil {
		return entry, errors.New("index returned no tree")
	}
	entry.Files, entry.Dirs = scan.FileCount, scan.DirCount
	if !discover {
		return entry, nil
	}
	var items []core.ScanItem
	entry.Discover, err = measure(ctx, "discover", func() error {
		if productDiscovery {
			home := volume.Mount
			volumes := []core.VolumeID{volume}
			if volume.Mount == "/" {
				home, _ = platform.UserHome()
				volumes = platform.ListVolumes()
			}
			plan := core.BeginJunkIndex(ctx, false, home, volumes, []*core.SizeTree{scan.Tree}, nil, nil)
			var partial bool
			items, partial = plan.DiscoverWithin(ctx, rules, core.DiscoveryBudget)
			if partial {
				return errors.New("product discovery exhausted its budget")
			}
		} else {
			items = core.DiscoverBuildArtifacts(ctx, rules, nil, scan.Tree)
		}
		return ctx.Err()
	})
	if err != nil {
		return entry, err
	}
	entry.Verify, err = measure(ctx, "verify", func() error { items = core.DropVanishedDiscovered(items); return ctx.Err() })
	if err != nil {
		return entry, err
	}
	entry.Items = len(items)
	sort.Slice(items, func(i, j int) bool { return items[i].Path < items[j].Path })
	digest := sha256.New()
	for _, item := range items {
		fmt.Fprintf(digest, "%q|%d|%d|%q|%q|%v|%v|%v|%v\n", item.Path, item.Size, item.FileCount, item.Label.Get(core.Chinese), item.Label.Get(core.English), item.Operation, item.Disposal, item.Recommended, item.Rule.Observation())
	}
	entry.Fingerprint = fmt.Sprintf("%x", digest.Sum(nil))
	return entry, nil
}

// Snapshot only the selected volume's regular cache files; no product file is
// opened for writing, and a different volume or unrelated user file is ignored.
func copyCacheGroup(source *core.DiskIndexCache, destination string, volume core.VolumeID) error {
	base, delta := source.Paths(volume)
	names := []string{filepath.Base(base), filepath.Base(delta)}
	entries, err := os.ReadDir(source.Root())
	if err != nil {
		return err
	}
	foundBase := false
	for _, entry := range entries {
		name := entry.Name()
		match := name == names[0] || name == names[1]
		for _, index := range names {
			match = match || (strings.HasPrefix(name, index+".coverage-") && strings.HasSuffix(name, ".json"))
		}
		if !match {
			continue
		}
		path := filepath.Join(source.Root(), name)
		info, err := os.Lstat(path)
		if err != nil {
			return err
		}
		if !info.Mode().IsRegular() {
			return fmt.Errorf("cache entry is not a regular file: %s", name)
		}
		if err = copyRegular(path, filepath.Join(destination, name), info); err != nil {
			return err
		}
		foundBase = foundBase || name == names[0]
	}
	if !foundBase {
		return os.ErrNotExist
	}
	return nil
}

func copyRegular(source, destination string, expected os.FileInfo) error {
	in, err := os.Open(source)
	if err != nil {
		return err
	}
	defer in.Close()
	opened, err := in.Stat()
	if err != nil {
		return err
	}
	if !os.SameFile(expected, opened) {
		return errors.New("cache changed during snapshot")
	}
	out, err := os.OpenFile(destination, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return err
	}
	_, copyErr := io.Copy(out, in)
	closeErr := out.Close()
	return errors.Join(copyErr, closeErr)
}
