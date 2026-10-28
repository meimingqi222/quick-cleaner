package ui

// Copy follows src/ui/i18n/{mod,views,dev_env}.rs.
type appsPageCopy struct {
	heading, subheading, desktop, dev, leftovers, presets, search                         string
	stats                                                                                 [3]string
	columns                                                                               [6]string
	count, filterAll, filterMatch, name, footer, footerMatch, size                        string
	loading, loadingSub, empty, uninstall, force, remove, discovered, official            string
	open, copy, standard, forceMenu, removeMenu                                           string
	recommended, all, clear, cancel, done, clean, residualTitle, residualSub              string
	phases, stages                                                                        [3]string
	keepOpen, manual, dataWarning                                                         string
	orphanTitle, orphanSub, residualEmpty, extensionTitle, extensionAdvice, extensionOpen string
	occupiedTitle, occupiedAdvice, registeredTitle, registeredAdvice, launchdGroup        string
	occupancyMore, listSep                                                                string
}

func appsPageStrings(l Language) appsPageCopy {
	if l == English {
		return appsPageCopy{
			heading: "Applications Manager", subheading: "Analyze disk usage and thoroughly clean residual files", desktop: "Desktop Apps", dev: "Dev Environments", leftovers: "App Leftovers", presets: "Presets:", search: "Search apps by name or publisher…",
			stats: [3]string{"Total Estimated Size", "Installed Applications", "Rarely Used (>90 days)"}, columns: [6]string{"Name", "Publisher", "Last Used", "Installed Date", "Size", "Actions"},
			count: "%d Apps", filterAll: "%d apps", filterMatch: "Matched %d of %d apps", name: "Name & Version (%d apps)", footer: "Displaying %d apps (Total %d installed)", footerMatch: "Matched %d apps (Total %d)", size: "Total Size: %s",
			loading: "Scanning installed applications…", loadingSub: "Calculating installed size, directories and last usage date", empty: "No matching applications found", uninstall: "Uninstall", force: "Force Clean", remove: "Remove files", discovered: "Unregistered · No official uninstaller", official: "Unregistered · Uninstall and clean",
			open: "Open Install Location", copy: "Copy Install Path", standard: "Standard Uninstall", forceMenu: "Force Residual Clean", removeMenu: "Remove program files and shortcuts…",
			recommended: "Recommended", all: "Select All", clear: "Clear Selection", cancel: "Cancel", done: "Done", clean: "Clean Selected (%d) · Free %s", residualTitle: "Found %d residual items for \"%s\"", residualSub: "Includes caches, app configuration and registry traces. Potential space: %s",
			phases: [3]string{"Analyzing the app and associated files", "Removing the application", "Verifying the uninstall result"}, stages: [3]string{"Analyze associated files", "Remove application", "Verify remaining files"}, keepOpen: "Keep QuickCleaner open until the uninstall completes",
			manual:      "Remove \"%s\" using the vendor's uninstaller, in-app action or required Finder procedure, not just main-app deletion. Back up data, finish service/extension removal and any required restart, then refresh the list. For input methods, remove system input sources first, use the official uninstall action, and remove companion settings apps as instructed to prevent reinstallation.",
			dataWarning: "The official uninstall action deletes user data. Back up anything you need to retain, such as dictionaries, learning records and settings, including data in custom directories; leftover selection cannot prevent this deletion.",
			orphanTitle: "Found %d leftover candidates", orphanSub: "Checked installed apps and system registrations. Certain items have app identity evidence; Possible items match names only. Select items manually. Potential space: %s", residualEmpty: "No residual files or registry traces found",
			extensionTitle: "Contains a system extension — turn it off in System Settings", extensionAdvice: "Third-party apps can't deactivate another team's extension — turn it off in System Settings → General → Login Items & Extensions → Driver Extensions ⓘ, reboot, then rescan", extensionOpen: "Open Login Items & Extensions",
			occupiedTitle: "Background processes are still running — database residue will fail to delete", occupiedAdvice: "Quit the app completely (menu bar and background agents included), then clean again",
			registeredTitle: "Launch-at-login items are still registered (not currently running) — residue may return at next login", registeredAdvice: "Quitting the app won't remove a login item — remove it in System Settings → General → Login Items, then clean again",
			launchdGroup: "Still registered with launchd (may start at login)", occupancyMore: "…and %d more", listSep: ", ",
		}
	}
	return appsPageCopy{
		heading: "软件全生命周期管理", subheading: "分析软件占用并深度清理卸载残留", desktop: "桌面应用", dev: "开发环境", leftovers: "已卸载残留", presets: "快速分类:", search: "搜索软件名称或发布者…",
		stats: [3]string{"估算总占用空间", "已安装应用总数", "长期未用软件 (>90天)"}, columns: [6]string{"软件名称", "发布者", "最近使用", "安装日期", "预估占用", "操作"},
		count: "%d 款", filterAll: "共 %d 款", filterMatch: "匹配 %d / %d 款", name: "应用名称与版本 (共 %d 款)", footer: "当前列表展示 %d 款软件（总计 %d 款已装）", footerMatch: "搜索匹配 %d 款软件（总计 %d 款）", size: "列表总占用: %s",
		loading: "正在智能检索已安装软件与空间占用", loadingSub: "深度测算软件安装目录、真实体积与最后使用时间", empty: "未找到匹配的已安装软件", uninstall: "卸载", force: "强力清理", remove: "移除文件", discovered: "未登记 · 无官方卸载入口", official: "未登记 · 卸载与清理",
		open: "打开安装目录", copy: "复制安装路径", standard: "官方常规卸载", forceMenu: "强力残留清理", removeMenu: "移除程序文件与快捷方式…",
		recommended: "推荐选中", all: "全选", clear: "清空选择", cancel: "取消", done: "完成", clean: "彻底清除所选 (%d) · 释放 %s", residualTitle: "发现「%[2]s」的 %[1]d 项关联残留", residualSub: "包括应用缓存、用户配置数据及注册表孤儿项，预计释放 %s",
		phases: [3]string{"正在分析应用及关联文件", "正在移除应用程序", "正在确认卸载结果"}, stages: [3]string{"分析关联文件", "移除应用程序", "复核残留项目"}, keepOpen: "卸载完成前请保持 QuickCleaner 运行",
		manual:      "「%s」需要按官方文档使用卸载工具、软件内入口或指定的 Finder 流程；不能仅删除主应用。请先备份数据，完成服务/扩展移除及必要重启后刷新列表。输入法请先移除系统输入源，再使用官方卸载入口，并按官方说明移除配套设置应用，避免再次安装。",
		dataWarning: "注意：此软件的官方卸载逻辑会直接删除用户数据。请先备份需要保留的数据（如词库、学习记录和设置，包括自定义目录中的数据）；后续残留勾选不能阻止这一步。",
		orphanTitle: "发现 %d 项待核对残留", orphanSub: "已核对应用清单与系统登记。“确定”有应用身份佐证；“可能”仅匹配目录名称。均需手动勾选，预计释放 %s", residualEmpty: "该软件未发现关联的文件或注册表残留",
		extensionTitle: "含系统扩展，需在系统设置中手动关闭", extensionAdvice: "第三方程序无权注销他人的系统扩展：到「系统设置 → 通用 → 登录项与扩展 → 驱动程序扩展 ⓘ」里关掉，重启后再回来重新扫描", extensionOpen: "打开登录项与扩展",
		occupiedTitle: "该应用的后台仍在运行，数据库类残留会删除失败", occupiedAdvice: "请先彻底退出该应用（含菜单栏常驻与后台代理），再重新清理",
		registeredTitle: "该应用仍登记着开机自启项（当前未在运行），残留清理后可能随登录回归", registeredAdvice: "退出应用不会移除开机自启项：到「系统设置 → 通用 → 登录项」里把它移除，再重新清理",
		launchdGroup: "launchd 仍有登记（可能开机自启）", occupancyMore: "…以及另外 %d 条", listSep: "、",
	}
}
