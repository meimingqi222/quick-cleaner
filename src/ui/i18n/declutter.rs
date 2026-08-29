//! 冗余整理（Declutter）视图的文案。
//!
//! 命名沿用 `tr_<域>_<物>`：跨标签页复用的直接叫 `tr_declutter_xxx`，
//! 只属于某个标签页的带上标签页名（`tr_declutter_photos_xxx`）。

use crate::core::i18n::Language;

// ------------------------------------------------------------ 跨标签页共用

pub fn tr_declutter_back_to_overview(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "返回概览",
        Language::En => "Back to Overview",
    }
}

pub fn tr_declutter_cancel_selection(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "取消全选",
        Language::En => "Cancel",
    }
}

pub fn tr_declutter_remove_selected(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "清理所选项 ›",
        Language::En => "Remove Selected ›",
    }
}

pub fn tr_declutter_selected_summary(lang: Language, count: usize, size: &str) -> String {
    match lang {
        Language::Zh => format!("已选 {count} 个项目 • 共 {size}"),
        Language::En => format!("{count} items selected • {size} total"),
    }
}

pub fn tr_declutter_reveal(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "定位",
        Language::En => "Reveal",
    }
}

pub fn tr_declutter_open(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "打开",
        Language::En => "Open",
    }
}

pub fn tr_declutter_review(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "查看 ›",
        Language::En => "Review ›",
    }
}

pub fn tr_declutter_pending(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "待扫描",
        Language::En => "Pending",
    }
}

pub fn tr_declutter_scanning(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "扫描中...",
        Language::En => "Scanning...",
    }
}

pub fn tr_declutter_scan_elapsed(lang: Language, secs: f64) -> String {
    match lang {
        Language::Zh => format!("扫描耗时 {secs:.1}s"),
        Language::En => format!("Scan took {secs:.1}s"),
    }
}

pub fn tr_declutter_modified_at(lang: Language, at: &str) -> String {
    match lang {
        Language::Zh => format!("修改时间: {at}"),
        Language::En => format!("Modified: {at}"),
    }
}

// 列头

pub fn tr_declutter_col_name(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "文件名",
        Language::En => "FILE NAME",
    }
}

pub fn tr_declutter_col_size(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "大小",
        Language::En => "SIZE",
    }
}

pub fn tr_declutter_col_kind(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "类型",
        Language::En => "KIND",
    }
}

pub fn tr_declutter_col_last_accessed(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "最后访问",
        Language::En => "LAST ACCESSED",
    }
}

pub fn tr_declutter_col_downloaded(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "下载时间",
        Language::En => "DOWNLOADED",
    }
}

// -------------------------------------------------------------------- 概览

pub fn tr_declutter_overview_heading(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "磁盘冗余整理与瘦身",
        Language::En => "Declutter Your Drive",
    }
}

pub fn tr_declutter_overview_subheading(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "我们识别到了占用存储空间的非必要文件。查看下方维度，只需轻点几下即可重获充裕的磁盘性能。",
        Language::En => "We've identified unnecessary files hoarding your storage space. Review the categories below and reclaim your disk performance with a single click.",
    }
}

pub fn tr_declutter_overview_scanning_btn(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "正在智能分析扫描...",
        Language::En => "Scanning Drive...",
    }
}

pub fn tr_declutter_overview_scan_btn(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "开启全盘冗余扫描",
        Language::En => "Start Smart Scan",
    }
}

pub fn tr_declutter_overview_scan_eta(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "预计耗时: ~1-2 秒 (索引加速)",
        Language::En => "Estimated time: ~1-2 secs (Indexed)",
    }
}

pub fn tr_declutter_overview_savings_label(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "可优化空间:",
        Language::En => "Potential Savings:",
    }
}

pub fn tr_declutter_overview_potential_savings(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "可释放空间",
        Language::En => "POTENTIAL SAVINGS",
    }
}

pub fn tr_declutter_overview_downloads_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "下载项文件夹",
        Language::En => "Downloads Folder",
    }
}

