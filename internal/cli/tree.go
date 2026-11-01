package cli

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type fileRow struct {
	Path      string `json:"path"`
	Name      string `json:"name"`
	Directory bool   `json:"directory"`
	Bytes     uint64 `json:"bytes"`
	Files     uint64 `json:"files,omitempty"`
	Mtime     uint64 `json:"mtime,omitempty"`
}

type treeReport struct {
	Schema      int       `json:"schema_version"`
	Command     string    `json:"command"`
	Source      string    `json:"source"`
	Root        string    `json:"root"`
	Partial     bool      `json:"partial"`
	Cancelled   bool      `json:"cancelled"`
	Truncated   bool      `json:"truncated"`
	Blocked     []string  `json:"blocked_paths"`
	Bytes       uint64    `json:"bytes"`
	Files       uint64    `json:"files"`
	Directories uint64    `json:"directories"`
	ElapsedMS   int64     `json:"elapsed_ms"`
	Rows        []fileRow `json:"rows"`
}

func cleanRoot(path string) (string, error) {
	if path == "" {
		return "", errors.New("a root is required")
	}
	abs, err := filepath.Abs(path)
	if err != nil {
		return "", err
	}
	md, err := os.Lstat(abs)
	if err != nil {
		return "", err
	}
	if !md.IsDir() || md.Mode()&os.ModeSymlink != 0 {
		return "", errors.New("root must be an existing directory, not a link")
	}
	return abs, nil
}

// The live acquisition adapter records coverage gaps instead of treating an
// unreadable subtree as empty. Aggregation and name matching stay in SizeTree.
func scanDirectory(ctx context.Context, root string, maxEntries int) (*core.SizeTree, error) {
	entries := []core.TreeIndexEntry{{Name: filepath.Base(root), IsDir: true, Used: true}}
	parents := map[string]uint32{root: 0}
	var blocked []string
	err := filepath.WalkDir(root, func(path string, d fs.DirEntry, walkErr error) error {
		if ctx.Err() != nil {
			blocked = append(blocked, root)
			return ctx.Err()
		}
		if walkErr != nil {
			blocked = append(blocked, path)
			return nil
		}
		if path == root {
			return nil
		}
		if len(entries) >= maxEntries {
			blocked = append(blocked, root)
			return fs.SkipAll
		}
		if d.Type()&os.ModeSymlink != 0 {
			return nil
		}
		md, err := d.Info()
		if err != nil {
			blocked = append(blocked, path)
			if d.IsDir() {
				return fs.SkipDir
			}
			return nil
		}
		if !md.IsDir() && !md.Mode().IsRegular() {
			return nil
		}
		parent, ok := parents[filepath.Dir(path)]
		if !ok {
			blocked = append(blocked, path)
			return nil
		}
		entry := core.TreeIndexEntry{Parent: parent, Name: d.Name(), IsDir: md.IsDir(), Used: true}
		if !entry.IsDir {
			entry.Size = platform.AllocatedFileSize(md)
		} else {
			parents[path] = uint32(len(entries))
		}
		if md.ModTime().Unix() > 0 {
			entry.Mtime = uint64(md.ModTime().Unix())
		}
		entries = append(entries, entry)
		return nil
	})
	tree := core.NewSizeTree(core.VolumeFromMountPoint(root), entries)
	tree.SetBlockedSubtrees(blocked)
	return tree, err
}

func runTree(parent context.Context, search bool, args []string, out, errOut io.Writer) (int, error) {
	name := "disk analyze"
	if search {
		name = "search"
	}
	f := flags(name, errOut)
	var common commonOptions
	common.bind(f)
	root := f.String("root", "", "directory to scan live (read-only)")
	volume := f.String("volume", "", "mount point for a fresh native volume scan (Windows/macOS)")
	limit := f.Int("limit", 100, "maximum search results")
	top := f.Int("top", 20, "maximum immediate children in disk analysis")
	query := f.String("query", "", "product filename pattern: substring, * or ? (ASCII case-insensitive)")
	maxEntries := f.Int("max-entries", 2000000, "live tree entry budget; reaching it reports partial coverage")
	if code, err := parse(f, args); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return ExitOK, nil
		}
		return code, err
	}
	if (*root == "") == (*volume == "") || common.timeout <= 0 || *limit < 1 || *limit > 1000000 || *top < 1 || *maxEntries < 2 || search && *query == "" {
		return ExitUsage, errors.New("specify exactly one of --root/--volume and valid positive limits; search requires --query")
	}
	path := *root
	if path == "" {
		path = *volume
	}
	path, err := cleanRoot(path)
	if err != nil {
		return ExitUsage, err
	}
	ctx, cancel := context.WithTimeout(parent, common.timeout)
	defer cancel()
	started := time.Now()
	var tree *core.SizeTree
	source := "live-directory"
	if *volume != "" {
		source = "native-volume"
		found := false
		for _, v := range platform.ListVolumes() {
			if core.Norm(v.MountPoint()) == core.Norm(path) {
				found = true
				break
			}
		}
		if !found {
			return ExitUsage, errors.New("--volume must name a mounted volume root; use --root for a directory")
		}
		tree, err = scanNative(ctx, core.VolumeFromMountPoint(path))
	} else {
		tree, err = scanDirectory(ctx, path, *maxEntries)
	}
	if err != nil && contextCode(ctx) == ExitOK {
		return ExitError, err
	}
	if tree == nil {
		tree = core.NewSizeTree(core.VolumeFromMountPoint(path), nil)
		tree.SetBlockedSubtrees([]string{path})
	}
	r := treeReport{Schema: 1, Command: name, Source: source, Root: path, Blocked: tree.BlockedSubtrees(), Rows: []fileRow{}, Bytes: tree.SizeOf(tree.Root()), Files: tree.CountUsedFiles(), Directories: tree.CountUsedDirs()}
	r.Partial = len(r.Blocked) > 0
	if search {
		hits := tree.SearchCancellable(ctx, *query, *limit+1)
		r.Truncated = len(hits) > *limit
		for _, hit := range hits[:min(len(hits), *limit)] {
			r.Rows = append(r.Rows, fileRow{Path: hit.Path, Name: hit.Name, Directory: hit.IsDir, Bytes: hit.Size, Mtime: hit.Mtime})
		}
	} else {
		nodes := tree.Children(tree.Root())
		sort.SliceStable(nodes, func(i, j int) bool { return nodes[i].Size > nodes[j].Size })
		r.Truncated = len(nodes) > *top
		for _, node := range nodes[:min(len(nodes), *top)] {
			r.Rows = append(r.Rows, fileRow{Path: tree.PathOf(node.Idx), Name: node.Name, Directory: node.IsDir, Bytes: node.Size, Files: node.FileCount})
		}
	}
	r.ElapsedMS = time.Since(started).Milliseconds()
	code := contextCode(ctx)
	r.Cancelled = code != ExitOK
	r.Partial = r.Partial || r.Cancelled
	if common.json {
		err = writeJSON(out, r)
	} else {
		_, err = fmt.Fprintf(out, "%s: %s; %d files; %s; partial=%v\n", name, path, r.Files, core.FmtSize(r.Bytes), r.Partial)
		for _, row := range r.Rows {
			if err == nil {
				_, err = fmt.Fprintf(out, "%12s  %s\n", core.FmtSize(row.Bytes), row.Path)
			}
		}
	}
	if err != nil {
		return ExitError, err
	}
	if code != ExitOK {
		return code, ctx.Err()
	}
	if r.Partial {
		return ExitIncomplete, nil
	}
	return ExitOK, nil
}
