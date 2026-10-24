package core

import (
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
	"unicode/utf8"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type DevDiscoveryResult struct {
	CondaEnvs, PythonEnvs, NodePackages, PythonTools []DevAssetItem
	Total                                            AssetStorageSize
	RemovableExclusive                               uint64
}

func DiscoverDevAssets() DevDiscoveryResult { return DiscoverDevAssetsWith(RunToolWithTimeout) }

func DiscoverDevAssetsWith(run DevToolRunner) DevDiscoveryResult {
	out := DevDiscoveryResult{CondaEnvs: CondaEnvironments(run), PythonEnvs: PythonEnvironments(run), NodePackages: NodeGlobalPackages(run), PythonTools: PythonTools(run)}
	groups := []*[]DevAssetItem{&out.CondaEnvs, &out.PythonEnvs, &out.NodePackages, &out.PythonTools}
	seen := map[string]bool{}
	var dirs []string
	for _, group := range groups {
		kept := (*group)[:0]
		for _, item := range *group {
			key := Norm(item.Path)
			if seen[key] {
				continue
			}
			seen[key] = true
			kept = append(kept, item)
			dirs = append(dirs, item.Path)
		}
		*group = kept
	}
	total, sizes := MeasureBatchStorage(dirs)
	out.Total = total
	busy := SpotCheckInUse(dirs)
	i := 0
	for _, group := range groups {
		for j := range *group {
			item := &(*group)[j]
			if i < len(sizes) {
				item.Size = sizes[i]
			}
			i++
			if state, ok := busy[item.Path]; ok {
				item.InUse = state
			}
			if CanRemoveDevAsset(*item) {
				out.RemovableExclusive += item.Size.ExclusiveReclaimableBytes
			}
		}
	}
	return out
}

func CondaEnvironments(run DevToolRunner) []DevAssetItem {
	inv, e := InventoryConda(run, "conda")
	if e != "" {
		inv, e = InventoryConda(run, "mamba")
	}
	active := ""
	if p, ok := os.LookupEnv("CONDA_PREFIX"); ok {
		active = Norm(p)
	}
	var items []DevAssetItem
	seen := map[string]bool{}
	push := func(p string, base bool, source AssetSource) {
		key := Norm(p)
		if seen[key] {
			return
		}
		seen[key] = true
		items = append(items, condaItem(p, base, source, active))
	}
	if e == "" {
		for _, p := range inv.Envs {
			push(p, inv.RootPrefix != nil && Norm(*inv.RootPrefix) == Norm(p), AssetTool)
		}
	} else {
		for _, root := range commonCondaRoots() {
			if devIsDir(root) {
				push(root, true, AssetLayout)
				for _, p := range devSubdirs(filepath.Join(root, "envs"), false) {
					push(p, false, AssetLayout)
				}
			}
		}
		if home, ok := platform.UserHome(); ok {
			b, _ := os.ReadFile(filepath.Join(home, ".conda", "environments.txt"))
			if !utf8.Valid(b) {
				b = nil
			}
			for _, line := range strings.Split(string(b), "\n") {
				p := strings.TrimSpace(line)
				if p != "" && devIsDir(p) {
					push(p, false, AssetLayout)
				}
			}
		}
	}
	sort.SliceStable(items, func(i, j int) bool {
		if items[i].Kind.IsBase != items[j].Kind.IsBase {
			return items[i].Kind.IsBase
		}
		return items[i].DisplayName() < items[j].DisplayName()
	})
	return items
}

func commonCondaRoots() []string {
	var roots []string
	if home, ok := platform.UserHome(); ok {
		for _, name := range []string{"miniconda3", "anaconda3", "miniforge3", "mambaforge", "micromamba"} {
			roots = append(roots, filepath.Join(home, name))
		}
		if runtime.GOOS != "windows" {
			for _, name := range []string{"opt/miniconda3", "opt/anaconda3"} {
				roots = append(roots, filepath.Join(home, name))
			}
		}
	}
	if local, ok := platform.UserCacheDir(); ok {
		for _, name := range []string{"miniconda3", "anaconda3", "miniforge3"} {
			roots = append(roots, filepath.Join(local, name))
		}
	}
	if runtime.GOOS == "windows" {
		if p, ok := os.LookupEnv("ProgramData"); ok {
			roots = append(roots, filepath.Join(p, "miniconda3"), filepath.Join(p, "anaconda3"))
		}
	} else {
		roots = append(roots, "/opt/conda", "/opt/miniconda3", "/usr/local/miniconda3", "/usr/local/anaconda3")
	}
	return roots
}

func condaItem(path string, base bool, source AssetSource, active string) DevAssetItem {
	name := filepath.Base(path)
	if base {
		name = "base"
	}
	kind := DevAssetKind{Type: DevCondaEnv, Name: name, IsBase: base}
	var evidence *TimedEvidence
	meta := filepath.Join(path, "conda-meta")
	if devIsDir(meta) {
		count := 0
		entries, _ := os.ReadDir(meta)
		for _, entry := range entries {
			name := entry.Name()
			if strings.HasSuffix(name, ".json") {
				count++
				if kind.PythonVersion == nil && strings.HasPrefix(name, "python-") {
					v := strings.Split(name, "-")[1]
					kind.PythonVersion = &v
				}
			}
		}
		kind.PackageCount = &count
		evidence = devEvidence(filepath.Join(meta, "history"), TimeCondaTransaction)
	}
	return DevAssetItem{ID: "conda:" + Norm(path), Kind: kind, Path: path, LastChange: evidence, IsActiveEnv: active != "" && active == Norm(path), Source: source}
}

func NodeGlobalPackages(run DevToolRunner) []DevAssetItem {
	var items []DevAssetItem
	if packages, e := InventoryNpm(run); e == "" {
		for _, p := range packages {
			items = append(items, nodePackageItem("npm", p.Name, p.Version, p.Path, AssetTool))
		}
	} else {
		for _, root := range layoutNpmDirs() {
			items = append(items, ScanNodeModules("npm", root)...)
		}
	}
	if root, e := InventoryRoot(run, "pnpm"); e == "" {
		items = append(items, ScanNodeModules("pnpm", root)...)
	} else {
		for _, root := range layoutPnpmDirs() {
			items = append(items, ScanNodeModules("pnpm", root)...)
		}
	}
	if home, ok := platform.UserHome(); ok {
		items = append(items, ScanNodeModules("bun", filepath.Join(home, ".bun/install/global/node_modules"))...)
	}
	devSort(items)
	return items
}

// ScanNodeModules trusts declared top-level dependencies; directory fallback is display-only.
func ScanNodeModules(manager, root string) []DevAssetItem {
	var items []DevAssetItem
	push := func(path, name string, source AssetSource) {
		if !devIsDir(path) {
			return
		}
		v := readDevManifest(path)
		if s := devString(v["name"]); s != nil {
			name = *s
		}
		version := devString(v["version"])
		items = append(items, nodePackageItem(manager, name, version, path, source))
	}
	if names, ok := DeclaredTopLevel(root); ok {
		for _, name := range names {
			push(filepath.Join(root, name), name, AssetTool)
		}
		return items
	}
	for _, p := range devSubdirs(root, true) {
		name := filepath.Base(p)
		if strings.HasPrefix(name, "@") {
			for _, scoped := range devSubdirs(p, false) {
				push(scoped, filepath.Base(scoped), AssetLayout)
			}
		} else {
			push(p, name, AssetLayout)
		}
	}
	return items
}

func readDevManifest(path string) map[string]json.RawMessage {
	b, _ := os.ReadFile(filepath.Join(path, "package.json"))
	var v map[string]json.RawMessage
	if !utf8.Valid(b) || json.Unmarshal(b, &v) != nil {
		return nil
	}
	return v
}
func nodePackageItem(manager, name string, version *string, path string, source AssetSource) DevAssetItem {
	if version == nil {
		v := "unknown"
		version = &v
	}
	v := readDevManifest(path)
	var shims []string
	if single := devString(v["bin"]); single != nil {
		shims = []string{filepath.Base(*single)}
	} else if bins, ok := devObject(v["bin"]); ok {
		shims = devKeys(bins)
	}
	return DevAssetItem{ID: manager + ":" + name, Kind: DevAssetKind{Type: DevNodePackage, Manager: manager, Name: name, Version: version, BinShims: shims}, Path: path, LastChange: devEvidence(filepath.Join(path, "package.json"), TimePackageManifest), Source: source}
}

func layoutNpmDirs() []string {
	var roots []string
	if home, ok := platform.UserHome(); ok {
		roots = append(roots, filepath.Join(home, "node_modules"), filepath.Join(home, ".npm-global/lib/node_modules"))
	}
	if runtime.GOOS == "windows" {
		if roaming, ok := platform.UserDataDir(); ok {
			roots = append(roots, filepath.Join(roaming, "npm/node_modules"))
		}
	} else {
		roots = append(roots, "/usr/local/lib/node_modules")
	}
	return roots
}
func layoutPnpmDirs() []string {
	var parents, roots []string
	if home, ok := platform.UserHome(); ok {
		parents = append(parents, filepath.Join(home, ".local/share/pnpm/global"))
	}
	if local, ok := platform.UserCacheDir(); ok {
		parents = append(parents, filepath.Join(local, "pnpm/global"))
	}
	for _, parent := range parents {
		for _, p := range devSubdirs(parent, false) {
			p = filepath.Join(p, "node_modules")
			if devIsDir(p) {
				roots = append(roots, p)
			}
		}
	}
	return roots
}

func PythonTools(run DevToolRunner) []DevAssetItem {
	var items []DevAssetItem
	uvRoots := []string{}
	uvSource := AssetLayout
	if root, e := InventoryRoot(run, "uv"); e == "" {
		uvRoots = append(uvRoots, root)
		uvSource = AssetTool
	} else {
		if local, ok := platform.UserCacheDir(); ok {
			uvRoots = append(uvRoots, filepath.Join(local, "uv/tools"))
		}
		if home, ok := platform.UserHome(); ok {
			p := filepath.Join(home, ".local/share/uv/tools")
			uvRoots = append(uvRoots, p, p)
		}
	}
	for _, root := range uvRoots {
		for _, p := range devSubdirs(root, true) {
			items = append(items, pythonToolItem(PythonToolUv, filepath.Base(p), p, uvSource))
		}
	}
	var pipxRoots []string
	if home, ok := platform.UserHome(); ok {
		pipxRoots = append(pipxRoots, filepath.Join(home, ".local/pipx/venvs"))
	}
	if tools, e := InventoryPipx(run); e == "" {
		for _, tool := range tools {
			path := tool.Name
			if tool.Venv != nil {
				path = *tool.Venv
			} else if len(pipxRoots) > 0 {
				path = filepath.Join(pipxRoots[0], tool.Name)
			}
			items = append(items, pythonToolItem(PythonToolPipx, tool.Name, path, AssetTool))
		}
	} else {
		for _, root := range pipxRoots {
			for _, p := range devSubdirs(root, true) {
				items = append(items, pythonToolItem(PythonToolPipx, filepath.Base(p), p, AssetLayout))
			}
		}
	}
	devSort(items)
	return items
}

func pythonToolItem(kind PythonToolKind, name, path string, source AssetSource) DevAssetItem {
	var evidence *TimedEvidence
	for _, name := range []string{"pipx_metadata.json", "uv-receipt.toml"} {
		evidence = devEvidence(filepath.Join(path, name), TimeToolMetadata)
		if evidence != nil {
			break
		}
	}
	if evidence == nil {
		evidence = devEvidence(path, TimeDirectoryEntry)
	}
	return DevAssetItem{ID: string(kind) + ":" + name, Kind: DevAssetKind{Type: DevPythonTool, ToolKind: kind, Name: name, Executables: []string{name}}, Path: path, LastChange: evidence, Source: source}
}
