package core

import (
	"os"
	"path/filepath"
	"strings"
	"time"
	"unicode/utf8"
)

type RemovalRefusal string

const (
	RemovalProtected            RemovalRefusal = "protected"
	RemovalProtectedEnvironment RemovalRefusal = "protected_environment"
	RemovalBusy                 RemovalRefusal = "busy"
	RemovalUnsupportedTool      RemovalRefusal = "unsupported_tool"
	RemovalUnverifiedSource     RemovalRefusal = "unverified_source"
	RemovalToolUnavailable      RemovalRefusal = "tool_unavailable"
	RemovalToolUnreadable       RemovalRefusal = "tool_unreadable"
	RemovalNotRegistered        RemovalRefusal = "not_registered"
)

type RemovalStatus string

const (
	AssetRemoved RemovalStatus = "removed"
	AssetRefused RemovalStatus = "refused"
	AssetFailed  RemovalStatus = "failed"
)

// Refused means no mutation was attempted; Failed may have partially changed it.
type RemovalOutcome struct {
	Status       RemovalStatus
	Refusal      RemovalRefusal
	Code, Detail string
}

func devRemovalChannel(i DevAssetItem) string {
	switch i.Kind.Type {
	case DevCondaEnv:
		return "conda"
	case DevNodePackage:
		if i.Kind.Manager == "npm" || i.Kind.Manager == "pnpm" {
			return i.Kind.Manager
		}
	case DevPythonTool:
		if i.Kind.ToolKind == PythonToolUv || i.Kind.ToolKind == PythonToolPipx {
			return string(i.Kind.ToolKind)
		}
	case DevPipPackage:
		return "pip"
	case DevVirtualEnv:
		return "venv"
	}
	return ""
}

// CanRemoveDevAsset controls visibility only; execution independently rechecks.
func CanRemoveDevAsset(i DevAssetItem) bool {
	channel := devRemovalChannel(i)
	if channel == "" || i.IsProtected() {
		return false
	}
	if channel == "venv" {
		return i.Kind.Identity != nil
	}
	return i.Source == AssetTool
}

type preparedDevRemoval struct {
	target, program string
	args            []string
	tree            bool
}

func RemoveDevAsset(i DevAssetItem) RemovalOutcome {
	return removeDevAssetWith(i, RunToolWithTimeout, func(path string) bool {
		return DeleteTree(path, NewCleanProgress(0, 0)) == CleanOK
	})
}

func removeDevAssetWith(i DevAssetItem, run DevToolRunner, removeTree func(string) bool) RemovalOutcome {
	refuse := func(r RemovalRefusal) RemovalOutcome { return RemovalOutcome{Status: AssetRefused, Refusal: r} }
	if IsProtected(i.Path) {
		return refuse(RemovalProtected)
	}
	if i.IsProtected() {
		return refuse(RemovalProtectedEnvironment)
	}
	channel := devRemovalChannel(i)
	if channel == "" {
		return refuse(RemovalUnsupportedTool)
	}
	if channel != "venv" && i.Source != AssetTool {
		return refuse(RemovalUnverifiedSource)
	}
	if i.InUse != SpotClear {
		return refuse(RemovalBusy)
	}
	prepared, refusal := prepareDevRemoval(i, channel, run)
	if refusal != "" {
		return refuse(refusal)
	}
	fail := func(code, detail string) RemovalOutcome {
		return RemovalOutcome{Status: AssetFailed, Code: code, Detail: detail}
	}
	if prepared.tree {
		if refusal := preflightDevVenv(i); refusal != "" {
			return refuse(refusal)
		}
		if !removeTree(prepared.target) {
			return fail("directory-removal-failed", prepared.target+" could not be fully deleted")
		}
	} else {
		result, ok := run(prepared.program, prepared.args, 300*time.Second)
		// A timeout may already have changed state. Never fall back to deleting.
		if !ok {
			return fail("command-unavailable-or-timeout", prepared.program+" "+strings.Join(prepared.args, " ")+" did not complete")
		}
		if !result.OK {
			detail := strings.TrimSpace(string(result.Stderr))
			if detail == "" {
				detail = strings.TrimSpace(string(result.Stdout))
			} else {
				detail = string([]rune(detail)[:min(300, len([]rune(detail)))])
			}
			return fail("command-failed", detail)
		}
	}
	if _, err := os.Lstat(prepared.target); os.IsNotExist(err) {
		return RemovalOutcome{Status: AssetRemoved}
	}
	return fail("reported-success-but-remains", prepared.target+" still exists")
}

