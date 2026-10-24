package app

import (
	"context"
	"fmt"
	"runtime"
	"runtime/debug"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func (r *Root) scanApps() {
	if r.busy() {
		return
	}
	r.apps.Scanning = true
	r.appIconGen++
	r.appIconTried = map[string]bool{}
	r.appIcons = map[string]*ui.Bitmap{}
	r.message("正在智能检索已安装软件与空间占用…", "Discovering installed apps and their disk usage…")
	r.start(false, nil, func(ctx context.Context) func() {
		apps, err := core.ListInstalledAppsProgress(ctx, func(partial []core.InstalledApp) {
			snapshot := partial
			r.publish(func() {
				if r.closed || r.job == nil {
					return
				}
				r.installed = snapshot
				r.appsLoaded = true
				r.dashboard.AppsScanned = true
				r.dashboard.AppCount = len(snapshot)
				r.rebuildApps()
				r.kickAppIcons(snapshot)
				r.message(fmt.Sprintf("已列出 %d 款软件，正在测算占用…", len(snapshot)), fmt.Sprintf("Listed %d apps, measuring disk usage…", len(snapshot)))
			})
		})
		return func() {
			r.apps.Scanning = false
			if err != nil {
				r.failure(err)
				r.apps.Notice = core.SameText(err.Error())
				return
			}
			r.installed = apps
			r.appsLoaded = true
			r.dashboard.AppsScanned = true
			r.dashboard.AppCount = len(apps)
			r.rebuildApps()
			r.kickAppIcons(apps)
			r.message(fmt.Sprintf("已加载 %d 款软件，估算总占用 %s", len(apps), r.apps.TotalSize), fmt.Sprintf("Loaded %d apps, about %s in total", len(apps), r.apps.TotalSize))
			if !r.apps.ResidualsOpen {
				r.apps.Notice = core.Text{}
			}
		}
	})
}

func (r *Root) kickAppIcons(apps []core.InstalledApp) {
	if r.appIconTried == nil {
		r.appIconTried = map[string]bool{}
	}
	type iconJob struct{ id, path string }
	var jobs []iconJob
	for _, app := range apps {
		if r.appIconTried[app.ID] {
			continue
		}
		r.appIconTried[app.ID] = true
		path, ok := app.IconCacheKey()
		if !ok {
			continue
		}
		jobs = append(jobs, iconJob{app.ID, path})
	}
	if len(jobs) == 0 {
		return
	}
	gen := r.appIconGen
	go func() {
		defer func() {
			if rec := recover(); rec != nil {
				core.LogPanic("app-icons", rec, debug.Stack())
			}
		}()
		pending := map[string]*ui.Bitmap{}
		flush := func() {
			if len(pending) == 0 {
				return
			}
			got := pending
			pending = map[string]*ui.Bitmap{}
			r.publish(func() {
				if r.closed || gen != r.appIconGen {
					return
				}
				r.mergeAppIcons(got)
			})
		}
		for _, job := range jobs {
			if r.ctx.Err() != nil {
				break
			}
			data := platform.AppIconPNG(job.path)
			if len(data) == 0 {
				continue
			}
			bitmap, err := ui.DecodeBitmap(data)
			if err != nil || bitmap == nil {
				continue
			}
			pending[job.id] = bitmap
			if len(pending) >= 12 {
				flush()
			}
		}
		flush()
	}()
}

func (r *Root) mergeAppIcons(batch map[string]*ui.Bitmap) {
	if r.appIcons == nil {
		r.appIcons = map[string]*ui.Bitmap{}
	}
	for id, bitmap := range batch {
		r.appIcons[id] = bitmap
	}
	for i := range r.apps.Apps {
		if bitmap, ok := batch[r.apps.Apps[i].ID]; ok {
			r.apps.Apps[i].Icon = bitmap
		}
	}
}

func (r *Root) rebuildApps() {
	r.apps.Apps = nil
	var total, filtered uint64
	for _, a := range r.installed {
		row := view.AppsPageApp{ID: a.ID, Name: a.Name, Version: a.Version, Publisher: a.Publisher, Size: core.FmtSize(a.EstimatedSize), Discovered: a.Discovery != nil, CanUninstall: a.CanUninstall(), HasLocation: a.InstallLocation != nil, ManualUninstall: core.RequiresManualUninstall(a), UninstallDeletesData: core.UninstallDeletesUserData(a)}
		row.Icon = r.appIcons[a.ID]
		if a.LastUsedDate != nil {
			row.LastUsed = *a.LastUsedDate
		}
		if a.InstallDate != nil {
			row.InstalledDate = *a.InstallDate
		}
		r.apps.Apps = append(r.apps.Apps, row)
		total += a.EstimatedSize
	}
	r.apps.View = core.FilterAndSortApps(r.installed, r.apps.Preset, r.apps.Search, r.apps.Sort)
	r.apps.StaleCount = 0
	for _, a := range r.installed {
		if core.IsRarelyUsed(a, core.NowUnixSecs()) {
			r.apps.StaleCount++
		}
	}
	for _, i := range r.apps.View {
		filtered += r.installed[i].EstimatedSize
	}
	r.apps.TotalSize, r.apps.FilteredSize = core.FmtSize(total), core.FmtSize(filtered)
}

func (r *Root) findApp(id string) (core.InstalledApp, bool) {
	for _, a := range r.installed {
		if a.ID == id {
			return a, true
		}
	}
	return core.InstalledApp{}, false
}

func (r *Root) appsActions() view.AppsPageActions {
	return view.AppsPageActions{
		SetTab: func(t view.AppsPageTab) {
			r.apps.Tab = t
			if t == view.AppsPageDevEnvironments && !r.devLoaded {
				r.scanDev()
			}
		}, SetPreset: func(p core.AppFilterPreset) { r.apps.Preset = p; r.rebuildApps() }, SetSearch: func(s string) { r.apps.Search = s; r.rebuildApps() }, SortBy: func(c core.AppSortColumn) { r.apps.Sort.Toggle(c); r.rebuildApps() },
		RequestUninstall: r.requestUninstall, ScanResiduals: r.scanResiduals, OpenLocation: func(id string) {
			if a, ok := r.findApp(id); ok && a.InstallLocation != nil {
				r.open(*a.InstallLocation)
			}
		}, CopyInstallPath: func(id string) {
			if a, ok := r.findApp(id); ok && a.InstallLocation != nil {
				r.copy(*a.InstallLocation)
			}
		},
		RenderDevEnvironments: func(c *ui.Context) { r.dev.RenderContent(c, r.devActions()) },
		ScanOrphanLeftovers:   r.scanOrphanResiduals, CleanSelectedResiduals: r.requestResidualClean,
		OpenSystemExtensionSettings: func() { go func() { err := platform.OpenLoginItemsSettings(); r.publish(func() { r.failure(err) }) }() },
		CloseResiduals: func() {
			if !r.busy() {
				r.apps.ResidualsOpen = false
			}
		},
		SelectRecommended: func() { r.selectResiduals(1) }, SelectAllResiduals: func() { r.selectResiduals(2) }, ClearResidualSelection: func() { r.selectResiduals(0) },
		SetResidualSelected: func(id string, selected bool) {
			if r.busy() {
				return
			}
			for i := range r.apps.Residuals {
				if r.apps.Residuals[i].ID == id && !r.apps.Residuals[i].Manual {
					r.apps.Residuals[i].Selected = selected
				}
			}
			r.residualSummary()
		},
	}
}

func (r *Root) requestUninstall(id string) {
	a, ok := r.findApp(id)
	if !ok || !a.CanUninstall() || r.busy() {
		return
	}
	if core.RequiresManualUninstall(a) {
		r.apps.Notice = core.NewText("请在应用的官方设置中卸载；不会直接删除应用。", "Uninstall in the application's official settings; no direct deletion was performed.")
		return
	}
	// A replaced bundle must not inherit the reviewed app's removal request.
	var identity *core.TargetIdentity
	if a.InstallLocation != nil {
		if v, ok := core.CaptureIdentity(*a.InstallLocation); ok {
			identity = &v
		}
	}
	detail := a.Name
	if a.InstallLocation != nil {
		detail += "\n" + *a.InstallLocation
	}
	if core.UninstallDeletesUserData(a) {
		detail += "\n\n" + r.tr("官方卸载器也会删除用户数据。", "The official uninstaller also deletes user data.")
	}
	detail += "\n\n" + r.tr("运行官方卸载路线。失败不会授权后备删除；残留必须另行审查。", "Runs the official uninstall route. Failure never authorizes fallback deletion; residuals require separate review.")
	r.askKind(view.ConfirmUninstall, r.tr("确认卸载应用", "Uninstall application?"), detail, func() {
		r.apps.Uninstalling = true
		r.apps.UninstallPhase = view.AppsPageDiscovering
		r.apps.UninstallAppName = a.Name
		r.start(true, nil, func(ctx context.Context) func() {
			// Freeze discovery before official removal destroys ownership evidence.
			frozen, scanErr := scanAppResiduals(ctx, a)
			r.publish(func() {
				if r.apps.Uninstalling {
					r.apps.UninstallPhase = view.AppsPageRemoving
				}
			})
			var err error
			if ctx.Err() != nil {
				err = ctx.Err()
			} else if runtime.GOOS == "darwin" && a.InstallLocation != nil && (identity == nil || !identity.Recheck(*a.InstallLocation)) {
				err = fmt.Errorf("application installation identity unavailable or changed")
			} else {
				err = uninstallApp(a)
			}
			r.publish(func() {
				if r.apps.Uninstalling {
					r.apps.UninstallPhase = view.AppsPageVerifying
				}
			})
			var remaining []core.ResidualItem
			if err == nil && scanErr == nil && ctx.Err() == nil {
				remaining, scanErr = verifyResiduals(ctx, frozen.Items)
			}
			cancelled := ctx.Err() != nil
			return func() {
				r.apps.Uninstalling = false
				if err != nil {
					r.apps.Notice = uninstallFailureNotice(err)
					r.failure(err)
					r.record("uninstall", []string{a.Name}, 0, 0, 1, 0)
					return
				}
				r.record("uninstall", []string{a.Name}, 1, 0, 0, 0)
				r.apps.Notice = core.NewText("官方卸载已验证；残留未自动删除。", "Official removal verified; residuals were not automatically deleted.")
				if scanErr != nil {
					r.apps.Notice = core.SameText("Removal verified; residual review unavailable: " + scanErr.Error())
				}
				if !cancelled && scanErr == nil {
					frozen.Items = remaining
					r.showResiduals(frozen, &a)
				}
				if !cancelled {
					r.scanApps()
				}
			}
		})
	})
}

func (r *Root) scanResiduals(id string) {
	a, ok := r.findApp(id)
	if !ok || r.busy() {
		return
	}
	r.apps.ResidualScanning = true
	r.start(false, nil, func(ctx context.Context) func() {
		result, err := scanAppResiduals(ctx, a)
		return func() {
			r.apps.ResidualScanning = false
			if err != nil {
				r.failure(err)
				r.apps.Notice = core.SameText(err.Error())
				return
			}
			r.showResiduals(result, &a)
		}
	})
}

func (r *Root) scanDev() {
	if r.busy() {
		return
	}
	r.dev.Scanning = true
	r.message("正在扫描开发环境…", "Scanning developer environments…")
	r.start(false, nil, func(ctx context.Context) func() {
		result := core.DiscoverDevAssets()
		cancelled := ctx.Err()
		return func() {
			r.dev.Scanning = false
			if cancelled != nil {
				r.failure(cancelled)
				return
			}
			r.devLoaded = true
			r.dev.Assets = append(append(append(result.CondaEnvs, result.PythonEnvs...), result.NodePackages...), result.PythonTools...)
			r.dev.TotalLogical = result.Total.LogicalBytes
			r.dev.TotalReclaimable = result.RemovableExclusive
			live := map[string]bool{}
			for _, item := range r.dev.Assets {
				live[item.ID] = true
			}
			for id := range r.dev.Selected {
				if !live[id] {
					delete(r.dev.Selected, id)
				}
			}
			if !live[r.dev.ActiveID] {
				r.dev.ActiveID = ""
			}
			r.dev.Packages = map[string][]core.DevAssetItem{}
			r.message(fmt.Sprintf("已加载 %d 个开发环境资产，可释放 %s", len(r.dev.Assets), core.FmtSize(result.RemovableExclusive)), fmt.Sprintf("Loaded %d developer assets, %s reclaimable", len(r.dev.Assets), core.FmtSize(result.RemovableExclusive)))
		}
	})
}

func (r *Root) selectDev(id string) {
	r.dev.ActiveID = id
	for _, item := range r.dev.Assets {
		if item.ID != id {
			continue
		}
		if r.busy() {
			return
		}
		if _, ok := r.dev.Packages[id]; ok {
			return
		}
		if item.Kind.Type != core.DevVirtualEnv && (item.Kind.Type != core.DevPythonInterpreter || item.Kind.Scope != core.SiteUser || item.Kind.SitePackages == nil) {
			return
		}
		r.dev.LoadingSite = id
		r.start(false, nil, func(ctx context.Context) func() {
			var site, python string
			if item.Kind.Type == core.DevVirtualEnv {
				var siteOK, pythonOK bool
				site, siteOK = core.VenvSitePackages(item.Path)
				python, pythonOK = core.VenvPython(item.Path)
				if !siteOK || !pythonOK {
					return func() {
						r.dev.LoadingSite = ""
						r.message("环境包目录或解释器不可用。", "Environment package directory or interpreter unavailable.")
					}
				}
			} else {
				site, python = *item.Kind.SitePackages, item.Kind.Python
			}
			packages := core.PipPackages(site, python)
			paths := make([]string, len(packages))
			for i := range packages {
				paths[i] = packages[i].Path
			}
			occupancy := core.SpotCheckInUse(paths)
			for i := range packages {
				packages[i].InUse = core.SpotUnknown
				if state, known := occupancy[packages[i].Path]; known {
					packages[i].InUse = state
				}
			}
			cancelled := ctx.Err()
			return func() {
				r.dev.LoadingSite = ""
				if cancelled != nil {
					r.failure(cancelled)
					return
				}
				r.dev.Packages[id] = packages
				if r.dev.ActiveID != id {
					r.selectDev(r.dev.ActiveID)
				}
			}
		})
		return
	}
}

func (r *Root) devItems() []core.DevAssetItem {
	items := slices.Clone(r.dev.Assets)
	for _, ps := range r.dev.Packages {
		items = append(items, ps...)
	}
	return items
}
func (r *Root) devActions() view.DevEnvPageActions {
	return view.DevEnvPageActions{Rescan: r.scanDev, SetSearch: func(s string) { r.dev.Search = s }, SetFilter: func(f view.DevEnvFilter) { r.dev.Filter = f }, SelectAsset: r.selectDev,
		ToggleSelected: func(id string) {
			if r.busy() {
				return
			}
			for _, i := range r.devItems() {
				if i.ID == id && core.CanRemoveDevAsset(i) {
					r.dev.Selected[id] = !r.dev.Selected[id]
				}
			}
		},
		ClearSelection: func() {
			if !r.busy() {
				r.dev.Selected = map[string]bool{}
			}
		}, SelectAll: func() {
			if !r.busy() {
				for _, i := range r.dev.Assets {
					if core.CanRemoveDevAsset(i) {
						r.dev.Selected[i.ID] = true
					}
				}
			}
		}, InvertSelection: func() {
			if !r.busy() {
				for _, i := range r.dev.Assets {
					if core.CanRemoveDevAsset(i) {
						r.dev.Selected[i.ID] = !r.dev.Selected[i.ID]
					}
				}
			}
		},
		SelectSitePackages: func(id string) {
			if !r.busy() {
				for _, i := range r.dev.Packages[id] {
					if core.CanRemoveDevAsset(i) {
						r.dev.Selected[i.ID] = true
					}
				}
			}
		}, InvertSitePackages: func(id string) {
			if !r.busy() {
				for _, i := range r.dev.Packages[id] {
					if core.CanRemoveDevAsset(i) {
						r.dev.Selected[i.ID] = !r.dev.Selected[i.ID]
					}
				}
			}
		},
		RequestRemove: func(id string) { r.requestDevRemoval(map[string]bool{id: true}) }, RequestRemoveSelected: func() { r.requestDevRemoval(r.dev.Selected) },
	}
}

func devWhat(r *Root, item core.DevAssetItem) string {
	name := item.Kind.Name
	switch item.Kind.Type {
	case core.DevCondaEnv:
		return r.tr(fmt.Sprintf(" conda 环境「%s」", name), fmt.Sprintf(" the conda environment \"%s\"", name))
	case core.DevNodePackage:
		return r.tr(fmt.Sprintf(" %s 全局包「%s」", item.Kind.Manager, name), fmt.Sprintf(" the %s global package \"%s\"", item.Kind.Manager, name))
	case core.DevPythonTool:
		return r.tr(fmt.Sprintf(" CLI 工具「%s」", name), fmt.Sprintf(" the CLI tool \"%s\"", name))
	case core.DevPythonInterpreter:
		return r.tr(fmt.Sprintf(" Python 解释器「%s」的包目录", name), fmt.Sprintf(" the package directory of Python \"%s\"", name))
	case core.DevVirtualEnv:
		return r.tr(fmt.Sprintf("虚拟环境「%s」", name), fmt.Sprintf("the virtualenv \"%s\"", name))
	case core.DevPipPackage:
		return r.tr(fmt.Sprintf("pip 包「%s」", name), fmt.Sprintf("the pip package \"%s\"", name))
	default:
		return name
	}
}

func devRefusal(lang core.Language, reason core.RemovalRefusal) string {
	zh := map[core.RemovalRefusal]string{
		core.RemovalProtected:            "该路径受核心安全规则保护，不能移除",
		core.RemovalProtectedEnvironment: "base 环境与当前激活的环境不能移除",
		core.RemovalBusy:                 "该目标正被占用，或占用状态无法确认",
		core.RemovalUnsupportedTool:      "该生态的卸载命令尚未核实，只支持查看",
		core.RemovalUnverifiedSource:     "该条目来自按目录的降级扫描，未经生态确认，只支持查看",
		core.RemovalToolUnavailable:      "该生态的命令不可用（未安装或不在 PATH 上），无法安全移除",
		core.RemovalToolUnreadable:       "该生态的命令输出无法解析，暂不移除",
		core.RemovalNotRegistered:        "该目标已不在生态自己的清单里，或位置与扫描时不一致",
	}
	en := map[core.RemovalRefusal]string{
		core.RemovalProtected:            "This path is protected by the core safety rules",
		core.RemovalProtectedEnvironment: "The base environment and the active environment cannot be removed",
		core.RemovalBusy:                 "The target is in use, or its occupancy cannot be determined",
		core.RemovalUnsupportedTool:      "This ecosystem's uninstall command is not verified yet; view only",
		core.RemovalUnverifiedSource:     "This entry came from a directory fallback scan, not the ecosystem itself; view only",
		core.RemovalToolUnavailable:      "The ecosystem's command is unavailable (not installed or not on PATH)",
		core.RemovalToolUnreadable:       "The ecosystem's command output could not be parsed",
		core.RemovalNotRegistered:        "The target is no longer in the ecosystem's own inventory, or its location changed",
	}
	table := en
	if lang == core.Chinese {
		table = zh
	}
	if text, ok := table[reason]; ok {
		return text
	}
	return string(reason)
}

func (r *Root) dropRemovedDevAssets(items []core.DevAssetItem, outcomes []core.RemovalOutcome) {
	gone := map[string]bool{}
	for i, o := range outcomes {
		if o.Status == core.AssetRemoved && i < len(items) {
			gone[items[i].ID] = true
		}
	}
	if len(gone) == 0 {
		return
	}
	r.dev.Assets = slices.DeleteFunc(r.dev.Assets, func(i core.DevAssetItem) bool { return gone[i.ID] })
	for id, packages := range r.dev.Packages {
		r.dev.Packages[id] = slices.DeleteFunc(packages, func(i core.DevAssetItem) bool { return gone[i.ID] })
	}
	if gone[r.dev.ActiveID] {
		r.dev.ActiveID = ""
	}
}

func (r *Root) requestDevRemoval(selected map[string]bool) {
	if r.busy() {
		return
	}
	var items []core.DevAssetItem
	var paths []string
	for _, i := range r.devItems() {
		if selected[i.ID] && core.CanRemoveDevAsset(i) {
			items = append(items, i)
			paths = append(paths, i.Path)
		}
	}
	if len(items) == 0 {
		return
	}
	name := items[0].DisplayName()
	title := r.tr(fmt.Sprintf("移除 %s？", name), fmt.Sprintf("Remove %s?", name))
	if len(items) > 1 {
		title = r.tr(fmt.Sprintf("批量移除选中的 %d 个项？", len(items)), fmt.Sprintf("Remove %d selected items?", len(items)))
	}
	what := devWhat(r, items[0])
	body := r.tr(
		fmt.Sprintf("将调用该生态自己的卸载命令移除%s。这不是清理缓存：移除后需要重新安装才能恢复，且不会回退成直接删除目录。", what),
		fmt.Sprintf("Runs the ecosystem's own uninstall command to remove %s. This is not cache cleanup: reinstalling is the only way back, and it never falls back to deleting files.", what),
	)
	if items[0].Kind.Type == core.DevVirtualEnv && len(items) == 1 {
		body = r.tr(
			fmt.Sprintf("将整目录删除%s。它已通过 pyvenv.cfg 验明是虚拟环境，且不在任何包管理器的登记里，目录本身就是全部状态。删除后可随时用 venv / poetry 重建，但里面的包需要重新安装。", what),
			fmt.Sprintf("Deletes %s together with its whole directory. It is verified as a virtualenv by pyvenv.cfg and registered in no package manager—the directory is its whole state. It can be recreated with venv / poetry, but its packages need reinstalling.", what),
		)
	}
	detail := targetDetails(paths)
	if len(items) > 1 {
		var bytes uint64
		var lines []string
		for i, item := range items {
			bytes += item.Size.ExclusiveReclaimableBytes
			if i < 6 {
				lines = append(lines, "· "+item.DisplayName())
			}
		}
		if len(items) > 6 {
			lines = append(lines, r.tr(fmt.Sprintf("...等共 %d 项", len(items)), fmt.Sprintf("...%d items in total", len(items))))
		}
		body = r.tr("将依次调用各生态的原生卸载命令移除选中的环境或包。这不是清理缓存：移除后需要重新安装，且不会直接删除文件。", "Runs the ecosystem's own uninstall command for each selected environment or package. This is not cache cleanup: reinstalling is required to restore, and it never falls back to direct deletion.")
		detail = r.tr(fmt.Sprintf("预计可释放：%s\n\n待移除清单：\n%s", core.FmtSize(bytes), strings.Join(lines, "\n")), fmt.Sprintf("Estimated reclaimable: %s\n\nTo be removed:\n%s", core.FmtSize(bytes), strings.Join(lines, "\n")))
	} else if len(items) == 1 && items[0].Kind.Type == core.DevPipPackage {
		version := "-"
		if items[0].Kind.Version != nil && *items[0].Kind.Version != "" {
			version = *items[0].Kind.Version
		}
		detail = r.tr(fmt.Sprintf("位置：%s\n版本：%s", items[0].Path, version), fmt.Sprintf("Location: %s\nVersion: %s", items[0].Path, version))
	} else if len(items) == 1 {
		detail = r.tr(fmt.Sprintf("位置：%s\n独占可释放：%s", items[0].Path, core.FmtSize(items[0].Size.ExclusiveReclaimableBytes)), fmt.Sprintf("Location: %s\nExclusive reclaimable: %s", items[0].Path, core.FmtSize(items[0].Size.ExclusiveReclaimableBytes)))
	}
	r.ask(title, body+"\n\n"+detail, func() {
		r.dev.Removing = true
		r.start(true, nil, func(ctx context.Context) func() {
			var outcomes []core.RemovalOutcome
			for _, i := range items {
				if ctx.Err() != nil {
					break
				}
				i.InUse = core.SpotUnknown
				if state, known := core.SpotCheckInUse([]string{i.Path})[i.Path]; known {
					i.InUse = state
				}
				outcomes = append(outcomes, core.RemoveDevAsset(i))
			}
			cancelled := ctx.Err() != nil
			return func() {
				r.dev.Removing = false
				r.dev.Selected = map[string]bool{}
				var ok, failed, skipped uint
				var notes []string
				for i, o := range outcomes {
					label := items[i].DisplayName()
					switch o.Status {
					case core.AssetRemoved:
						ok++
						notes = append(notes, r.tr(fmt.Sprintf("已移除 %s", label), fmt.Sprintf("Removed %s", label)))
					case core.AssetRefused:
						skipped++
						reason := devRefusal(r.settings.Language, o.Refusal)
						notes = append(notes, r.tr(fmt.Sprintf("未移除 %s：%s", label, reason), fmt.Sprintf("Did not remove %s: %s", label, reason)))
					default:
						failed++
						notes = append(notes, r.tr(fmt.Sprintf("移除 %s 未完成，状态未知，请手工确认：%s", label, o.Detail), fmt.Sprintf("Removing %s did not complete; state is unknown, verify manually: %s", label, o.Detail)))
					}
				}
				skipped += uint(len(items) - len(outcomes))
				r.record("remove_dev_assets", paths, ok, skipped, failed, 0)
				if len(items) == 1 && len(notes) == 1 {
					r.dashboard.StatusText = notes[0]
				} else {
					r.dashboard.StatusText = r.tr(
						fmt.Sprintf("批量移除完成：已移除 %d 项，失败 %d 项，跳过 %d 项", ok, failed, skipped),
						fmt.Sprintf("Batch remove finished: %d removed, %d failed, %d skipped", ok, failed, skipped),
					)
				}
				if !cancelled && failed > 0 {
					r.scanDev()
				} else if !cancelled {
					r.dropRemovedDevAssets(items, outcomes)
				}
			}
		})
	})
}
