package core

import (
	"encoding/json"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"
	"unicode/utf8"
)

type DevToolRunner func(string, []string, time.Duration) (ProcRun, bool)
type InventoryError string

const (
	InventoryUnavailable InventoryError = "unavailable"
	InventoryUnreadable  InventoryError = "unreadable"
	DevInventoryTimeout                 = 20 * time.Second
)

type CondaInventory struct {
	Envs       []string
	RootPrefix *string
}
type NpmPackage struct {
	Name    string
	Version *string
	Path    string
}
type PipxTool struct {
	Name string
	Venv *string
}

func inventoryText(run DevToolRunner, program string, args []string, allowFailure bool) (string, InventoryError) {
	r, ok := run(program, args, DevInventoryTimeout)
	if !ok || (!allowFailure && !r.OK) || strings.TrimSpace(string(r.Stdout)) == "" {
		return "", InventoryUnavailable
	}
	return string(r.Stdout), ""
}

func inventoryJSON(run DevToolRunner, program string, args []string, allowFailure bool) (map[string]json.RawMessage, InventoryError) {
	t, e := inventoryText(run, program, args, allowFailure)
	if e != "" {
		return nil, e
	}
	var v map[string]json.RawMessage
	if json.Unmarshal([]byte(t), &v) != nil || v == nil {
		return nil, InventoryUnreadable
	}
	return v, ""
}

func devString(raw json.RawMessage) *string {
	var s string
	if json.Unmarshal(raw, &s) != nil || string(raw) == "null" {
		return nil
	}
	return &s
}
func devObject(raw json.RawMessage) (map[string]json.RawMessage, bool) {
	var v map[string]json.RawMessage
	e := json.Unmarshal(raw, &v)
	return v, e == nil && v != nil
}
func devKeys(v map[string]json.RawMessage) []string {
	names := make([]string, 0, len(v))
	for k := range v {
		names = append(names, k)
	}
	sort.Strings(names)
	return names
}

func InventoryConda(run DevToolRunner, program string) (CondaInventory, InventoryError) {
	v, e := inventoryJSON(run, program, []string{"info", "--json"}, false)
	if e != "" {
		return CondaInventory{}, e
	}
	var envs []json.RawMessage
	if json.Unmarshal(v["envs"], &envs) != nil || envs == nil {
		return CondaInventory{}, InventoryUnreadable
	}
	out := CondaInventory{Envs: []string{}, RootPrefix: devString(v["root_prefix"])}
	for _, raw := range envs {
		if s := devString(raw); s != nil {
			out.Envs = append(out.Envs, *s)
		}
	}
	return out, ""
}

func inventoryPath(run DevToolRunner, program string, args []string) (string, InventoryError) {
	t, e := inventoryText(run, program, args, false)
	if e != "" {
		return "", e
	}
	t = strings.TrimSpace(t)
	if !filepath.IsAbs(t) {
		return "", InventoryUnreadable
	}
	return t, ""
}

func InventoryRoot(run DevToolRunner, program string) (string, InventoryError) {
	switch program {
	case "pnpm":
		return inventoryPath(run, program, []string{"root", "--global"})
	case "uv":
		return inventoryPath(run, program, []string{"tool", "dir"})
	default:
		return "", InventoryUnavailable
	}
}

func InventoryNpm(run DevToolRunner) ([]NpmPackage, InventoryError) {
	prefix, e := inventoryPath(run, "npm", []string{"prefix", "--global"})
	if e != "" {
		return nil, e
	}
	v, e := inventoryJSON(run, "npm", []string{"ls", "--global", "--depth=0", "--json"}, true)
	if e != "" {
		return nil, e
	}
	deps, ok := devObject(v["dependencies"])
	if !ok {
		return nil, InventoryUnreadable
	}
	out := make([]NpmPackage, 0, len(deps))
	for _, name := range devKeys(deps) {
		p, _ := devObject(deps[name])
		out = append(out, NpmPackage{Name: name, Version: devString(p["version"]), Path: filepath.Join(prefix, "node_modules", name)})
	}
	return out, ""
}

func InventoryPipx(run DevToolRunner) ([]PipxTool, InventoryError) {
	v, e := inventoryJSON(run, "pipx", []string{"list", "--json"}, false)
	if e != "" {
		return nil, e
	}
	venvs, ok := devObject(v["venvs"])
	if !ok {
		return nil, InventoryUnreadable
	}
	out := make([]PipxTool, 0, len(venvs))
	for _, name := range devKeys(venvs) {
		v, _ := devObject(venvs[name])
		m, _ := devObject(v["metadata"])
		out = append(out, PipxTool{Name: name, Venv: devString(m["environment"])})
	}
	return out, ""
}

// A valid empty dependency object is authoritative, unlike a missing or broken manifest.
func DeclaredTopLevel(nodeModules string) ([]string, bool) {
	b, e := os.ReadFile(filepath.Join(filepath.Dir(nodeModules), "package.json"))
	if e != nil || !utf8.Valid(b) {
		return nil, false
	}
	var v map[string]json.RawMessage
	if json.Unmarshal(b, &v) != nil {
		return nil, false
	}
	deps, ok := devObject(v["dependencies"])
	if !ok {
		return nil, false
	}
	return devKeys(deps), true
}
