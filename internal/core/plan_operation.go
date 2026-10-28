package core

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type OperationKind string

const (
	OpFile              OperationKind = "file"
	OpTree              OperationKind = "tree"
	OpGitWorktree       OperationKind = "git_worktree"
	OpContents          OperationKind = "contents"
	OpDocker            OperationKind = "docker"
	OpBrew              OperationKind = "brew"
	OpGo                OperationKind = "go"
	OpPnpm              OperationKind = "pnpm"
	OpSnapshot          OperationKind = "snapshot"
	OpTrash             OperationKind = "trash"
	OpNative            OperationKind = "native"
	OpRegistration      OperationKind = "registration"
	OpOfficialUninstall OperationKind = "official_uninstall"
)

func (k OperationKind) Label(lang Language) string {
	zh := lang == Chinese
	switch k {
	case OpFile:
		if zh {
			return "删除文件"
		}
		return "delete file"
	case OpTree:
		if zh {
			return "删除目录树"
		}
		return "delete folder tree"
	case OpContents:
		if zh {
			return "清空目录内容"
		}
		return "empty folder contents"
	case OpGitWorktree:
		if zh {
			return "移除工作区"
		}
		return "remove worktree"
	case OpDocker:
		if zh {
			return "删除镜像"
		}
		return "remove image"
	case OpBrew:
		if zh {
			return "brew 清理"
		}
		return "brew cleanup"
	case OpGo:
		if zh {
			return "Go 缓存清理"
		}
		return "Go cache cleanup"
	case OpPnpm:
		if zh {
			return "pnpm store 清理"
		}
		return "pnpm store cleanup"
	case OpSnapshot:
		if zh {
			return "删除快照"
		}
		return "delete snapshot"
	case OpTrash:
		if zh {
			return "清空回收站"
		}
		return "empty trash"
	case OpRegistration:
		if zh {
			return "删除登记项"
		}
		return "remove registration"
	case OpOfficialUninstall:
		if zh {
			return "运行官方卸载"
		}
		return "run official uninstaller"
	case OpNative:
		if zh {
			return "删除原生残留"
		}
		return "remove native residue"
	default:
		return string(k)
	}
}

type NativeKind string

const (
	RegistryKey     NativeKind = "registry_key"
	RegistryValue   NativeKind = "registry_value"
	ScheduledTask   NativeKind = "scheduled_task"
	SystemExtension NativeKind = "system_extension"
)

type Operation struct {
	Kind         OperationKind `json:"kind"`
	Registration string        `json:"registration,omitempty"`
	Reference    string        `json:"reference,omitempty"`
	Name         string        `json:"name,omitempty"`
	Native       NativeKind    `json:"native,omitempty"`
	Identifier   string        `json:"identifier,omitempty"`
}
type ExecutionRoute string

const (
	RouteTrash      ExecutionRoute = "trash"
	RouteNative     ExecutionRoute = "native"
	RouteOwner      ExecutionRoute = "owner"
	RouteFilesystem ExecutionRoute = "filesystem"
	RouteWorktree   ExecutionRoute = "worktree"
	RouteUnrouted   ExecutionRoute = "unrouted"
)

func (o Operation) RemovesDirectory() bool {
	return o.Kind == OpFile || o.Kind == OpTree || o.Kind == OpGitWorktree
}
func (o Operation) CarriesScanSize() bool {
	return o.IsNativeResource() || o.Kind == OpGo || o.Kind == OpPnpm
}
func (o Operation) CoversSubtree() bool {
	return o.Kind == OpTree || o.Kind == OpContents || o.Kind == OpGo || o.Kind == OpPnpm || o.Kind == OpGitWorktree || o.Kind == OpTrash
}
func (o Operation) IsFilesystemBacked() bool {
	return o.RemovesDirectory() || o.Kind == OpContents || o.Kind == OpGo || o.Kind == OpPnpm
}
func (o Operation) IsNativeResource() bool {
	return o.Kind == OpDocker || o.Kind == OpSnapshot || o.Kind == OpBrew || o.Kind == OpNative
}
func (o Operation) Route() ExecutionRoute {
	switch o.Kind {
	case OpTrash:
		return RouteTrash
	case OpDocker, OpSnapshot, OpBrew:
		return RouteNative
	case OpGo, OpPnpm:
		return RouteOwner
	case OpFile, OpTree, OpContents:
		return RouteFilesystem
	case OpGitWorktree:
		return RouteWorktree
	}
	return RouteUnrouted
}

