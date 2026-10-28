package ui

// Copy follows src/ui/i18n/dev_env.rs.
type devEnvCopy struct {
	title, description, rescan, empty, search, base, active, displayOnly     string
	sections                                                                 [3]string
	filters                                                                  [4]string
	metrics                                                                  [3]string
	exclusive, size, changed, unknown, remove, packages, loading, noPackages string
	noSelection, path, executables, all, invert, clear, siteAll, matched     string
	selected, selection, zeroSelection, batchRemove, selectionLabel          string
}

func devEnvStrings(l Language) devEnvCopy {
	if l == English {
		return devEnvCopy{
			title: "Dev Environments & Packages", description: "Inspect installed Conda environments, Node global packages, and Python CLI tools. Removal always runs the ecosystem's own command, never a file delete",
			rescan: "Rescan", empty: "No environments or global packages detected", search: "Search envs or packages...", base: "Base Protected", active: "Active", displayOnly: "Display only",
			sections: [3]string{"Python & Conda Environments", "Node Global Packages", "Python CLI Tools"}, filters: [4]string{"All", "Python", "Node", "Tools"},
			metrics: [3]string{"Total Assets", "Reclaimable (removable)", "Total Logical Size"}, exclusive: "Exclusive", size: "Size", changed: "Last changed", unknown: "no record",
			remove: "Remove", packages: "%d packages", loading: "Loading packages…", noPackages: "No identifiable installed packages", noSelection: "Select an environment or tool on the left to view details",
			path: "Install Path", executables: "Associated Executables", all: "All", invert: "Invert", clear: "Clear", siteAll: "Select all", matched: "%d of %d packages",
			selected: "Selected for removal", selection: "%d items · %s", zeroSelection: "0 items", batchRemove: "Remove (%d)", selectionLabel: "Select %s",
		}
	}
	return devEnvCopy{
		title: "开发环境与包管理", description: "审查已安装的 Conda 虚拟环境、Node 全局包与 Python 独立 CLI 工具。移除一律交给各生态自己的命令执行，不做文件删除",
		rescan: "重新检测", empty: "未检测到相关环境或全局包", search: "搜索环境或包名...", base: "Base 基础环境", active: "当前激活", displayOnly: "只展示",
		sections: [3]string{"Python 与 Conda 环境", "Node / JS 全局包", "Python 独立 CLI 工具"}, filters: [4]string{"全部", "Python", "Node", "工具"},
		metrics: [3]string{"环境与工具总数", "可移除项可释放", "逻辑总规模"}, exclusive: "独占可释放", size: "占用大小", changed: "最后变更", unknown: "无记录",
		remove: "移除", packages: "%d 个包", loading: "正在读取包列表…", noPackages: "没有可识别的已安装包", noSelection: "在左侧选择环境或工具以查看详情",
		path: "安装路径", executables: "关联可执行程序", all: "全选", invert: "反选", clear: "清空", siteAll: "全选本环境", matched: "匹配 %d / 共 %d 个包",
		selected: "已选择移除", selection: "%d 项 · %s", zeroSelection: "0 项", batchRemove: "批量移除 (%d)", selectionLabel: "选择 %s",
	}
}
