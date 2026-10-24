package ui

import (
	"fmt"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

type Language = core.Language

const (
	Chinese = core.Chinese
	English = core.English
)

type dashboardCopy struct {
	subtitle, freed, language, ready, standard, admin, fdaGranted, fdaLimited string
	nav                                                                       [7]string
	rescan, scanning, cleaning, found, clean, noJunk, start                   string
	scanningSub, cleaningSub, scanningBlurb, cleanBlurb, idleBlurb, review    string
	quick                                                                     [4]string
	appsFound, foundBlurb, freedPill                                          string
	statusHardware, statusProcesses, statusLive, statusPaused                 string
}

// Copy is ported verbatim from src/ui/i18n/{mod,views}.rs.
var dashboardChinese = dashboardCopy{
	subtitle: "极速磁盘与软件清理", freed: "本次已释放空间", language: "界面语言", ready: "就绪",
	standard: "普通模式", admin: "管理员模式", fdaGranted: "完全磁盘访问", fdaLimited: "磁盘受限 (点击配置)",
	nav:    [7]string{"概览扫描", "状态监控", "智能清理", "软件管理", "磁盘透镜", "冗余整理", "文件搜索"},
	rescan: "重新扫描", scanning: "扫描中…", cleaning: "清理中…", found: "发现可清理内容",
	clean: "系统很干净", noJunk: "暂无可清理垃圾", start: "点击开始一键智能扫描",
	scanningSub: "正在检查系统缓存与开发环境残留", cleaningSub: "正在彻底移除选中垃圾",
	scanningBlurb: "正在扫描系统缓存、应用日志、开发依赖产物与临时文件…",
	cleanBlurb:    "未发现可清理的冗余缓存，系统状态良好。", idleBlurb: "快速扫描系统缓存、开发依赖产物与磁盘占用。",
	review: "查看详情并清理", quick: [4]string{"一键清理", "卸载分析", "空间透镜", "重复/大文件"},
	appsFound: "已发现 %d 款", foundBlurb: "已在 %d 个类别中发现 %s 可清理内容。", freedPill: "本次已释放 %s",
	statusHardware: "硬件概览", statusProcesses: "进程管理", statusLive: "实时监测", statusPaused: "已暂停",
}

var dashboardEnglish = dashboardCopy{
	subtitle: "Fast Disk & App Cleaner", freed: "Space Freed", language: "Language", ready: "Ready",
	standard: "Standard user", admin: "Administrator", fdaGranted: "Full Disk Access", fdaLimited: "Limited Access (Configure)",
	nav:    [7]string{"Overview", "Status", "Smart Clean", "Apps", "Disk Lens", "Declutter", "File Search"},
	rescan: "Rescan", scanning: "Scanning…", cleaning: "Cleaning…", found: "Cleanable Found",
	clean: "System is Clean", noJunk: "No Junk Found", start: "Click to Start Smart Scan",
	scanningSub: "Checking system caches and dev leftovers", cleaningSub: "Permanently removing selected items",
	scanningBlurb: "Scanning system caches, application logs, build artifacts, and temp files…",
	cleanBlurb:    "No redundant caches found. System is running cleanly.", idleBlurb: "Quickly scan system caches, developer dependencies, and disk usage.",
	review: "Review & Clean Junk", quick: [4]string{"Clean Junk", "Uninstall", "Storage", "Duplicates/Photos"},
	appsFound: "%d Apps", foundBlurb: "Found %s cleanable items across %d categories.", freedPill: "%s freed",
	statusHardware: "Hardware", statusProcesses: "Processes", statusLive: "Live", statusPaused: "Paused",
}

func dashboardStrings(l Language) dashboardCopy {
	if l == English {
		return dashboardEnglish
	}
	return dashboardChinese
}

func dashboardFoundBlurb(l Language, categories int, size string) string {
	if l == English {
		return fmt.Sprintf(dashboardStrings(l).foundBlurb, size, categories)
	}
	return fmt.Sprintf(dashboardStrings(l).foundBlurb, categories, size)
}
