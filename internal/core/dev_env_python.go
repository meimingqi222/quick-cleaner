package core

import (
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
	"unicode/utf8"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type InterpreterFacts struct{ Version, Prefix, InstallSite, UserSite *string }

func ProbePython(run DevToolRunner, program string) InterpreterFacts {
	const script = `import json,sys,site,sysconfig;print(json.dumps({"version":sys.version.split()[0],"prefix":sys.prefix,"install":sysconfig.get_paths().get("purelib"),"user":site.getusersitepackages()}))`
	v, e := inventoryJSON(run, program, []string{"-c", script}, false)
	if e != "" {
		return InterpreterFacts{}
	}
	site := func(key string) *string {
		p := devString(v[key])
		if p != nil && *p == "" {
			return nil
		}
		return p
	}
	return InterpreterFacts{devString(v["version"]), devString(v["prefix"]), site("install"), site("user")}
}

func PythonInterpreters(run DevToolRunner) []DevAssetItem {
	var items []DevAssetItem
	seen, sites := map[string]bool{}, map[string]bool{}
	for _, name := range []string{"python", "python3"} {
		program, ok := platform.ResolveToolProgram(name)
		if !ok {
			continue
		}
		facts := ProbePython(run, name)
		key := program
		if facts.Prefix != nil {
			key = *facts.Prefix
		}
		key = Norm(key)
		if seen[key] {
			continue
		}
		seen[key] = true
		items = append(items, pythonInterpreterRows(name, program, facts, sites)...)
	}
	return items
}

func pythonInterpreterRows(name, program string, facts InterpreterFacts, seen map[string]bool) []DevAssetItem {
	display := name
	if facts.Version != nil {
		display = "Python " + *facts.Version
	}
	rows := []struct {
		scope SiteScope
		path  *string
	}{{SiteInstall, facts.InstallSite}}
	if facts.UserSite != nil {
		rows = append(rows, struct {
			scope SiteScope
			path  *string
		}{SiteUser, facts.UserSite})
	} else {
		rows[0].path = nil
	}
	var items []DevAssetItem
	for _, row := range rows {
		k := DevAssetKind{Type: DevPythonInterpreter, Name: display, Version: facts.Version, Scope: row.scope, Python: program}
		item := DevAssetItem{ID: "python:" + Norm(program), Kind: k, Path: program, Source: AssetLayout}
		if row.path != nil {
			p := *row.path
			if !devIsDir(p) || seen[Norm(p)] {
				continue
			}
			seen[Norm(p)] = true
			count := countDistributions(p)
			item.ID = "python:" + Norm(p)
			item.Path = p
			item.Source = AssetTool
			item.Kind.SitePackages = row.path
			item.Kind.PackageCount = &count
			item.LastChange = devEvidence(p, TimeSitePackages)
		}
		items = append(items, item)
	}
	return items
}

type VenvRoot struct {
	Path    string
	Manager VenvManager
}

func VenvRoots(workonHome *string) []VenvRoot {
	var roots []VenvRoot
	home, ok := platform.UserHome()
	if workonHome != nil {
		roots = append(roots, VenvRoot{*workonHome, VenvVirtualenvwrapper})
	} else if ok {
		roots = append(roots, VenvRoot{filepath.Join(home, ".virtualenvs"), VenvVirtualenvwrapper})
	}
	if ok {
		for _, p := range []string{".cache/pypoetry/virtualenvs", "Library/Caches/pypoetry/virtualenvs"} {
			roots = append(roots, VenvRoot{filepath.Join(home, p), VenvPoetry})
		}
	}
	if local, ok := platform.UserCacheDir(); ok {
		roots = append(roots, VenvRoot{filepath.Join(local, "pypoetry", "Cache", "virtualenvs"), VenvPoetry})
	}
	return roots
}

func VirtualEnvs(roots []VenvRoot, poetryPaths []string) []DevAssetItem {
	var items []DevAssetItem
	seen := map[string]bool{}
	push := func(path string, manager VenvManager) {
		if seen[Norm(path)] {
			return
		}
		id := CaptureVenvIdentity(path)
		if id == nil {
			return
		}
		cfg, e := os.ReadFile(filepath.Join(path, "pyvenv.cfg"))
		if e != nil || !utf8.Valid(cfg) {
			return
		}
		kind := DevAssetKind{Type: DevVirtualEnv, Name: filepath.Base(path), Version: venvCfgValue(string(cfg), "version"), BasePython: venvCfgValue(string(cfg), "home"), VenvManager: manager, Identity: id}
		item := DevAssetItem{ID: "venv:" + Norm(path), Kind: kind, Path: path, Source: AssetTool}
		if site, ok := VenvSitePackages(path); ok {
			count := countDistributions(site)
			item.Kind.PackageCount = &count
			item.LastChange = devEvidence(site, TimeSitePackages)
		}
		seen[Norm(path)] = true
		items = append(items, item)
	}
	for _, root := range roots {
		for _, p := range devSubdirs(root.Path, false) {
			push(p, root.Manager)
		}
	}
	for _, p := range poetryPaths {
		push(p, VenvPoetry)
	}
	devSort(items)
	return items
}

func VenvSitePackages(dir string) (string, bool) {
	windows := filepath.Join(dir, "Lib", "site-packages")
	if devIsDir(windows) {
		return windows, true
	}
	found := ""
	for _, p := range devSubdirs(filepath.Join(dir, "lib"), false) {
		candidate := filepath.Join(p, "site-packages")
		if devIsDir(candidate) {
			found = candidate
		}
	}
	return found, found != ""
}

func countDistributions(site string) int {
	n := 0
	for _, p := range devSubdirs(site, false) {
		if strings.HasSuffix(p, ".dist-info") {
			n++
		}
	}
	return n
}
func venvCfgValue(cfg, key string) *string {
	for _, line := range strings.Split(cfg, "\n") {
		k, v, ok := strings.Cut(line, "=")
		v = strings.TrimSpace(v)
		if ok && asciiLower(strings.TrimSpace(k)) == asciiLower(key) {
			if v == "" {
				return nil
			}
			return &v
		}
	}
	return nil
}
func VenvPython(dir string) (string, bool) {
	p := filepath.Join(dir, "bin", "python")
	if runtime.GOOS == "windows" {
		p = filepath.Join(dir, "Scripts", "python.exe")
	}
	md, e := os.Stat(p)
	return p, e == nil && md.Mode().IsRegular()
}

func PipPackages(sitePackages, python string) []DevAssetItem {
	var items []DevAssetItem
	for _, p := range devSubdirs(sitePackages, false) {
		if !strings.HasSuffix(p, ".dist-info") {
			continue
		}
		b, e := os.ReadFile(filepath.Join(p, "METADATA"))
		if e != nil || !utf8.Valid(b) {
			continue
		}
		name, ok := MetadataField(string(b), "Name")
		if !ok || !IsValidDistributionName(name) {
			continue
		}
		var version *string
		if v, ok := MetadataField(string(b), "Version"); ok {
			version = &v
		}
		items = append(items, DevAssetItem{ID: "pip:" + Norm(p), Kind: DevAssetKind{Type: DevPipPackage, Python: python, Name: name, Version: version}, Path: p, LastChange: devEvidence(p, TimePackageManifest), Source: AssetTool})
	}
	devSort(items)
	return items
}

func MetadataField(text, key string) (string, bool) {
	for _, line := range strings.Split(text, "\n") {
		if strings.TrimSpace(line) == "" {
			break
		}
		name, value, ok := strings.Cut(line, ":")
		if ok && asciiLower(strings.TrimSpace(name)) == asciiLower(key) {
			value = strings.TrimSpace(value)
			return value, value != ""
		}
	}
	return "", false
}
func IsValidDistributionName(name string) bool {
	if name == "" {
		return false
	}
	for _, c := range name {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '.' || c == '-' || c == '_') {
			return false
		}
	}
	return true
}

