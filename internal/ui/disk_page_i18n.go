package ui

import (
	"fmt"
	"strings"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

type diskCopy struct {
	heading, subheading, loading, errorPrefix, errorHint, prompt, scan                                                     string
	tree, files, up, collapse, copy, name, size, action                                                                    string
	protected, delete, empty, noFiles, partial, selectView, selectItem                                                     string
	selectedBadge, selectedCount, selectedTitle, clear, confirm, recycle, freed, recycled                                  string
	breakdown, topCategories, total, scanned, free, usedFree, other, otherUsed, folderItems, filesTitle, filesCount, usage string
	kinds                                                                                                                  [5]string
}

func diskStrings(lang Language) diskCopy {
	if lang == Chinese {
		return diskCopy{
			heading: "Disk Lens 磁盘透镜", subheading: "分析磁盘各层级空间占用，定位大文件与冗余目录", loading: "正在深度分析磁盘 %s 空间占用",
			errorPrefix: "磁盘分析失败：", errorHint: "请确保以管理员权限运行，或切换至其他可用盘符重试", prompt: "选择要分析的磁盘并开始深度扫描", scan: "开始分析 %s 空间占用",
			tree: "目录树", files: "全盘大文件", up: "← 上级", collapse: "‹ 收起", copy: "复制路径", name: "名称", size: "大小", action: "操作",
			protected: "系统保护项目", delete: "删除", empty: "此目录为空", noFiles: "未找到大文件", partial: "部分索引", selectView: "选择当前视图", selectItem: "选择 %s",
			selectedBadge: "已选 %d 项 (%s)", selectedCount: "%d 项已选中", selectedTitle: "已选择要清理的磁盘项目", clear: "清空选择", confirm: "确认永久删除", recycle: "删除到回收站", freed: "待彻底释放", recycled: "移入回收站（暂不释放空间）",
			breakdown: "空间占比分布", topCategories: "Top %d 分类", total: "%s 总容量", scanned: "%s 已扫描", free: "空闲可用空间", usedFree: "已用 %.0f%% · 空闲 %s", other: "其他 %d 项", otherUsed: "其他已用 %d 项", folderItems: "当前目录共 %d 个子项", filesTitle: "全盘大文件分布", filesCount: "前 %d 个大文件汇总", usage: "已用 %s / 共 %s",
			kinds: [5]string{"媒体视频 (Media)", "程序/动态库 (Bin)", "压缩镜像 (Archive)", "文档数据 (Data)", "其他文件 (Others)"},
		}
	}
	return diskCopy{
		heading: "Disk Lens Analyzer", subheading: "Analyze disk usage by hierarchy and locate large files", loading: "Analyzing storage for drive %s…",
		errorPrefix: "Disk analysis failed: ", errorHint: "Please ensure running as administrator or switch to another drive", prompt: "Select a drive to analyze storage hierarchy", scan: "Analyze Storage for %s",
		tree: "Directory Tree", files: "Large Files", up: "← Up", collapse: "‹ Collapse", copy: "Copy path", name: "Name", size: "Size", action: "Action",
		protected: "System Protected", delete: "Delete", empty: "Directory is empty", noFiles: "No large files found", partial: "Partial index", selectView: "Select current view", selectItem: "Select %s",
		selectedBadge: "%d items (%s)", selectedCount: "%d items selected", selectedTitle: "Selected Disk Items", clear: "Clear Selection", confirm: "Confirm Permanent Delete", recycle: "Delete to Recycle Bin", freed: "To be freed", recycled: "Moved to Recycle Bin (space not freed yet)",
		breakdown: "Space breakdown", topCategories: "Top %d categories", total: "%s Total", scanned: "%s Scanned", free: "Free Space", usedFree: "Used %.0f%% · Free %s", other: "Other %d items", otherUsed: "Other %d items", folderItems: "%d items in folder", filesTitle: "Largest Files Breakdown", filesCount: "Top %d large files", usage: "Used %s / Total %s",
		kinds: [5]string{"Media & Video", "Binaries & DLLs", "Archives & Images", "Documents & Data", "Other Files"},
	}
}

func diskChoose(lang Language, zh, en string) string {
	if lang == Chinese {
		return zh
	}
	return en
}

func diskLoadingSub(lang Language, windows bool) string {
	if lang == Chinese {
		return "快速索引全盘文件结构与体积分布，请稍候"
	}
	if windows {
		return "Indexing NTFS file hierarchy and sizes, please wait"
	}
	return "Scanning file hierarchy and sizes, please wait"
}

func (s DiskPage) volumeLabel(v core.VolumeID) string {
	raw := v.Display()
	if s.Windows {
		return diskChoose(s.Language, raw+" 盘", "Drive "+raw)
	}
	if raw == "/" {
		return diskChoose(s.Language, "系统盘 (/)", "System (/)")
	}
	return strings.TrimPrefix(raw, "/Volumes/")
}

func (s DiskPage) volumeUsage(space *DiskPageSpace) string {
	if space == nil {
		return ""
	}
	used := uint64(0)
	if space.Total > space.Free {
		used = space.Total - space.Free
	}
	return fmt.Sprintf(diskStrings(s.Language).usage, core.FmtSize(used), core.FmtSize(space.Total))
}
