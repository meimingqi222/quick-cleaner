package ui

import "github.com/meimingqi222/quick-cleaner/internal/core"

// Product copy is transcribed from src/ui/i18n/{categories,mod,views}.rs.
type junkCopy struct {
	headingSub, found, exclusions, exclusionsHint, exclusionsEmpty, remove string
	loadingTitle, loadingSub, empty, manual, partial, discovering          string
	cleanTitle, cleanSub                                                   string
	selected, items, selectedItems, clean, exclude, release, files         string
	failed, showDetails, hideDetails                                       string
	batch                                                                  [4]string
}

func junkStrings(l Language) junkCopy {
	if l == English {
		return junkCopy{
			headingSub: "Rebuildable app, browser, package-manager and update-package caches are selected; temp data and dev builds require manual selection",
			found:      "Cleanable Found", exclusions: "Exclusions (%d)", exclusionsHint: "These paths are never touched by any cleanup",
			exclusionsEmpty: "No exclusions yet. Click \"Exclude\" on an item below to add one", remove: "Remove",
			loadingTitle: "Scanning system junk files…", loadingSub: "Safely checking system temp, caches, logs and Recycle Bin",
			empty: "Nothing cleanable found in this category", manual: "Manual Select", partial: "Partial", discovering: "Scanning…",
			cleanTitle: "Your system is clean", cleanSub: "Nothing cleanable was found. Scan again later to check.",
			selected: "Selected for Cleaning", items: "%d items", selectedItems: "(%d items)", clean: "Clean Now", exclude: "Exclude", release: "Quit apps and clean", files: "%s files",
			failed: "%d targets still have leftovers after last clean", showDetails: "Show details ▾", hideDetails: "Hide details ▴",
			batch: [4]string{"Recommended", "Select All", "Invert", "Clear All"},
		}
	}
	return junkCopy{
		headingSub: "可重建的应用、浏览器、包管理与应用更新包缓存默认已勾选；临时数据与开发产物需手动勾选",
		found:      "发现可清理内容", exclusions: "排除清单 (%d)", exclusionsHint: "这些路径不会被任何清理操作触碰",
		exclusionsEmpty: "还没有排除项。在下方列表里点击条目的「排除」即可加入", remove: "移除",
		loadingTitle: "正在全面扫描系统冗余垃圾", loadingSub: "安全检索系统临时缓存、应用日志与回收站，准备释放空间",
		empty: "此类别未发现可清理内容", manual: "需手动勾选", partial: "部分统计", discovering: "检索中…",
		cleanTitle: "系统很干净", cleanSub: "没有发现可清理的内容，可以稍后再扫描一次。",
		selected: "已选择清理", items: "共 %d 项", selectedItems: "(%d 项)", clean: "立即清理", exclude: "排除", release: "退出应用后清理", files: "%s 个文件",
		failed: "上次清理有 %d 个目标仍有残留", showDetails: "查看详情 ▾", hideDetails: "收起详情 ▴",
		batch: [4]string{"推荐选中", "全选", "反选", "清空选中"},
	}
}

var junkCategories = map[core.CategoryID][2]core.Text{
	core.SystemTemp:         {core.NewText("系统临时文件", "System Temp Files"), core.NewText("系统临时文件与系统更新残留", "System temporary files and update leftovers")},
	core.UserTemp:           {core.NewText("用户临时文件", "User Temp Files"), core.NewText("用户主目录下的应用临时文件", "Application temporary files under user profile")},
	core.UserCache:          {core.NewText("应用缓存", "Application Cache"), core.NewText("应用明确存放在缓存目录中的可重建数据", "Rebuildable data stored in application cache directories")},
	core.BrowserCache:       {core.NewText("浏览器缓存", "Browser Cache"), core.NewText("Chrome / Edge / Safari 等浏览器的缓存数据", "Cache files from Chrome, Edge, Firefox, Safari")},
	core.PackageCache:       {core.NewText("包管理缓存", "Package Manager Cache"), core.NewText("npm / pnpm / cargo / go 等包管理器缓存", "Caches from npm, pnpm, Cargo, Go, pip, etc.")},
	core.UpdaterPackages:    {core.NewText("应用更新包", "Application Update Packages"), core.NewText("已下载的更新包与更新器暂存，删了最多多下一次更新，不丢任何数据", "Downloaded update packages and updater staging; costs a re-download, loses no data")},
	core.Logs:               {core.NewText("日志与崩溃转储", "Logs & Crash Dumps"), core.NewText("系统与应用日志、崩溃转储", "System and application event logs and crash dumps")},
	core.RecycleBinCategory: {core.NewText("回收站 / 废纸篓", "Recycle Bin / Trash"), core.NewText("回收站/废纸篓中已删除的文件", "Deleted files in Recycle Bin or Trash")},
	core.Thumbnails:         {core.NewText("缩略图缓存", "Thumbnail Cache"), core.NewText("系统缩略图缓存，可安全重建", "System thumbnail cache, safe to rebuild")},
	core.BrokenLoginItems:   {core.NewText("损坏的登录项", "Broken Login Items"), core.NewText("引用目标已不存在或配置已失效的启动项", "Startup entries whose executable is missing or configuration is invalid")},
	core.AiAgents:           {core.NewText("AI 编程助手缓存", "AI Assistant Cache"), core.NewText("Claude Code / Codex / Trae / Cursor 等 AI 编程工具的会话记录与缓存", "Session records and caches from Claude, Cursor, Trae, etc.")},
	core.DevBuild:           {core.NewText("项目构建产物与依赖", "Build Artifacts & Deps"), core.NewText("代码目录下的 node_modules / target / .venv / bin·obj 等，可重新构建", "node_modules, target, .venv, bin/obj in projects, rebuildable")},
	core.DevWorktrees:       {core.NewText("AI agent 临时 worktree", "AI Agent Git Worktrees"), core.NewText("AI agent 留下的临时 git worktree，可能含未提交改动", "Temporary worktrees created by AI agents, may contain uncommitted edits")},
	core.LocalSnapshots:     {core.NewText("APFS 本地快照", "APFS Local Snapshots"), core.NewText("APFS 本地快照，macOS「磁盘莫名爆满」的头号原因", "APFS local snapshots, the #1 cause of mysterious disk-full on macOS")},
	core.IosBackup:          {core.NewText("iOS 设备备份", "iOS Device Backup"), core.NewText("iTunes / Finder 创建的 iOS 设备完整备份，单个可达 100 GB+", "Full iOS device backups created by iTunes / Finder, can be 100 GB+ each")},
	core.OldIdeData:         {core.NewText("旧版 IDE 数据", "Old IDE Version Data"), core.NewText("Application Support 下 JetBrains 的按版本数据目录，当前版本之外的旧目录，移入废纸篓", "Per-version JetBrains data dirs under Application Support, excluding the newest, moved to Trash")},
	core.DockerImages:       {core.NewText("冗余 Docker 镜像", "Redundant Docker Images"), core.NewText("悬空镜像、未被任何容器使用的镜像与同仓库旧版本标签，经 docker image rm 释放", "Dangling images, tags unused by any container and old versions, freed via docker image rm")},
}
