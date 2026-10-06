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

#[cfg(test)]
mod tests {
    use super::*;

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
}
