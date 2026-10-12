//! 视图层文案：各主视图（磁盘、检索、垃圾列表、仪表盘、应用）的标题、
//! 计数与格式化句式。原先是内联在各 `views/*.rs` 里的中英 match，
//! 迁到这里后遵循与其余 `tr_*` 相同的约定：渲染时按当前语言取。

use crate::core::i18n::Language;

// ---- 文件快速检索 ----

pub fn tr_search_footer_top(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("全盘最大的 {count} 项 · 输入关键字精确检索"),
        Language::En => format!("Top {count} largest items · type to filter"),
    }
}

pub fn tr_search_footer_matched(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("匹配到 {count} 个文件 / 文件夹"),
        Language::En => format!("Matched {count} items"),
    }
}

// ---- 垃圾清理列表 ----

pub fn tr_junk_total_items(lang: Language, total: usize) -> String {
    match lang {
        Language::Zh => format!("共 {total} 项"),
        Language::En => format!("{total} items"),
    }
}

pub fn tr_junk_selected_items(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("({count} 项)"),
        Language::En => format!("({count} items)"),
    }
}

// ---- 磁盘透镜：左栏 ----

pub fn tr_disk_total_capacity(lang: Language, size: &str) -> String {
    match lang {
        Language::Zh => format!("{size} 总容量"),
        Language::En => format!("{size} Total"),
    }
}

pub fn tr_disk_scanned_size(lang: Language, size: &str) -> String {
    match lang {
        Language::Zh => format!("{size} 已扫描"),
        Language::En => format!("{size} Scanned"),
    }
}

pub fn tr_disk_breadcrumb_root(lang: Language, volume: &str) -> String {
    match lang {
        Language::Zh => format!("{volume}: 根目录"),
        Language::En => format!("{volume}: Root"),
    }
}

// ---- 磁盘透镜：主视图 ----

pub fn tr_disk_error_hint(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "请确保以管理员权限运行，或切换至其他可用盘符重试",
        Language::En => "Please ensure running as administrator or switch to another drive",
    }
}

pub fn tr_disk_error_prefix(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "磁盘分析失败：",
        Language::En => "Disk analysis failed: ",
    }
}

pub fn tr_disk_prompt_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "选择要分析的磁盘并开始深度扫描",
        Language::En => "Select a drive to analyze storage hierarchy",
    }
}

pub fn tr_disk_scan_button(lang: Language, volume: &str) -> String {
    match lang {
        Language::Zh => format!("开始分析 {volume} 空间占用"),
        Language::En => format!("Analyze Storage for {volume}"),
    }
}

// ---- 磁盘选择卡片 ----

pub fn tr_volume_drive_label(lang: Language, raw: &str) -> String {
    match lang {
        Language::Zh => format!("{raw} 盘"),
        Language::En => format!("Drive {raw}"),
    }
}

pub fn tr_volume_system_root(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "系统盘 (/)",
        Language::En => "System (/)",
    }
}

pub fn tr_volume_picker_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "选择要分析的磁盘",
        Language::En => "Select Drive to Analyze",
    }
}

pub fn tr_volume_available_count(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("{count} 个可用磁盘"),
        Language::En => format!("{count} available"),
    }
}

pub fn tr_volume_used_of_total(lang: Language, used: &str, total: &str) -> String {
    match lang {
        Language::Zh => format!("已用 {used} / 共 {total}"),
        Language::En => format!("Used {used} / Total {total}"),
    }
}

// ---- 磁盘透镜：右栏 ----

pub fn tr_disk_selected_badge(lang: Language, count: usize, size: &str) -> String {
    match lang {
        Language::Zh => format!("已选 {count} 项 ({size})"),
        Language::En => format!("{count} items ({size})"),
    }
}

pub fn tr_disk_up_button(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "← 上级",
        Language::En => "← Up",
    }
}

pub fn tr_disk_protected_label(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "系统保护项目",
        Language::En => "System Protected",
    }
}

pub fn tr_disk_delete_label(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "删除",
        Language::En => "Delete",
    }
}

pub fn tr_disk_selected_count(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("{count} 项已选中"),
        Language::En => format!("{count} items selected"),
    }
}

// ---- 磁盘占比面板 ----

pub fn tr_breakdown_other_used(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("其他已用 {count} 项"),
        Language::En => format!("Other {count} items"),
    }
}