pub fn tr_declutter_overview_downloads_sub(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "历史安装包、归档与缓存",
        Language::En => "Old installers and archives",
    }
}

pub fn tr_declutter_overview_files_unit(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "个文件",
        Language::En => "Files",
    }
}

pub fn tr_declutter_overview_large_files_sub(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "超过 100MB 且半年未访问",
        Language::En => "Over 100MB untouched in 6 mos",
    }
}

pub fn tr_declutter_overview_large_files_total(lang: Language, size: &str) -> String {
    match lang {
        Language::Zh => format!("总计 ~{size}"),
        Language::En => format!("~{size} Total"),
    }
}

pub fn tr_declutter_overview_duplicates_kicker(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "冗余副本数据",
        Language::En => "REDUNDANT DATA",
    }
}

pub fn tr_declutter_overview_duplicates_unit(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "组完全相同副本",
        Language::En => "Sets found across folders",
    }
}

pub fn tr_declutter_overview_select(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "智能挑选 ›",
        Language::En => "Select ›",
    }
}

pub fn tr_declutter_overview_photos_kicker(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "视觉冗余整理",
        Language::En => "VISUAL CLUTTER",
    }
}

pub fn tr_declutter_overview_photos_unit(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "相似/连拍照片组",
        Language::En => "Estimated groups",
    }
}

// ---------------------------------------------------------------- 相似图片

pub fn tr_declutter_photos_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "相似图片",
        Language::En => "Similar Photos",
    }
}

pub fn tr_declutter_photos_heading(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "相似图片整理",
        Language::En => "Similar Photos",
    }
}

pub fn tr_declutter_photos_kicker(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "◧ 冗余整理",
        Language::En => "◧ DECLUTTER MODULE",
    }
}

pub fn tr_declutter_photos_subheading(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "审查视觉上高度相似或连拍的图片组。系统已自动为您标出每组最佳品质的照片，只需一键清理冗余版本。",
        Language::En => "Review grouped images that appear visually identical or highly similar. We've highlighted the highest quality version in each group.",
    }
}

pub fn tr_declutter_photos_empty_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "暂未发现相似或连拍冗余照片",
        Language::En => "No similar or burst photos found",
    }
}

pub fn tr_declutter_photos_empty_desc(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "您的相册非常整洁，未发现连拍或高重复度照片。",
        Language::En => "Your photo library is clean without redundant bursts.",
    }
}

pub fn tr_declutter_photos_best_quality(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "最佳品质 (已保留)",
        Language::En => "Best Quality (Kept)",
    }
}

pub fn tr_declutter_photos_to_clean(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "待清理",
        Language::En => "To Clean",
    }
}

pub fn tr_declutter_photos_kept(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "已保留",
        Language::En => "Kept",
    }
}

pub fn tr_declutter_photos_keep_best(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "★ 仅保留最佳",
        Language::En => "★ Keep Best Only",
    }
}

pub fn tr_declutter_photos_keep_all(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "全部保留",
        Language::En => "Keep All",
    }
}

pub fn tr_declutter_photos_smart_select(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "✨ 自动挑选最佳",
        Language::En => "✨ Smart Select All",
    }
}

pub fn tr_declutter_photos_collapse(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "▲ 收起",
        Language::En => "▲ Collapse",
    }
}

pub fn tr_declutter_photos_redundant_copies(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("待清理相似副本 (共 {count} 张)："),
        Language::En => format!("Redundant Copies ({count}):"),
    }
}

pub fn tr_declutter_photos_show_more(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("+ 查看其余 {count} 张..."),
        Language::En => format!("+ {count} more..."),
    }
}

