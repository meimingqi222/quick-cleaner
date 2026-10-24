package core

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type DevAssetType string

const (
	DevCondaEnv          DevAssetType = "conda_env"
	DevNodePackage       DevAssetType = "node_global_package"
	DevPythonTool        DevAssetType = "python_tool"
	DevPythonInterpreter DevAssetType = "python_interpreter"
	DevVirtualEnv        DevAssetType = "virtual_env"
	DevPipPackage        DevAssetType = "pip_package"
)

type AssetSource uint8

const (
	AssetLayout AssetSource = iota
	AssetTool
)

type SiteScope uint8

const (
	SiteInstall SiteScope = iota
	SiteUser
)

type VenvManager string

const (
	VenvVirtualenvwrapper VenvManager = "virtualenvwrapper"
	VenvPoetry            VenvManager = "poetry"
)

type PythonToolKind string

const (
	PythonToolUv   PythonToolKind = "uv"
	PythonToolPipx PythonToolKind = "pipx"
)

type TimestampSource string

const (
	TimeCondaTransaction TimestampSource = "conda_transaction"
	TimePackageManifest  TimestampSource = "package_manifest"
	TimeToolMetadata     TimestampSource = "tool_metadata"
	TimeDirectoryEntry   TimestampSource = "directory_entry"
	TimeSitePackages     TimestampSource = "site_packages"
)

// TimedEvidence records ecosystem changes, never proof of last use.
type TimedEvidence struct {
	At     time.Time
	Source TimestampSource
}

func (e TimedEvidence) AgeDays(now time.Time) uint64 {
	if now.Before(e.At) {
		return 0
	}
	// time.Duration saturates after 292 years; filesystem epochs can be older.
	seconds := uint64(now.Unix()) - uint64(e.At.Unix())
	if now.Nanosecond() < e.At.Nanosecond() {
		seconds--
	}
	return seconds / 86_400
}

type DevAssetKind struct {
	Type          DevAssetType
	Name          string
	IsBase        bool
	PythonVersion *string
	Version       *string
	PackageCount  *int
	Manager       string
	BinShims      []string
	ToolKind      PythonToolKind
	Executables   []string
	Scope         SiteScope
	SitePackages  *string
	Python        string
	BasePython    *string
	VenvManager   VenvManager
	Identity      *VenvIdentity
}

type DevAssetItem struct {
	ID          string
	Kind        DevAssetKind
	Path        string
	Size        AssetStorageSize
	LastChange  *TimedEvidence
	InUse       SpotCheck
	IsActiveEnv bool
	Source      AssetSource
}

func (i DevAssetItem) DisplayName() string { return i.Kind.Name }
func (i DevAssetItem) IsProtected() bool {
	return i.IsActiveEnv || (i.Kind.Type == DevCondaEnv && i.Kind.IsBase)
}

// VenvIdentity is an object identity, not a mutable directory timestamp.
type VenvIdentity struct{ object [2]uint64 }

func CaptureVenvIdentity(path string) *VenvIdentity {
	if !filepath.IsAbs(path) {
		return nil
	}
	volume := filepath.VolumeName(path)
	root := volume + string(filepath.Separator)
	separators := string(filepath.Separator)
	if runtime.GOOS == "windows" {
		separators = `\/`
	}
	for current, depth := strings.TrimRight(path, separators), 0; ; depth++ {
		if len(current) <= len(volume) {
			current = root
		}
		if depth >= 128 {
			return nil
		}
		md, err := os.Lstat(current)
		if err != nil || !md.IsDir() || ruleEvidenceLink(md) {
			return nil
		}
		if current == root {
			break
		}
		// Do not clean '..' before checking its preceding component for links.
		end := strings.LastIndexAny(current, separators)
		if end < len(volume) {
			current = root
		} else {
			current = strings.TrimRight(current[:end], separators)
		}
	}
	_, object, ok := platform.DevEnvStorageFileInfo(path)
	if !ok {
		return nil
	}
	return &VenvIdentity{object: object}
}

func (id VenvIdentity) Recheck(path string) bool {
	now := CaptureVenvIdentity(path)
	return now != nil && *now == id
}