pub fn tr_breakdown_free_space(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "空闲可用空间",
        Language::En => "Free Space",
    }
}

pub fn tr_breakdown_used_free(lang: Language, used_pct: u64, free: &str) -> String {
    match lang {
        Language::Zh => format!("已用 {used_pct}% · 空闲 {free}"),
        Language::En => format!("Used {used_pct}% · Free {free}"),
    }
}

pub fn tr_breakdown_other_items(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("其他 {count} 项"),
        Language::En => format!("Other {count} items"),
    }
}

pub fn tr_breakdown_folder_items(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("当前目录共 {count} 个子项"),
        Language::En => format!("{count} items in folder"),
    }
}

pub fn tr_breakdown_files_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "全盘大文件分布",
        Language::En => "Largest Files Breakdown",
    }
}

pub fn tr_breakdown_files_count(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("前 {count} 个大文件汇总"),
        Language::En => format!("Top {count} large files"),
    }
}

// ---- 仪表盘 ----

pub fn tr_dashboard_junk_found(lang: Language, categories: usize, size: &str) -> String {
    match lang {
        Language::Zh => format!("已在 {categories} 个类别中发现 {size} 可清理内容。"),
        Language::En => format!("Found {size} cleanable items across {categories} categories."),
    }
}

pub fn tr_dashboard_clean_junk(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "一键清理",
        Language::En => "Clean Junk",
    }
}

pub fn tr_dashboard_apps_found(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("已发现 {count} 款"),
        Language::En => format!("{count} Apps"),
    }
}

pub fn tr_dashboard_uninstall_analysis(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "卸载分析",
        Language::En => "Uninstall",
    }
}

pub fn tr_dashboard_storage_lens(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "空间透镜",
        Language::En => "Storage",
    }
}

pub fn tr_dashboard_declutter(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "冗余整理",
        Language::En => "Declutter",
    }
}

pub fn tr_dashboard_declutter_sub(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "重复/大文件",
        Language::En => "Duplicates/Photos",
    }
}

// ---- 已安装应用列表 ----

pub fn tr_apps_count(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("{count} 款"),
        Language::En => format!("{count} Apps"),
    }
}

pub fn tr_apps_filter_all(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("共 {count} 款"),
        Language::En => format!("{count} apps"),
    }
}

pub fn tr_apps_filter_matched(lang: Language, shown: usize, total: usize) -> String {
    match lang {
        Language::Zh => format!("匹配 {shown} / {total} 款"),
        Language::En => format!("Matched {shown} of {total} apps"),
    }
}

pub fn tr_apps_name_header(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("应用名称与版本 (共 {count} 款)"),
        Language::En => format!("Name & Version ({count} apps)"),
    }
}

pub fn tr_apps_empty(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "未找到匹配的已安装软件",
        Language::En => "No matching applications found",
    }
}

pub fn tr_apps_footer_all(lang: Language, shown: usize, total: usize) -> String {
    match lang {
        Language::Zh => format!("当前列表展示 {shown} 款软件（总计 {total} 款已装）"),
        Language::En => format!("Displaying {shown} apps (Total {total} installed)"),
    }
}

pub fn tr_apps_footer_matched(lang: Language, shown: usize, total: usize) -> String {
    match lang {
        Language::Zh => format!("搜索匹配 {shown} 款软件（总计 {total} 款）"),
        Language::En => format!("Matched {shown} apps (Total {total})"),
    }
}

pub fn tr_apps_total_size(lang: Language, size: &str) -> String {
    match lang {
        Language::Zh => format!("列表总占用: {size}"),
        Language::En => format!("Total Size: {size}"),
    }
}

pub fn tr_search_searching(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "正在后台高速检索…",
        Language::En => "Searching…",
    }
}

pub fn tr_col_action(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "操作",
        Language::En => "Action",
    }
}

pub fn tr_search_footer_hint(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "双击条目直接打开 · 点击表头自定义排序",
        Language::En => "Double-click to open · Click header to sort",
    }
}

pub fn tr_search_footer_limit(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "单次最多展示 500 项",
        Language::En => "Max 500 items",
    }
}

pub fn tr_common_open(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "打开",
        Language::En => "Open",
    }
}

pub fn tr_common_reveal(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "定位",
        Language::En => "Reveal",
    }
}

pub fn tr_col_name(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "名称",
        Language::En => "Name",
    }
}

pub fn tr_col_size(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "大小",
        Language::En => "Size",
    }
}

