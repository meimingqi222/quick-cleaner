package core

import (
	"os"

	"golang.org/x/text/cases"
	"golang.org/x/text/language"
)

type Confidence uint8

const (
	Possible Confidence = iota
	Certain
)

func (c Confidence) IsCertain() bool { return c == Certain }
func (c Confidence) Label() string   { return c.LabelLang(Chinese) }
func (c Confidence) LabelLang(l Language) string {
	if c == Certain {
		return NewText("确定", "Certain").Get(l)
	}
	return NewText("可能", "Possible").Get(l)
}

type ResidualType uint8

const (
	ResidualFile ResidualType = iota
	ResidualDirectory
	ResidualRegistryKey
	ResidualRegistryValue
	ResidualScheduledTask
	ResidualSystemExtension
)

// Path is a filesystem path, registry subpath or task name, depending on Type.
type ResidualKind struct {
	Type                   ResidualType
	Path                   string
	Bytes                  uint64
	Root                   AppRegRoot
	Name, TeamID, BundleID string
}

func (k ResidualKind) Size() uint64 {
	if k.Type == ResidualFile || k.Type == ResidualDirectory {
		return k.Bytes
	}
	return 0
}
func (k ResidualKind) Operation() Operation {
	switch k.Type {
	case ResidualFile:
		return Operation{Kind: OpFile}
	case ResidualDirectory:
		return Operation{Kind: OpTree}
	case ResidualRegistryKey:
		return Operation{Kind: OpNative, Native: RegistryKey, Identifier: k.Root.Label() + `\` + k.Path}
	case ResidualRegistryValue:
		return Operation{Kind: OpNative, Native: RegistryValue, Identifier: k.Root.Label() + `\` + k.Path + " → " + k.Name}
	case ResidualScheduledTask:
		return Operation{Kind: OpNative, Native: ScheduledTask, Identifier: k.Path}
	case ResidualSystemExtension:
		return Operation{Kind: OpNative, Native: SystemExtension, Identifier: k.TeamID + "/" + k.BundleID}
	}
	return Operation{}
}
func (k ResidualKind) KindLabel() string { return k.KindLabelLang(Chinese) }
func (k ResidualKind) KindLabelLang(l Language) string {
	return [...]Text{NewText("文件", "File"), NewText("目录", "Directory"), NewText("注册表项", "Registry Key"), NewText("注册表值", "Registry Value"), NewText("计划任务", "Scheduled task"), NewText("系统扩展", "System extension")}[k.Type].Get(l)
}
func (k ResidualKind) DisplayLabel() string {
	switch k.Type {
	case ResidualRegistryKey, ResidualRegistryValue:
		return k.Operation().Identifier
	case ResidualSystemExtension:
		return k.BundleID
	}
	return k.Path
}

const (
	ResidualUninstallEntry       ResidualSource = "UninstallEntry"
	ResidualInstallDir           ResidualSource = "InstallDir"
	ResidualEmptyInstallParent   ResidualSource = "EmptyInstallParent"
	ResidualVendorRegKey         ResidualSource = "VendorRegKey"
	ResidualAppDataDir           ResidualSource = "AppDataDir"
	ResidualLikelyAppDataDir     ResidualSource = "LikelyAppDataDir"
	ResidualAppSupportDir        ResidualSource = "AppSupportDir"
	ResidualStartMenuDir         ResidualSource = "StartMenuDir"
	ResidualShortcut             ResidualSource = "Shortcut"
	ResidualConfigRegKey         ResidualSource = "ConfigRegKey"
	ResidualLikelyConfigRegKey   ResidualSource = "LikelyConfigRegKey"
	ResidualAppPathsEntry        ResidualSource = "AppPathsEntry"
	ResidualStartupEntry         ResidualSource = "StartupEntry"
	ResidualUserEnvEntry         ResidualSource = "UserEnvEntry"
	ResidualService              ResidualSource = "Service"
	ResidualLikelyService        ResidualSource = "LikelyService"
	ResidualFirewallRule         ResidualSource = "FirewallRule"
	ResidualRasTrace             ResidualSource = "RasTrace"
	ResidualLeakDiagnostics      ResidualSource = "LeakDiagnostics"
	ResidualCompatSetting        ResidualSource = "CompatSetting"
	ResidualInstallerFolderEntry ResidualSource = "InstallerFolderEntry"
	ResidualProgramNameCache     ResidualSource = "ProgramNameCache"
	ResidualDefaultProgramsEntry ResidualSource = "DefaultProgramsEntry"
	ResidualComClass             ResidualSource = "ComClass"
	ResidualShellExtension       ResidualSource = "ShellExtension"
	ResidualTaskSource           ResidualSource = "ScheduledTask"
	ResidualPrefetchFile         ResidualSource = "PrefetchFile"
	ResidualCrashDump            ResidualSource = "CrashDump"
	ResidualUninstallerLeftover  ResidualSource = "UninstallerLeftover"
	ResidualCacheDir             ResidualSource = "CacheDir"
	ResidualLogDir               ResidualSource = "LogDir"
	ResidualPreferenceFile       ResidualSource = "PreferenceFile"
	ResidualContainerDir         ResidualSource = "ContainerDir"
	ResidualApplicationScript    ResidualSource = "ApplicationScript"
	ResidualRecentDocumentList   ResidualSource = "RecentDocumentList"
	ResidualPackageReceipt       ResidualSource = "PackageReceipt"
	ResidualLaunchAgent          ResidualSource = "LaunchAgent"
	ResidualLaunchDaemon         ResidualSource = "LaunchDaemon"
	ResidualDotConfigDir         ResidualSource = "DotConfigDir"
	ResidualExtensionSource      ResidualSource = "SystemExtension"
	ResidualOther                ResidualSource = "Other"
)

var residualSourceText = map[ResidualSource]Text{
	ResidualUninstallEntry: NewText("卸载登记项", "Uninstall entry"), ResidualInstallDir: NewText("安装目录", "Install directory"), ResidualEmptyInstallParent: NewText("空的安装父目录", "Empty install parent"), ResidualVendorRegKey: NewText("厂商配置项", "Vendor registry key"), ResidualAppDataDir: NewText("应用数据目录", "App data directory"), ResidualLikelyAppDataDir: NewText("疑似应用数据目录", "Likely app data directory"), ResidualAppSupportDir: NewText("应用支持目录", "Application Support directory"), ResidualStartMenuDir: NewText("开始菜单目录", "Start menu folder"), ResidualShortcut: NewText("快捷方式", "Shortcut"), ResidualConfigRegKey: NewText("配置注册表项", "Config registry key"), ResidualLikelyConfigRegKey: NewText("疑似配置注册表项", "Likely config registry key"), ResidualAppPathsEntry: NewText("App Paths 登记", "App Paths entry"), ResidualStartupEntry: NewText("开机启动项", "Startup entry"), ResidualUserEnvEntry: NewText("用户环境变量", "User environment variable"), ResidualService: NewText("服务", "Service"), ResidualLikelyService: NewText("疑似服务", "Likely service"), ResidualFirewallRule: NewText("防火墙规则", "Firewall rule"), ResidualRasTrace: NewText("RAS 跟踪记录", "RAS tracing entry"), ResidualLeakDiagnostics: NewText("内存泄漏诊断记录", "Leak diagnostics entry"), ResidualCompatSetting: NewText("兼容性设置", "Compatibility setting"), ResidualInstallerFolderEntry: NewText("安装器目录登记", "Installer folder entry"), ResidualProgramNameCache: NewText("程序名缓存", "Program name cache"), ResidualDefaultProgramsEntry: NewText("默认程序登记", "Default programs entry"), ResidualComClass: NewText("COM 组件", "COM class"), ResidualShellExtension: NewText("右键菜单/外壳扩展", "Shell extension"), ResidualTaskSource: NewText("计划任务", "Scheduled task"), ResidualPrefetchFile: NewText("Prefetch 预读取", "Prefetch file"), ResidualCrashDump: NewText("崩溃转储", "Crash dump"), ResidualUninstallerLeftover: NewText("卸载器残骸", "Uninstaller leftover"), ResidualCacheDir: NewText("缓存目录", "Cache directory"), ResidualLogDir: NewText("日志目录", "Log directory"), ResidualPreferenceFile: NewText("偏好设置文件", "Preference file"), ResidualContainerDir: NewText("沙盒容器", "Sandbox container"), ResidualApplicationScript: NewText("应用脚本目录", "Application scripts"), ResidualRecentDocumentList: NewText("最近使用记录", "Recent document list"), ResidualPackageReceipt: NewText("安装收据", "Package receipt"), ResidualLaunchAgent: NewText("登录启动项", "Launch agent"), ResidualLaunchDaemon: NewText("系统守护进程", "Launch daemon"), ResidualDotConfigDir: NewText("配置目录", "Dotfile config directory"), ResidualExtensionSource: NewText("系统扩展（需在系统设置中关闭）", "System extension (turn off in System Settings)"), ResidualOther: NewText("其他残留", "Other residual"),
}

func (s ResidualSource) Label() string               { return s.LabelLang(Chinese) }
func (s ResidualSource) LabelLang(l Language) string { return residualSourceText[s].Get(l) }

type ResidualItem struct {
	Rule                 *RuleRef
	Kind                 ResidualKind
	Confidence           Confidence
	RequiresConfirmation bool
	Source               ResidualSource
	Identity             *TargetIdentity
	OwnerBundleID        *string
}

func NewResidualItem(k ResidualKind, s ResidualSource, c Confidence) ResidualItem {
	i := ResidualItem{Kind: k, Source: s, Confidence: c}
	if k.Type == ResidualFile || k.Type == ResidualDirectory {
		if id, ok := CaptureIdentity(k.Path); ok {
			i.Identity = &id
		}
	}
	return i
}
func CertainResidual(k ResidualKind, s ResidualSource) ResidualItem {
	return NewResidualItem(k, s, Certain)
}
func PossibleResidual(k ResidualKind, s ResidualSource) ResidualItem {
	return NewResidualItem(k, s, Possible)
}
func (i ResidualItem) ForBundle(id string) ResidualItem { i.OwnerBundleID = &id; return i }
func (i ResidualItem) Owner() (string, bool) {
	if i.OwnerBundleID == nil || *i.OwnerBundleID == "" {
		return "", false
	}
	return *i.OwnerBundleID, true
}
func (i ResidualItem) Size() uint64         { return i.Kind.Size() }
func (i ResidualItem) DisplayLabel() string { return i.DisplayLabelLang(Chinese) }
func (i ResidualItem) DisplayLabelLang(l Language) string {
	return "[" + i.Source.LabelLang(l) + "] " + i.Kind.DisplayLabel()
}
func residualKey(k ResidualKind) string {
	switch k.Type {
	case ResidualFile, ResidualDirectory:
		return "fs:" + Norm(k.Path)
	case ResidualRegistryKey:
		return "key:" + k.Root.Label() + ":" + Norm(k.Path)
	case ResidualRegistryValue:
		return "val:" + k.Root.Label() + ":" + Norm(k.Path) + ":" + cases.Lower(language.Und).String(k.Name)
	case ResidualScheduledTask:
		return "task:" + cases.Lower(language.Und).String(k.Path)
	case ResidualSystemExtension:
		return "ext:" + k.TeamID + ":" + k.BundleID
	}
	return ""
}
func residualCovers(parent, child ResidualKind) bool {
	switch parent.Type {
	case ResidualDirectory:
		return (child.Type == ResidualFile || child.Type == ResidualDirectory) && atOrUnder(Norm(child.Path), Norm(parent.Path))
	case ResidualRegistryKey:
		return (child.Type == ResidualRegistryKey || child.Type == ResidualRegistryValue) && parent.Root == child.Root && atOrUnder(Norm(child.Path), Norm(parent.Path))
	}
	return false
}

// DedupeResiduals returns stable survivors; absorbing children never grants authority.
func DedupeResiduals(items []ResidualItem) []ResidualItem {
	kept := []ResidualItem{}
	positions := map[string]int{}
	for _, i := range items {
		key := residualKey(i.Kind)
		if n, ok := positions[key]; ok {
			manual := i.RequiresConfirmation || kept[n].RequiresConfirmation
			if i.Confidence > kept[n].Confidence {
				kept[n] = i
			}
			kept[n].RequiresConfirmation = manual
		} else {
			positions[key] = len(kept)
			kept = append(kept, i)
		}
	}
	out := []ResidualItem{}
	for n, i := range kept {
		drop := false
		for m, j := range kept {
			if j.RequiresConfirmation && residualCovers(i.Kind, j.Kind) {
				i.RequiresConfirmation = true
			}
			if n != m && residualCovers(j.Kind, i.Kind) {
				drop = true
			}
		}
		if !drop {
			out = append(out, i)
		}
	}
	return out
}

type ResidualOccupancy struct{ Processes, LaunchdLabels []string }

func (o ResidualOccupancy) IsOccupied() bool { return len(o.Processes) > 0 || len(o.LaunchdLabels) > 0 }

type ResidualScope uint8

const (
	ResidualScopeApp ResidualScope = iota
	ResidualScopeOrphanLeftovers
)

type ResidualScanResult struct {
	AppName, AppID string
	Scope          ResidualScope
	Items          []ResidualItem
	TotalFileSize  uint64
	Occupancy      ResidualOccupancy
}

// CleanupPlan is diagnostic only: platform ownership/occupancy checks remain mandatory.
func (r ResidualScanResult) CleanupPlan() *CleanupPlan {
	if len(r.Items) == 0 {
		return nil
	}
	rule := EngineRuleRef()
	for _, i := range r.Items {
		if i.Rule != nil {
			rule = *i.Rule
			break
		}
	}
	targets := make([]PlannedTarget, 0, len(r.Items))
	for _, i := range r.Items {
		op := i.Kind.Operation()
		path := i.Kind.Path
		if op.Kind == OpNative {
			path = op.Identifier
		}
		targets = append(targets, PlannedTarget{Path: path, Operation: op, Identity: i.Identity, Disposal: Permanent})
	}
	return NewCleanupPlan(rule, targets)
}
func (r ResidualScanResult) DefaultSelection() map[int]struct{} {
	out := map[int]struct{}{}
	for n, i := range r.Items {
		if i.Confidence.IsCertain() && !i.RequiresConfirmation {
			out[n] = struct{}{}
		}
	}
	return out
}
func (r ResidualScanResult) CertainCount() int {
	n := 0
	for _, i := range r.Items {
		if i.Confidence.IsCertain() {
			n++
		}
	}
	return n
}
func AppGoneAfterResidualClean(original, remaining []ResidualItem) bool {
	for _, s := range []ResidualSource{ResidualUninstallEntry, ResidualInstallDir} {
		had, has := false, false
		for _, i := range original {
			had = had || i.Source == s
		}
		for _, i := range remaining {
			has = has || i.Source == s
		}
		if had && !has {
			return true
		}
	}
	return false
}
func DiscoveredProgramFilesAbsent(d AppDiscovery) bool {
	if len(d.ProgramPaths) == 0 {
		return false
	}
	for _, p := range d.ProgramPaths {
		if _, err := os.Lstat(p); !os.IsNotExist(err) {
			return false
		}
	}
	return true
}

type ResidualCleanFollowUp struct {
	RetryItems     []ResidualItem
	RetrySelected  map[int]struct{}
	LeftoverForApp []ResidualItem
}

func FollowUpResidualClean(items []ResidualItem, selected map[int]struct{}, stillPresent func(ResidualItem) bool) ResidualCleanFollowUp {
	out := ResidualCleanFollowUp{RetrySelected: map[int]struct{}{}}
	for n, i := range items {
		if _, ok := selected[n]; !ok {
			out.LeftoverForApp = append(out.LeftoverForApp, i)
		} else if stillPresent(i) {
			out.RetrySelected[len(out.RetryItems)] = struct{}{}
			out.RetryItems = append(out.RetryItems, i)
			out.LeftoverForApp = append(out.LeftoverForApp, i)
		}
	}
	return out
}