var planSnapshotDate = regexp.MustCompile(`^\d{4}-\d{2}-\d{2}-\d{6}$`)

func SnapshotDate(name string) (string, bool) {
	date := name
	if strings.HasPrefix(name, "com.apple.TimeMachine.") && strings.HasSuffix(name, ".local") {
		date = strings.TrimSuffix(strings.TrimPrefix(name, "com.apple.TimeMachine."), ".local")
	}
	if !planSnapshotDate.MatchString(date) {
		return "", false
	}
	_, err := time.Parse("2006-01-02-150405", date)
	return date, err == nil
}

func ValidDockerReference(reference string) bool {
	if reference == "" || len(reference) > 512 || reference[0] == '-' {
		return false
	}
	for _, b := range []byte(reference) {
		if !(b >= 'a' && b <= 'z' || b >= 'A' && b <= 'Z' || b >= '0' && b <= '9' || strings.ContainsRune("_.-/:@", rune(b))) {
			return false
		}
	}
	return true
}

func (o Operation) TargetKey(path string) string {
	switch o.Kind {
	case OpDocker:
		return "docker:" + o.Reference
	case OpSnapshot:
		n := o.Name
		if date, ok := SnapshotDate(n); ok {
			n = date
		}
		return "snapshot:" + n
	case OpBrew:
		return "brew:cleanup"
	case OpNative:
		names := map[NativeKind]string{RegistryKey: "RegistryKey", RegistryValue: "RegistryValue", ScheduledTask: "ScheduledTask", SystemExtension: "SystemExtension"}
		return fmt.Sprintf("native:%s:%s", names[o.Native], o.Identifier)
	}
	return "path:" + Norm(path)
}
func (o Operation) ForScannedPath(path string) Operation {
	if md, e := os.Lstat(path); e == nil && md.Mode().IsRegular() && (o.Kind == OpTree || o.Kind == OpContents) {
		return Operation{Kind: OpFile}
	}
	if md, e := os.Lstat(filepath.Join(path, ".git")); o.Kind == OpTree && e == nil && md.Mode().IsRegular() {
		registration := filepath.Join(path, ".git")
		if entry, err := InspectWorktree(path); err == nil {
			registration = entry.Admin
		}
		return Operation{Kind: OpGitWorktree, Registration: registration}
	}
	return o
}

func (o Operation) ValidateTarget(path string, removeDirectory bool) bool {
	if o.Kind == OpNative {
		return member(string(o.Native), []string{string(RegistryKey), string(RegistryValue), string(ScheduledTask), string(SystemExtension)}) && o.Identifier != ""
	}
	if o.IsNativeResource() {
		if !IsVirtualPath(path) {
			return false
		}
		switch o.Kind {
		case OpDocker:
			return ValidDockerReference(o.Reference)
		case OpBrew:
			return true
		case OpSnapshot:
			_, ok := SnapshotDate(o.Name)
			return ok
		}
		return false
	}
	if IsVirtualPath(path) {
		return false
	}
	switch o.Kind {
	case OpGo:
		return IsGoModcache(path)
	case OpPnpm:
		return IsPnpmStore(path)
	case OpTrash:
		return platform.IsSystemTrash(path) || platform.IsVolumeTrash(path) || platform.IsPerDriveTrash(path)
	}
	if IsGoModcache(path) || IsPnpmStore(path) || platform.IsSystemTrash(path) || platform.IsVolumeTrash(path) || platform.IsPerDriveTrash(path) {
		return false
	}
	md, e := os.Lstat(path)
	if e != nil || ruleEvidenceLink(md) {
		return false
	}
	switch o.Kind {
	case OpFile:
		return removeDirectory && md.Mode().IsRegular()
	case OpTree:
		git, err := os.Stat(filepath.Join(path, ".git"))
		return removeDirectory && md.IsDir() && !(err == nil && git.Mode().IsRegular())
	case OpGitWorktree:
		entry, err := InspectWorktree(path)
		return removeDirectory && err == nil && Norm(entry.Admin) == Norm(o.Registration)
	case OpContents:
		return !removeDirectory && md.IsDir()
	}
	return false
}