pub fn tr_disk_selected_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "已选择要清理的磁盘项目",
        Language::En => "Selected Disk Items",
    }
}

pub fn tr_junk_selected_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "已选择清理",
        Language::En => "Selected for Cleaning",
    }
}

pub fn tr_apps_quick_filter(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "快速分类:",
        Language::En => "Presets:",
    }
}

pub fn tr_language_label(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "界面语言",
        Language::En => "Language",
    }
}

// ---- 概览页 ----

pub fn tr_dashboard_scanning_sub(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "正在检查系统缓存与开发环境残留",
        Language::En => "Checking system caches and dev leftovers",
    }
}

pub fn tr_dashboard_cleaning_sub(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "正在彻底移除选中垃圾",
        Language::En => "Permanently removing selected items",
    }
}

pub fn tr_dashboard_scanning_blurb(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "正在扫描系统缓存、应用日志、开发依赖产物与临时文件…",
        Language::En => {
            "Scanning system caches, application logs, build artifacts, and temp files…"
        }
    }
}

pub fn tr_dashboard_clean_blurb(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "未发现可清理的冗余缓存，系统状态良好。",
        Language::En => "No redundant caches found. System is running cleanly.",
    }
}

pub fn tr_dashboard_idle_blurb(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "快速扫描系统缓存、开发依赖产物与磁盘占用。",
        Language::En => "Quickly scan system caches, developer dependencies, and disk usage.",
    }
}

pub fn tr_dashboard_review_junk(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "查看详情并清理",
        Language::En => "Review & Clean Junk",
    }
}

// ---- 软件管理 ----

pub fn tr_apps_stat_storage(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "估算总占用空间",
        Language::En => "Total Estimated Size",
    }
}

pub fn tr_apps_stat_installed(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "已安装应用总数",
        Language::En => "Installed Applications",
    }
}

pub fn tr_apps_stat_stale(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "长期未用软件 (>90天)",
        Language::En => "Rarely Used (>90 days)",
    }
}

pub fn tr_apps_loading_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "正在智能检索已安装软件与空间占用",
        Language::En => "Scanning installed applications…",
    }
}

pub fn tr_apps_loading_sub(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "深度测算软件安装目录、真实体积与最后使用时间",
        Language::En => "Calculating installed size, directories and last usage date",
    }
}

// ---- 磁盘透镜 ----

pub fn tr_disk_loading_title(lang: Language, volume: &str) -> String {
    match lang {
        Language::Zh => format!("正在深度分析磁盘 {volume} 空间占用"),
        Language::En => format!("Analyzing storage for drive {volume}…"),
    }
}

pub fn tr_disk_loading_sub(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "快速索引全盘文件结构与体积分布，请稍候",
        Language::En if cfg!(target_os = "macos") => {
            "Scanning file hierarchy and sizes, please wait"
        }
        Language::En => "Indexing NTFS file hierarchy and sizes, please wait",
    }
}

pub fn tr_disk_volume_usage(lang: Language, used: &str, total: &str) -> String {
    match lang {
        Language::Zh => format!("已用 {used} / 共 {total}"),
        Language::En => format!("Used {used} / Total {total}"),
    }
}

pub fn tr_disk_dir_empty(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "此目录为空",
        Language::En => "Directory is empty",
    }
}

pub fn tr_disk_no_large_files(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "未找到大文件",
        Language::En => "No large files found",
    }
}

pub fn tr_disk_copy_path(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "复制路径",
        Language::En => "Copy path",
    }
}

pub fn tr_disk_path_copied(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "路径已复制",
        Language::En => "Path copied",
    }
}

pub fn tr_disk_kind_media(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "媒体视频 (Media)",
        Language::En => "Media & Video",
    }
}

pub fn tr_disk_kind_binaries(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "程序/动态库 (Bin)",
        Language::En => "Binaries & DLLs",
    }
}

pub fn tr_disk_kind_archives(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "压缩镜像 (Archive)",
        Language::En => "Archives & Images",
    }
}

pub fn tr_disk_kind_documents(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "文档数据 (Data)",
        Language::En => "Documents & Data",
    }
}

pub fn tr_disk_kind_other(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "其他文件 (Others)",
        Language::En => "Other Files",
    }
}

// ---- 智能清理 ----