/// 一组照片的统计行：总张数，以及选中时的待清理张数与可释放体积。
pub fn tr_declutter_photos_group_stats(
    lang: Language,
    total: usize,
    selected: usize,
    cleanable: &str,
) -> String {
    match (lang, selected > 0) {
        (Language::Zh, true) => {
            format!("• 共 {total} 张照片 (待清理 {selected} 张 · 可释放 {cleanable})")
        }
        (Language::Zh, false) => format!("• 共 {total} 张照片 (全部已保留)"),
        (Language::En, true) => format!("• {total} photos ({selected} to clean · {cleanable})"),
        (Language::En, false) => format!("• {total} photos (all kept)"),
    }
}

// ---------------------------------------------------------------- 重复文件

pub fn tr_declutter_duplicates_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "重复文件",
        Language::En => "Duplicates",
    }
}

pub fn tr_declutter_duplicates_heading(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "重复文件",
        Language::En => "Duplicate Files",
    }
}

pub fn tr_declutter_duplicates_empty_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "未发现重复文件",
        Language::En => "No duplicate files found",
    }
}

pub fn tr_declutter_duplicates_empty_desc(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "您的磁盘非常整洁，未发现占用空间的完全相同文件副本。",
        Language::En => "Your drive is clean with no identical duplicate files.",
    }
}

pub fn tr_declutter_duplicates_original(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "原件",
        Language::En => "ORIGINAL",
    }
}

pub fn tr_declutter_duplicates_copy(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "副本",
        Language::En => "DUPLICATE",
    }
}

pub fn tr_declutter_duplicates_keep_newest(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "保留最新副本",
        Language::En => "Keep Newest",
    }
}

pub fn tr_declutter_duplicates_keep_oldest(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "保留最旧副本",
        Language::En => "Keep Oldest",
    }
}

pub fn tr_declutter_duplicates_group_sub(lang: Language, size: &str, copies: usize) -> String {
    match lang {
        Language::Zh => format!("{size} · {copies} 份相同副本"),
        Language::En => format!("{size} · {copies} identical copies"),
    }
}

/// 列表上方的总结句。超过 50 组时列表只展示最大的 50 组，末尾追一句说明。
pub fn tr_declutter_duplicates_summary(lang: Language, groups: usize) -> String {
    let capped = groups > 50;
    match lang {
        Language::Zh => format!(
            "共发现 {groups} 组完全相同的重复副本占用磁盘空间。{}",
            if capped {
                "（列表展示占用空间最大的前 50 组）"
            } else {
                ""
            }
        ),
        Language::En => format!(
            "Found {groups} sets of identical duplicate files hoarding storage.{}",
            if capped {
                " (Displaying top 50 largest groups)"
            } else {
                ""
            }
        ),
    }
}

// ------------------------------------------------------------ 大型与旧文件

pub fn tr_declutter_large_files_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "大型与旧文件",
        Language::En => "Large & Old Files",
    }
}

pub fn tr_declutter_large_files_heading(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "审查大型文件",
        Language::En => "Review Items",
    }
}

pub fn tr_declutter_large_files_subheading(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "选择不再需要的文件，安全清理以释放宝贵的磁盘空间。",
        Language::En => {
            "Select files you no longer need. Safely remove them to free up disk space."
        }
    }
}

pub fn tr_declutter_large_files_empty_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "未发现符合条件的大型文件",
        Language::En => "No large files found",
    }
}

pub fn tr_declutter_large_files_empty_desc(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "未找到大于指定筛选体积的文件，您可以尝试清除筛选条件。",
        Language::En => "No files exceed the size filter. Try clearing the filter.",
    }
}

pub fn tr_declutter_large_files_total_found(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "筛选总计",
        Language::En => "TOTAL FOUND",
    }
}

pub fn tr_declutter_large_files_clear_filters(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "清除筛选",
        Language::En => "Clear Filters",
    }
}

// -------------------------------------------------------------------- 下载项

pub fn tr_declutter_downloads_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "下载项整理",
        Language::En => "Downloads",
    }
}

pub fn tr_declutter_downloads_heading(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "审查下载项",
        Language::En => "Review Downloads",
    }
}

