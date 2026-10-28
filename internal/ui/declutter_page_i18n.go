package ui

// Copy follows src/ui/i18n/declutter.rs, including its historical kind labels.
var declutterCopy = map[string][2]string{
	"pendingShort":   {"待扫描", "Pending"},
	"selectionLabel": {"选择 %s", "Select %s"},
	"tab0":           {"概览", "Overview"}, "tab1": {"相似图片", "Similar Photos"}, "tab2": {"重复文件", "Duplicates"}, "tab3": {"大型与旧文件", "Large & Old Files"}, "tab4": {"下载项", "Downloads"},
	"back": {"返回概览", "Back to Overview"}, "cancel": {"取消全选", "Cancel"}, "remove": {"清理所选项 ›", "Remove Selected ›"}, "selected": {"已选 %d 个项目 • 共 %s", "%d items selected • %s total"},
	"confirmTitle": {"确认移入废纸篓", "Confirm move to Trash"}, "confirmBody": {"将把 %d 项移入废纸篓。", "%d items will be moved to the Trash."},
	"confirmDetail": {"移入废纸篓不释放磁盘空间；需要真正释放时再清空废纸篓。移走的条目可从废纸篓恢复。", "Moving to the Trash does not free disk space; empty the Trash to reclaim it. Moved items can be restored from the Trash."},
	"heading0":      {"磁盘冗余整理与瘦身", "Declutter Your Drive"}, "sub0": {"我们识别到了占用存储空间的非必要文件。查看下方维度，只需轻点几下即可重获充裕的磁盘性能。", "We've identified unnecessary files hoarding your storage space. Review the categories below and reclaim your disk performance with a single click."},
	"scan": {"开启全盘冗余扫描", "Start Smart Scan"}, "scanBusy": {"正在智能分析扫描...", "Scanning Drive..."}, "scanning": {"扫描中...", "Scanning..."}, "elapsed": {"扫描耗时 %.1fs", "Scan took %.1fs"}, "eta": {"预计耗时: ~1-2 秒 (索引加速)", "Estimated time: ~1-2 secs (Indexed)"}, "savings": {"可优化空间:", "Potential Savings:"}, "potential": {"可释放空间", "POTENTIAL SAVINGS"}, "analyzed": {"● 已分析", "● Analyzed"}, "pending": {"● 待扫描", "● Pending"},
	"card4": {"下载项文件夹", "Downloads Folder"}, "cardSub4": {"历史安装包、归档与缓存", "Old installers and archives"}, "files": {"个文件", "Files"}, "cardSub3": {"超过 100MB 且半年未访问", "Over 100MB untouched in 6 mos"}, "total": {"总计 ~%s", "~%s Total"}, "cardSub2": {"冗余副本数据", "REDUNDANT DATA"}, "sets": {"组完全相同副本", "Sets found across folders"}, "select": {"智能挑选 ›", "Select ›"}, "review": {"查看 ›", "Review ›"}, "cardSub1": {"视觉冗余整理", "VISUAL CLUTTER"}, "photoGroups": {"相似/连拍照片组", "Estimated groups"},
	"heading1": {"相似图片整理", "Similar Photos"}, "sub1": {"审查视觉上高度相似或连拍的图片组。系统已自动为您标出每组最佳品质的照片，只需一键清理冗余版本。", "Review grouped images that appear visually identical or highly similar. We've highlighted the highest quality version in each group."}, "kicker": {"◧ 冗余整理", "◧ DECLUTTER MODULE"},
	"heading2": {"重复文件", "Duplicate Files"}, "sub2": {"", ""}, "heading3": {"审查大型文件", "Review Items"}, "sub3": {"选择不再需要的文件，安全清理以释放宝贵的磁盘空间。", "Select files you no longer need. Safely remove them to free up disk space."}, "heading4": {"审查下载项", "Review Downloads"}, "sub4": {"清理 Downloads 目录中残留的历史安装包、压缩归档与临时文件。", "Clean old DMG installers, archives and temp files from ~/Downloads."},
	"empty1": {"暂未发现相似或连拍冗余照片", "No similar or burst photos found"}, "emptySub1": {"您的相册非常整洁，未发现连拍或高重复度照片。", "Your photo library is clean without redundant bursts."}, "empty2": {"未发现重复文件", "No duplicate files found"}, "emptySub2": {"您的磁盘非常整洁，未发现占用空间的完全相同文件副本。", "Your drive is clean with no identical duplicate files."}, "empty3": {"未发现符合条件的大型文件", "No large files found"}, "emptySub3": {"未找到大于指定筛选体积的文件，您可以尝试清除筛选条件。", "No files exceed the size filter. Try clearing the filter."}, "empty4": {"下载文件夹暂无可清理项", "No downloads found"}, "emptySub4": {"您的 Downloads 文件夹中没有可识别的历史安装包或残留归档。", "No installer packages or archives found in Downloads."},
	"name": {"文件名", "FILE NAME"}, "size": {"大小", "SIZE"}, "kind": {"类型", "KIND"}, "downloaded": {"下载时间", "DOWNLOADED"}, "accessed": {"最后访问", "LAST ACCESSED"}, "count": {"%d 项", "%d items"}, "found": {"筛选总计", "TOTAL FOUND"}, "clearFilters": {"清除筛选", "Clear Filters"},
	"sizeAll": {"大小: 全部 ▾", "Size: All ▾"}, "sizeFilter": {"大小: > %s ✕", "Size: > %s ✕"}, "kindAll": {"类型: 全部 ▾", "Kind: All Types ▾"}, "kind0": {"类型: 视频 ✕", "Kind: Video ✕"}, "kind1": {"类型: 压缩包 ✕", "Kind: Archive ✕"}, "kind2": {"类型: 文件夹 ✕", "Kind: Folder ✕"}, "kind3": {"类型: 图片 ✕", "Kind: Image ✕"},
	"reveal": {"定位", "Reveal"}, "open": {"打开", "Open"}, "preview": {"看图", "Preview"}, "ctxMac": {"在访达中定位显示", "Reveal in Finder"}, "ctxWindows": {"在文件资源管理器中定位", "Reveal in File Explorer"}, "ctxReveal": {"在文件管理器中定位", "Show in File Manager"}, "ctxOpen": {"使用系统程序打开", "Open with System App"}, "ctxCopy": {"复制完整文件路径", "Copy Full Path"},
	"original": {"原件", "ORIGINAL"}, "duplicate": {"副本", "DUPLICATE"}, "newest": {"保留最新副本", "Keep Newest"}, "oldest": {"保留最旧副本", "Keep Oldest"}, "dupSub": {"%s · %d 份相同副本", "%s · %d identical copies"}, "modified": {"修改时间: %s", "Modified: %s"}, "dupSummary": {"共发现 %d 组完全相同的重复副本占用磁盘空间。", "Found %d sets of identical duplicate files hoarding storage."}, "dupCap": {"（按占用空间排序，当前显示前 %d 组）", " (largest first, showing %d)"},
	"showMore": {"显示更多（还剩 %d 项）", "Show more (%d remaining)"}, "showMoreGroups": {"显示更多（还剩 %d 组）", "Show more (%d groups remaining)"},
	"best": {"最佳品质 (已保留)", "Best Quality (Kept)"}, "toClean": {"待清理", "To Clean"}, "kept": {"已保留", "Kept"}, "keepBest": {"★ 仅保留最佳", "★ Keep Best Only"}, "keepAll": {"全部保留", "Keep All"}, "smart": {"✨ 自动挑选最佳", "✨ Smart Select All"}, "collapse": {"▲ 收起", "▲ Collapse"}, "more": {"+ 查看其余 %d 张...", "+ %d more..."}, "redundant": {"待清理相似副本 (共 %d 张)：", "Redundant Copies (%d):"}, "groupSelected": {"• 共 %d 张照片 (待清理 %d 张 · 可释放 %s)", "• %d photos (%d to clean · %s)"}, "groupKept": {"• 共 %d 张照片 (全部已保留)", "• %d photos (all kept)"},
}

func (s DeclutterPage) tr(key string) string {
	i := 0
	if s.Language == English {
		i = 1
	}
	return declutterCopy[key][i]
}