pub fn tr_junk_heading_sub(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "可重建的应用、浏览器、包管理与应用更新包缓存默认已勾选；临时数据与开发产物需手动勾选",
        Language::En => "Rebuildable app, browser, package-manager and update-package caches are selected; temp data and dev builds require manual selection",
    }
}

pub fn tr_junk_loading_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "正在全面扫描系统冗余垃圾",
        Language::En => "Scanning system junk files…",
    }
}

pub fn tr_junk_loading_sub(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "安全检索系统临时缓存、应用日志与回收站，准备释放空间",
        Language::En => "Safely checking system temp, caches, logs and Recycle Bin",
    }
}

/// 行内列举的分隔符。
pub fn tr_list_separator(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "、",
        Language::En => ", ",
    }
}

pub fn tr_status_uptime(lang: Language, days: u64, hours: u64, minutes: u64) -> String {
    match lang {
        Language::Zh if days > 0 => format!("{days} 天 {hours} 小时"),
        Language::Zh if hours > 0 => format!("{hours} 小时 {minutes} 分"),
        Language::Zh => format!("{minutes} 分钟"),
        Language::En if days > 0 => format!("{days}d {hours}h"),
        Language::En if hours > 0 => format!("{hours}h {minutes}m"),
        Language::En => format!("{minutes}m"),
    }
}

pub fn tr_view_declutter(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "冗余整理",
        Language::En => "Declutter",
    }
}

pub fn tr_view_search(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "文件搜索",
        Language::En => "File Search",
    }
}

/// 语言切换器上的语言自称，不随当前界面语言变化。
pub fn tr_language_name(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "中文",
        Language::En => "English",
    }
}

pub fn tr_disk_crumbs_collapse(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "‹ 收起",
        Language::En => "‹ Collapse",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::i18n::*;

    /// 两种语言都必须给出非空文案；带计数的句式必须真的带上数字。
    #[test]
    fn view_strings_are_localized_and_carry_their_values() {
        for lang in [Language::Zh, Language::En] {
            assert!(!tr_disk_up_button(lang).is_empty());
            assert!(!tr_apps_empty(lang).is_empty());
            let count = tr_apps_count(lang, 7);
            assert!(count.contains('7'), "{count}");
            let selected = tr_disk_selected_count(lang, 3);
            assert!(selected.contains('3'), "{selected}");
            let used = tr_breakdown_used_free(lang, 42, "10 GB");
            assert!(used.contains("42") && used.contains("10 GB"), "{used}");
            let found = tr_dashboard_junk_found(lang, 2, "1.5 GB");
            assert!(found.contains('2') && found.contains("1.5 GB"), "{found}");
        }
        assert_ne!(
            tr_disk_up_button(Language::Zh),
            tr_disk_up_button(Language::En)
        );
    }

    fn has_cjk(s: &str) -> bool {
        s.chars()
            .any(|c| matches!(c, '\u{3000}'..='\u{303f}' | '\u{4e00}'..='\u{9fff}' | '\u{ff00}'..='\u{ffef}'))
    }

    /// 迁移前这些文案只有中文，英文界面也显示中文；英文分支必须是纯英文，
    /// 批量移除的「跳过」计数在英文里也不能丢。
    #[test]
    fn english_copy_has_no_cjk_and_keeps_every_count() {
        let en = Language::En;
        let samples = [
            tr_dev_batch_removing(en, 3),
            tr_dev_batch_removing_item(en, 1, 3, "numpy"),
            tr_dev_batch_selection(en, 0, "0 B"),
            tr_dev_batch_selection(en, 2, "1 GB"),
            tr_dev_batch_confirm_detail(en, "1 GB", "· a"),
            tr_declutter_preview(en).to_string(),
            tr_declutter_reveal(en).to_string(),
            tr_disk_crumbs_collapse(en).to_string(),
            tr_declutter_path_copied(en, "/tmp/a"),
            tr_residual_launchd_evidence(en, "com.example.agent"),
            tr_status_uptime(en, 1, 2, 3),
            tr_list_separator(en).to_string(),
        ];
        for text in &samples {
            assert!(!has_cjk(text), "{text}");
        }
        let skipped = tr_dev_batch_remove_done(en, 4, "1 GB", 0, 2);
        assert!(
            skipped.contains("4") && skipped.contains("2 skipped"),
            "{skipped}"
        );
        let zh = tr_dev_batch_remove_done(Language::Zh, 4, "1 GB", 0, 2);
        assert!(zh.contains("跳过 2"), "{zh}");
    }
}