pub fn tr_declutter_downloads_subheading(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "清理 Downloads 目录中残留的历史安装包、压缩归档与临时文件。",
        Language::En => "Clean old DMG installers, archives and temp files from ~/Downloads.",
    }
}

pub fn tr_declutter_downloads_empty_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "下载文件夹暂无可清理项",
        Language::En => "No downloads found",
    }
}

pub fn tr_declutter_downloads_empty_desc(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "您的 Downloads 文件夹中没有可识别的历史安装包或残留归档。",
        Language::En => "No installer packages or archives found in Downloads.",
    }
}

pub fn tr_declutter_downloads_count(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("{count} 项"),
        Language::En => format!("{count} items"),
    }
}

// ---------------------------------------------------------------- 右键菜单

pub fn tr_declutter_ctx_reveal_finder(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "在访达中定位显示",
        Language::En => "Reveal in Finder",
    }
}

pub fn tr_declutter_ctx_reveal_explorer(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "在文件资源管理器中定位",
        Language::En => "Reveal in File Explorer",
    }
}

pub fn tr_declutter_ctx_reveal_generic(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "在文件管理器中定位",
        Language::En => "Show in File Manager",
    }
}

pub fn tr_declutter_ctx_open(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "使用系统程序打开",
        Language::En => "Open with System App",
    }
}

pub fn tr_declutter_ctx_copy_path(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "复制完整文件路径",
        Language::En => "Copy Full Path",
    }
}

// ------------------------------------- 大文件页筛选 chips 与概览页状态徽标
// 这几处原先以 `match (值, lang)` 的元组匹配留在视图里，不在当初
// 「78 处内联 match lang」的统计口径内，因此漏在了 tr_* 体系外面。

/// 大文件页「大小」筛选 chip。`min_size` 为 0 表示未选（下拉箭头），
/// 其余为已选阈值（✕ 可清除）。
pub fn tr_declutter_large_size_filter(lang: Language, min_size: u64) -> &'static str {
    match (min_size, lang) {
        (0, Language::Zh) => "大小: 全部 ▾",
        (0, Language::En) => "Size: All ▾",
        (50_000_000, Language::Zh) => "大小: > 50MB ✕",
        (50_000_000, Language::En) => "Size: > 50MB ✕",
        (100_000_000, Language::Zh) => "大小: > 100MB ✕",
        (100_000_000, Language::En) => "Size: > 100MB ✕",
        (500_000_000, Language::Zh) => "大小: > 500MB ✕",
        (500_000_000, Language::En) => "Size: > 500MB ✕",
        (_, Language::Zh) => "大小: > 1GB ✕",
        (_, Language::En) => "Size: > 1GB ✕",
    }
}

/// 大文件页「类型」筛选 chip。`kind` 为 `None` 表示未选。
pub fn tr_declutter_large_kind_filter(lang: Language, kind: Option<usize>) -> &'static str {
    match (kind, lang) {
        (Some(0), Language::Zh) => "类型: 视频 ✕",
        (Some(0), Language::En) => "Kind: Video ✕",
        (Some(1), Language::Zh) => "类型: 压缩包 ✕",
        (Some(1), Language::En) => "Kind: Archive ✕",
        (Some(2), Language::Zh) => "类型: 文件夹 ✕",
        (Some(2), Language::En) => "Kind: Folder ✕",
        (Some(3), Language::Zh) => "类型: 图片 ✕",
        (Some(3), Language::En) => "Kind: Image ✕",
        (_, Language::Zh) => "类型: 全部 ▾",
        (_, Language::En) => "Kind: All Types ▾",
    }
}

/// 概览页头部的分析状态徽标。
pub fn tr_declutter_overview_status_badge(lang: Language, scanned: bool) -> &'static str {
    match (scanned, lang) {
        (true, Language::Zh) => "● 已分析",
        (true, Language::En) => "● Analyzed",
        (false, Language::Zh) => "● 待扫描",
        (false, Language::En) => "● Pending",
    }
}