func PoetryEnvPaths(run DevToolRunner) []string {
	if _, ok := platform.ResolveToolProgram("poetry"); !ok {
		return nil
	}
	text, e := inventoryText(run, "poetry", []string{"env", "list", "--full-path"}, false)
	if e != "" {
		return nil
	}
	var paths []string
	for _, line := range strings.Split(text, "\n") {
		p := strings.TrimSpace(line)
		if filepath.IsAbs(p) {
			paths = append(paths, p)
		}
	}
	return paths
}

func PythonEnvironments(run DevToolRunner) []DevAssetItem {
	items := PythonInterpreters(run)
	var workon *string
	if s, ok := os.LookupEnv("WORKON_HOME"); ok {
		workon = &s
	}
	items = append(items, VirtualEnvs(VenvRoots(workon), PoetryEnvPaths(run))...)
	devSort(items)
	return items
}

func devIsDir(path string) bool { md, e := os.Stat(path); return e == nil && md.IsDir() }
func devSubdirs(root string, skipHidden bool) []string {
	var paths []string
	entries, _ := os.ReadDir(root)
	for _, entry := range entries {
		if skipHidden && strings.HasPrefix(entry.Name(), ".") {
			continue
		}
		p := filepath.Join(root, entry.Name())
		if devIsDir(p) {
			paths = append(paths, p)
		}
	}
	return paths
}
func devEvidence(path string, source TimestampSource) *TimedEvidence {
	md, e := os.Stat(path)
	if e != nil {
		return nil
	}
	return &TimedEvidence{At: md.ModTime(), Source: source}
}
func devSort(items []DevAssetItem) {
	sort.SliceStable(items, func(i, j int) bool { return items[i].DisplayName() < items[j].DisplayName() })
}