func inventoryRemovalRefusal(err InventoryError) RemovalRefusal {
	if err == "" {
		return ""
	}
	if err == InventoryUnreadable {
		return RemovalToolUnreadable
	}
	return RemovalToolUnavailable
}

func prepareDevRemoval(i DevAssetItem, channel string, run DevToolRunner) (preparedDevRemoval, RemovalRefusal) {
	p := preparedDevRemoval{target: i.Path, program: channel}
	switch channel {
	case "conda":
		inventory, err := InventoryConda(run, "conda")
		if err != "" {
			var second InventoryError
			inventory, second = InventoryConda(run, "mamba")
			if second != "" {
				if err == InventoryUnreadable || second == InventoryUnreadable {
					return p, RemovalToolUnreadable
				}
				return p, RemovalToolUnavailable
			}
			p.program = "mamba"
		}
		found := false
		for _, env := range inventory.Envs {
			if Norm(env) == Norm(i.Path) {
				found = true
				break
			}
		}
		if !found {
			return p, RemovalNotRegistered
		}
		if inventory.RootPrefix != nil && Norm(*inventory.RootPrefix) == Norm(i.Path) {
			return p, RemovalProtectedEnvironment
		}
		p.args = []string{"remove", "--prefix", i.Path, "--all", "--yes"}
	case "npm":
		packages, err := InventoryNpm(run)
		if err != "" {
			return p, inventoryRemovalRefusal(err)
		}
		found := false
		for _, pkg := range packages {
			if pkg.Name == i.Kind.Name && Norm(pkg.Path) == Norm(i.Path) {
				found = true
				break
			}
		}
		if !found {
			return p, RemovalNotRegistered
		}
		p.args = []string{"uninstall", "--global", i.Kind.Name}
	case "pnpm", "uv":
		root, err := InventoryRoot(run, channel)
		if err != "" {
			return p, inventoryRemovalRefusal(err)
		}
		if Norm(filepath.Join(root, i.Kind.Name)) != Norm(i.Path) {
			return p, RemovalNotRegistered
		}
		if channel == "pnpm" {
			p.args = []string{"remove", "--global", i.Kind.Name}
		} else {
			p.args = []string{"tool", "uninstall", i.Kind.Name}
		}
	case "pipx":
		tools, err := InventoryPipx(run)
		if err != "" {
			return p, inventoryRemovalRefusal(err)
		}
		found := false
		for _, tool := range tools {
			if tool.Name == i.Kind.Name && (tool.Venv == nil || Norm(*tool.Venv) == Norm(i.Path)) {
				found = true
				break
			}
		}
		if !found {
			return p, RemovalNotRegistered
		}
		p.args = []string{"uninstall", i.Kind.Name}
	case "pip":
		p.program = i.Kind.Python
		probe, ok := run(p.program, []string{"-m", "pip", "--version"}, 30*time.Second)
		if !ok || !probe.OK {
			return p, RemovalToolUnavailable
		}
		md, err := os.Stat(i.Path)
		if err != nil || !md.IsDir() {
			return p, RemovalNotRegistered
		}
		metadata, err := os.ReadFile(filepath.Join(i.Path, "METADATA"))
		if err != nil || !utf8.Valid(metadata) {
			return p, RemovalToolUnreadable
		}
		name, ok := MetadataField(string(metadata), "Name")
		if !ok || !IsValidDistributionName(name) {
			return p, RemovalToolUnreadable
		}
		p.args = []string{"-m", "pip", "uninstall", "-y", name}
	case "venv":
		p.tree = true
		return p, preflightDevVenv(i)
	default:
		return p, RemovalUnsupportedTool
	}
	return p, ""
}

func preflightDevVenv(i DevAssetItem) RemovalRefusal {
	if i.Kind.Identity == nil || !i.Kind.Identity.Recheck(i.Path) {
		return RemovalNotRegistered
	}
	md, err := os.Stat(filepath.Join(i.Path, "pyvenv.cfg"))
	if err != nil || !md.Mode().IsRegular() {
		return RemovalNotRegistered
	}
	return ""
}
