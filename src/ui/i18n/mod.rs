//! UI 视图层国际化（i18n）文案映射
//!
//! # 和 `core::i18n` 的分工
//!
//! 项目里有两套 i18n 机制，边界是「文案在什么时候确定语言」：
//!
//! - **本模块的 `tr_*` 函数**：渲染时按当前语言取。适用于**每帧重新渲染**
//!   的文案——按钮标题、列头、空状态提示。用户切语言时下一帧就跟着变，
//!   不需要任何额外处理。
//! - **`core::i18n::Text`（配 `bilingual()`）**：把两种语言都存下来，
//!   读的时候再挑。适用于**在某个时刻生成、之后长期挂着**的文案——
//!   状态栏那句话、扫描项的标签。它们生成于后台线程，那时还不知道用户
//!   之后会切到哪种语言；而切语言不该触发重扫，也不该让状态栏停在旧语言上。
//!
//! 判断方法：这段文字是「每次渲染重新算」还是「算一次存着」？前者用
//! `tr_*`，后者用 `Text`。
//!
//! # 为什么是一堆函数而不是一张表
//!
//! 每个 `tr_*` 都是一个 `match lang`，加语言时编译器会强制补全每一处。
//! 换成 `HashMap<&str, ...>` 就只能在运行时发现漏翻，对一个只有两种语言、
//! 靠穷尽匹配吃红利的项目来说是净亏。
//!
//! # 分文件规则
//!
//! 文案按视图域分文件：本文件放全局与各主视图共用的，`declutter` 子模块放
//! 冗余整理那一组。子模块整体 `pub use` 出来，调用方永远只写
//! `use crate::ui::i18n::*;` 一行，不需要知道某条文案住在哪个文件。
//!
//! # 一条约束
//!
//! 从 `core` / `platform` 冒上来的错误 payload 必须是**语言中立**的
//! （API 名、错误码、路径），因为它们会被原样嵌进这里的本地化外壳。
//! 见 `tr_scan_error`。

mod declutter;
pub use declutter::*;

use crate::core::i18n::Language;

pub fn tr_view_dashboard(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "概览扫描",
        Language::En => "Overview",
    }
}

pub fn tr_view_junk(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "智能清理",
        Language::En => "Smart Clean",
    }
}

pub fn tr_view_apps(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "软件管理",
        Language::En => "Apps",
    }
}

pub fn tr_view_disk(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "磁盘透镜",
        Language::En => "Disk Lens",
    }
}

pub fn tr_app_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "QuickCleaner",
        Language::En => "QuickCleaner",
    }
}

pub fn tr_app_subtitle(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "极速磁盘与软件清理",
        Language::En => "Fast Disk & App Cleaner",
    }
}

pub fn tr_freed_total(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "本次已释放空间",
        Language::En => "Space Freed",
    }
}

pub fn tr_scanning(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "扫描中…",
        Language::En => "Scanning…",
    }
}

pub fn tr_cleaning(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "清理中…",
        Language::En => "Cleaning…",
    }
}

pub fn tr_found_cleanable(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "发现可清理内容",
        Language::En => "Cleanable Found",
    }
}

pub fn tr_system_clean(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "系统很干净",
        Language::En => "System is Clean",
    }
}

pub fn tr_no_junk(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "暂无可清理垃圾",
        Language::En => "No Junk Found",
    }
}

pub fn tr_start_smart_scan(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "点击开始一键智能扫描",
        Language::En => "Click to Start Smart Scan",
    }
}

pub fn tr_clean_now(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "立即清理",
        Language::En => "Clean Now",
    }
}

pub fn tr_batch_rec(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "推荐选中",
        Language::En => "Recommended",
    }
}

pub fn tr_batch_all(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "全选",
        Language::En => "Select All",
    }
}

pub fn tr_batch_invert(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "反选",
        Language::En => "Invert",
    }
}

pub fn tr_batch_clear(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "清空选中",
        Language::En => "Clear All",
    }
}

/// 发现式扫描的时间预算耗尽，这一类的统计不完整。
///
/// 文案要让用户明白「数字偏小」而不是「出错了」——扫到的都是真的，只是
/// 可能还有没扫到的。体积旁边同时会显示 `≥` 前缀。
pub fn tr_partial_scan(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "部分统计",
        Language::En => "Partial",
    }
}

pub fn tr_need_manual_select(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "需手动勾选",
        Language::En => "Manual Select",
    }
}

pub fn tr_apps_heading(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "软件全生命周期管理",
        Language::En => "Applications Manager",
    }
}

pub fn tr_apps_subheading(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "分析软件占用并深度清理卸载残留",
        Language::En => "Analyze disk usage and thoroughly clean residual files",
    }
}

pub fn tr_search_placeholder(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "搜索软件名称或发布者…",
        Language::En => "Search apps by name or publisher…",
    }
}

pub fn tr_th_name(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "软件名称",
        Language::En => "Name",
    }
}

pub fn tr_th_publisher(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "发布者",
        Language::En => "Publisher",
    }
}

pub fn tr_th_last_used(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "最近使用",
        Language::En => "Last Used",
    }
}

pub fn tr_th_installed_date(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "安装日期",
        Language::En => "Installed Date",
    }
}

pub fn tr_th_size(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "预估占用",
        Language::En => "Size",
    }
}

pub fn tr_th_actions(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "操作",
        Language::En => "Actions",
    }
}

pub fn tr_btn_uninstall(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "卸载",
        Language::En => "Uninstall",
    }
}

pub fn tr_btn_force_clean(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "强力清理",
        Language::En => "Force Clean",
    }
}

pub fn tr_disk_heading(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "Disk Lens 磁盘透镜",
        Language::En => "Disk Lens Analyzer",
    }
}

pub fn tr_disk_subheading(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "分析磁盘各层级空间占用，定位大文件与冗余目录",
        Language::En => "Analyze disk usage by hierarchy and locate large files",
    }
}

pub fn tr_tab_tree(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "目录树",
        Language::En => "Directory Tree",
    }
}

pub fn tr_tab_files(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "全盘大文件",
        Language::En => "Large Files",
    }
}

pub fn tr_btn_clear_sel(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "清空选择",
        Language::En => "Clear Selection",
    }
}

pub fn tr_btn_cancel(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "取消",
        Language::En => "Cancel",
    }
}

pub fn tr_btn_confirm_delete(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "确认永久删除",
        Language::En => "Confirm Permanent Delete",
    }
}

pub fn tr_btn_done(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "完成",
        Language::En => "Done",
    }
}

pub fn tr_files_suffix(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "个文件",
        Language::En => "files",
    }
}

pub fn tr_drive_suffix(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "盘",
        Language::En => "Drive",
    }
}

// ============================================================================
// 状态栏文案
//
// 这些串以前直接写死在 `ui/mod.rs` 里——界面其余部分都双语了，只有状态栏
// 在英文模式下仍然一路中文。带参数的返回 String，不带参数的返回
// &'static str，与本文件其余词条保持一致。
// ============================================================================

pub fn tr_status_ready(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "就绪",
        Language::En => "Ready",
    }
}

pub fn tr_status_scanning(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "正在扫描可清理内容…",
        Language::En => "Scanning for cleanable content…",
    }
}

pub fn tr_status_scan_done(lang: Language, total: &str) -> String {
    match lang {
        Language::Zh => format!("扫描完成，共发现 {total} 可清理"),
        Language::En => format!("Scan complete — {total} cleanable"),
    }
}

pub fn tr_status_disk_scanning(lang: Language, vol: &crate::core::disk::VolumeId) -> String {
    match lang {
        Language::Zh => format!("正在深度分析磁盘 {vol}: 空间占用…"),
        Language::En => format!("Analyzing space usage on {vol}:…"),
    }
}

pub fn tr_status_disk_done(lang: Language, files: u64, size: &str, elapsed_secs: f64) -> String {
    match lang {
        Language::Zh => {
            format!("磁盘分析完成：已索引 {files} 个文件，占用 {size}，耗时 {elapsed_secs:.1}s")
        }
        Language::En => format!(
            "Disk analysis complete — {files} files indexed, {size} used, took {elapsed_secs:.1}s"
        ),
    }
}

pub fn tr_status_disk_failed(lang: Language, err: &str) -> String {
    match lang {
        Language::Zh => format!("磁盘分析失败：{err}"),
        Language::En => format!("Disk analysis failed: {err}"),
    }
}

pub fn tr_status_apps_scanning(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "正在智能检索已安装软件与空间占用…",
        Language::En => "Discovering installed apps and their disk usage…",
    }
}

pub fn tr_status_apps_done(lang: Language, count: usize, size: &str) -> String {
    match lang {
        Language::Zh => format!("已加载 {count} 款软件，估算总占用 {size}"),
        Language::En => format!("Loaded {count} apps, about {size} in total"),
    }
}

pub fn tr_status_uninstall_waiting(lang: Language, name: &str) -> String {
    match lang {
        Language::Zh => format!("已记录「{name}」的关联痕迹，正在等待官方卸载程序结束…"),
        Language::En => {
            format!("Traces of \"{name}\" recorded — waiting for its uninstaller to finish…")
        }
    }
}

pub fn tr_uninstall_progress_title(lang: Language, name: &str) -> String {
    match lang {
        Language::Zh => format!("正在卸载「{name}」"),
        Language::En => format!("Uninstalling \"{name}\""),
    }
}

pub fn tr_uninstall_phase_discovering(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "正在分析应用及关联文件",
        Language::En => "Analyzing the app and associated files",
    }
}

pub fn tr_uninstall_phase_removing(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "正在移除应用程序",
        Language::En => "Removing the application",
    }
}

pub fn tr_uninstall_phase_verifying(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "正在确认卸载结果",
        Language::En => "Verifying the uninstall result",
    }
}

pub fn tr_uninstall_stage_discover(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "分析关联文件",
        Language::En => "Analyze associated files",
    }
}

pub fn tr_uninstall_stage_remove(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "移除应用程序",
        Language::En => "Remove application",
    }
}

pub fn tr_uninstall_stage_verify(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "复核残留项目",
        Language::En => "Verify remaining files",
    }
}

pub fn tr_uninstall_keep_open(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "卸载完成前请保持 QuickCleaner 运行",
        Language::En => "Keep QuickCleaner open until the uninstall completes",
    }
}

pub fn tr_status_uninstall_done(lang: Language, name: &str) -> String {
    match lang {
        Language::Zh => format!("「{name}」官方卸载已完成"),
        Language::En => format!("\"{name}\" uninstaller finished"),
    }
}

pub fn tr_status_uninstall_failed(lang: Language, name: &str) -> String {
    match lang {
        Language::Zh => format!("「{name}」官方卸载未正常完成"),
        Language::En => format!("\"{name}\" uninstaller did not complete normally"),
    }
}

pub fn tr_status_uninstall_residual(
    lang: Language,
    head: &str,
    count: usize,
    size: &str,
) -> String {
    match lang {
        Language::Zh => format!("{head}，复核后仍有 {count} 项残留（{size}）"),
        Language::En => format!("{head} — {count} leftovers remain after verification ({size})"),
    }
}

pub fn tr_status_residual_scanning(lang: Language, name: &str) -> String {
    match lang {
        Language::Zh => format!("正在深度扫描「{name}」的文件与注册表残留…"),
        Language::En => format!("Deep-scanning file and registry leftovers of \"{name}\"…"),
    }
}

pub fn tr_status_residual_done(lang: Language, name: &str, count: usize, size: &str) -> String {
    match lang {
        Language::Zh => format!("残留扫描完成：发现「{name}」的 {count} 项残留，共 {size}"),
        Language::En => format!("Scan complete — {count} leftovers of \"{name}\", {size} total"),
    }
}

pub fn tr_status_residual_none_selected(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "未选择任何要清除的残留项",
        Language::En => "No leftover items selected",
    }
}

/// 残留清理被最后一道判据挡下：这个应用看起来还装着（或者查不出来）。
///
/// 文案刻意不说「清理失败」——什么都没删，这是一次成功的拦截。用户需要
/// 知道的是「为什么没动」和「接下来该怎么办」。
pub fn tr_status_residual_still_installed(lang: Language, name: &str) -> String {
    match lang {
        Language::Zh => {
            format!("已中止：「{name}」看起来仍安装在这台电脑上，未删除任何内容。请先完成卸载再清理残留")
        }
        Language::En => format!(
            "Stopped: \"{name}\" still appears to be installed. Nothing was deleted — finish uninstalling it first"
        ),
    }
}

/// 条目行尾「永久排除」按钮的文案。
pub fn tr_exclude(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "排除",
        Language::En => "Exclude",
    }
}

/// 永久排除完成后的状态行。说清两件事：现在起任何删除通道都会跳过它，
/// 以及本次从列表里移走了几条已扫出的条目。
pub fn tr_status_excluded(lang: Language, removed: usize) -> String {
    match lang {
        Language::Zh => {
            format!("已加入排除清单，所有清理都会跳过此路径（已从列表移走 {removed} 条）")
        }
        Language::En => {
            format!("Added to exclusions — all cleanup will skip this path ({removed} items removed from the list)")
        }
    }
}

pub fn tr_status_residual_cleaning(lang: Language, name: &str, count: usize) -> String {
    match lang {
        Language::Zh => format!("正在彻底清除「{name}」的 {count} 项残留…"),
        Language::En => format!("Removing {count} leftovers of \"{name}\"…"),
    }
}

pub fn tr_status_residual_cleaned(lang: Language, name: &str, count: usize, size: &str) -> String {
    match lang {
        Language::Zh => format!("已彻底清除「{name}」的 {count} 项残留，释放 {size}"),
        Language::En => format!("Removed {count} leftovers of \"{name}\", freed {size}"),
    }
}

/// 有「需手动处理」的残留（SIP 下的系统扩展）时的收尾文案。
///
/// 不能复用 `_partial` 的「被占用或权限不足」——那是在把平台限制说成软件
/// 出错，用户会以为重试一下就好。
pub fn tr_status_residual_cleaned_manual(
    lang: Language,
    name: &str,
    count: usize,
    size: &str,
    manual: usize,
) -> String {
    match lang {
        Language::Zh => format!(
            "已清除「{name}」的 {count} 项残留，释放 {size}（{manual} 项需在系统设置中手动关闭）"
        ),
        Language::En => format!(
            "Removed {count} leftovers of \"{name}\", freed {size} ({manual} need to be turned off in System Settings)"
        ),
    }
}

pub fn tr_status_residual_cleaned_partial(
    lang: Language,
    name: &str,
    size: &str,
    skipped: usize,
) -> String {
    match lang {
        Language::Zh => {
            format!("「{name}」清除完成，释放 {size}（{skipped} 项被占用或权限不足已跳过）")
        }
        Language::En => format!(
            "\"{name}\" cleaned, freed {size} ({skipped} skipped — in use or access denied)"
        ),
    }
}

pub fn tr_status_stopping(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "正在停止清理…",
        Language::En => "Stopping the cleanup…",
    }
}

pub fn tr_status_nothing_selected(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "没有勾选任何要清理的内容",
        Language::En => "Nothing is selected for cleaning",
    }
}

/// 勾选的目标全部处于占用状态时，替代「没有勾选」的提示。
pub fn tr_status_all_busy(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("{count} 个勾选项正在被应用使用，已取消勾选"),
        Language::En => format!("{count} selected items are in use and were unchecked"),
    }
}

/// 完成状态里追加的「跳过了几个占用项」。
pub fn tr_busy_skipped(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("已跳过 {count} 个正在使用的项目"),
        Language::En => format!("skipped {count} in-use items"),
    }
}

pub fn tr_status_deleting_n(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("正在清理 {count} 项…"),
        Language::En => format!("Cleaning {count} items…"),
    }
}

pub fn tr_status_clean_done(lang: Language, files: u64, size: &str) -> String {
    match lang {
        Language::Zh => format!("清理完成：已删除 {files} 个文件，释放 {size}"),
        Language::En => format!("Cleanup complete — {files} files deleted, {size} freed"),
    }
}

pub fn tr_status_clean_done_partial(
    lang: Language,
    files: u64,
    size: &str,
    skipped: usize,
) -> String {
    match lang {
        Language::Zh => {
            format!("清理完成：已删除 {files} 个文件，释放 {size}（{skipped} 项被占用已跳过）")
        }
        Language::En => format!(
            "Cleanup complete — {files} files deleted, {size} freed ({skipped} skipped, in use)"
        ),
    }
}

pub fn tr_status_deleting_path(lang: Language, path: &str) -> String {
    match lang {
        Language::Zh => format!("正在删除 {path}…"),
        Language::En => format!("Deleting {path}…"),
    }
}

pub fn tr_status_deleted_path(lang: Language, path: &str, files: u64, size: &str) -> String {
    match lang {
        Language::Zh => format!("已删除 {path}（{files} 个文件，{size}）"),
        Language::En => format!("Deleted {path} ({files} files, {size})"),
    }
}

pub fn tr_status_delete_failed(lang: Language, path: &str) -> String {
    match lang {
        Language::Zh => format!("删除失败：{path}（被占用或权限不足）"),
        Language::En => format!("Failed to delete {path} (in use or access denied)"),
    }
}

pub fn tr_status_batch_deleting(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("正在批量删除 {count} 项…"),
        Language::En => format!("Deleting {count} selected items…"),
    }
}

pub fn tr_status_batch_done(lang: Language, files: u64, size: &str) -> String {
    match lang {
        Language::Zh => format!("批量删除完成：已删除 {files} 个文件，释放 {size}"),
        Language::En => format!("Batch delete complete — {files} files deleted, {size} freed"),
    }
}

pub fn tr_status_batch_done_partial(lang: Language, size: &str, skipped: usize) -> String {
    match lang {
        Language::Zh => format!("批量删除完成，释放 {size}（{skipped} 项受保护或被占用已跳过）"),
        Language::En => {
            format!("Batch delete complete, freed {size} ({skipped} skipped — protected or in use)")
        }
    }
}

// ============================================================================
// 确认对话框文案
// ============================================================================

pub fn tr_confirm_clean_selected_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "确认清理选中项",
        Language::En => "Confirm cleanup",
    }
}

pub fn tr_confirm_clean_selected_detail(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "缓存、临时文件和构建产物会永久删除；损坏登录项和旧版 IDE 数据会移入废纸篓。",
        Language::En => "Caches, temporary files, and build artifacts are permanently deleted; broken login items and old IDE data are moved to Trash.",
    }
}

pub fn tr_confirm_delete_selected_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "确认永久删除选中项",
        Language::En => "Confirm permanent deletion",
    }
}

pub fn tr_confirm_delete_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "确认永久删除",
        Language::En => "Confirm permanent deletion",
    }
}

pub fn tr_confirm_delete_selected_msg(lang: Language, count: usize, size: &str) -> String {
    match lang {
        Language::Zh => format!("将永久删除 {count} 项，释放约 {size} 磁盘空间。"),
        Language::En => format!("{count} items will be permanently deleted, freeing about {size}."),
    }
}

pub fn tr_confirm_delete_msg(lang: Language, count: usize, size: &str) -> String {
    match lang {
        Language::Zh => format!("将删除 {count} 项，共 {size}。"),
        Language::En => format!("{count} items will be deleted, {size} in total."),
    }
}

pub fn tr_confirm_delete_path_msg(lang: Language, path: &str, size: &str) -> String {
    match lang {
        Language::Zh => format!("将删除 {path}（{size}）。"),
        Language::En => format!("{path} ({size}) will be deleted."),
    }
}

/// 「不进回收站」的基础警告。
pub fn tr_confirm_no_recycle(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "文件不会进入回收站，删除后无法恢复。",
        Language::En => "Files do not go to the Recycle Bin and cannot be recovered.",
    }
}

/// 批量删除时的警告：多提醒一句「别把重要数据勾进去了」。
pub fn tr_confirm_no_recycle_check_data(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "文件与目录不会进入回收站，删除后无法恢复。请确认没有重要数据。",
        Language::En => "Files and folders do not go to the Recycle Bin and cannot be recovered. Make sure nothing important is selected.",
    }
}

/// 确认弹窗的应用数据升级警示：目标触及 ~/Library/Application Support。
pub fn tr_confirm_app_data_warning(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "目标位于 ~/Library/Application Support：这里存放应用数据（聊天记录、密码库、本地数据库等），永久删除后无法恢复。请确认你了解这些目录的用途。",
        Language::En => "The target is under ~/Library/Application Support - application data lives here (chat history, password vaults, local databases) and cannot be recovered once permanently deleted. Make sure you know what these directories contain.",
    }
}

/// 删单个路径时的警告：多提醒一句「别删正在跑的程序的数据」。
pub fn tr_confirm_no_recycle_check_running(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "文件不会进入回收站，删除后无法恢复。请确认它不是正在使用的程序或数据。",
        Language::En => "Files do not go to the Recycle Bin and cannot be recovered. Make sure this is not data of a program that is currently running.",
    }
}

pub fn tr_protected_path(lang: Language, path: &str) -> String {
    match lang {
        Language::Zh => format!("「{path}」是受保护的系统路径，不能删除"),
        Language::En => format!("\"{path}\" is a protected system path and cannot be deleted"),
    }
}

// ============================================================================
// 顶栏 / 进度条 / 列表里剩下的零散文案
// ============================================================================

pub fn tr_btn_rescan(lang: Language, busy: bool) -> &'static str {
    match (lang, busy) {
        (Language::Zh, true) => "扫描中…",
        (Language::Zh, false) => "重新扫描",
        (Language::En, true) => "Scanning…",
        (Language::En, false) => "Rescan",
    }
}

pub fn tr_btn_refresh_apps(lang: Language, busy: bool) -> &'static str {
    match (lang, busy) {
        (Language::Zh, true) => "读取中…",
        (Language::Zh, false) => "刷新软件列表",
        (Language::En, true) => "Loading…",
        (Language::En, false) => "Refresh apps",
    }
}

pub fn tr_btn_reanalyze_disk(lang: Language, busy: bool) -> &'static str {
    match (lang, busy) {
        (Language::Zh, true) => "扫描中…",
        (Language::Zh, false) => "重新分析磁盘",
        (Language::En, true) => "Scanning…",
        (Language::En, false) => "Re-analyze disk",
    }
}

pub fn tr_elevation_mode(lang: Language, elevated: bool) -> &'static str {
    match (lang, elevated) {
        (Language::Zh, true) => "管理员模式",
        (Language::Zh, false) => "普通模式",
        (Language::En, true) => "Administrator",
        (Language::En, false) => "Standard user",
    }
}

pub fn tr_freed_pill(lang: Language, size: &str) -> String {
    match lang {
        Language::Zh => format!("本次已释放 {size}"),
        Language::En => format!("{size} freed"),
    }
}

pub fn tr_file_count(lang: Language, count: &str) -> String {
    match lang {
        Language::Zh => format!("{count} 个文件"),
        Language::En => format!("{count} files"),
    }
}

pub fn tr_file_progress(lang: Language, done: &str, total: &str) -> String {
    match lang {
        Language::Zh => format!("{done} / {total} 个文件"),
        Language::En => format!("{done} / {total} files"),
    }
}

pub fn tr_clean_phase(lang: Language, cancelling: bool) -> &'static str {
    match (lang, cancelling) {
        (Language::Zh, true) => "正在停止…",
        (Language::Zh, false) => "正在永久删除",
        (Language::En, true) => "Stopping…",
        (Language::En, false) => "Deleting permanently",
    }
}

pub fn tr_failed_count(lang: Language, count: &str) -> String {
    match lang {
        Language::Zh => format!("失败 {count}"),
        Language::En => format!("{count} failed"),
    }
}

pub fn tr_btn_stop(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "停止",
        Language::En => "Stop",
    }
}

pub fn tr_category_empty(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "此类别未发现可清理内容",
        Language::En => "Nothing cleanable found in this category",
    }
}

pub fn tr_last_clean_skipped(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("上次清理有 {count} 处项目被占用或受系统保护而跳过"),
        Language::En => {
            format!("{count} items were skipped last time — in use or system-protected")
        }
    }
}

pub fn tr_toggle_details(lang: Language, expanded: bool) -> &'static str {
    match (lang, expanded) {
        (Language::Zh, true) => "收起详情 ▴",
        (Language::Zh, false) => "查看详情 ▾",
        (Language::En, true) => "Hide details ▴",
        (Language::En, false) => "Show details ▾",
    }
}

pub fn tr_volume_root(lang: Language, vol: &crate::core::disk::VolumeId) -> String {
    match lang {
        Language::Zh => format!("{vol}: 根目录"),
        Language::En => format!("{vol}: root"),
    }
}

pub fn tr_space_breakdown(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "空间占比分布",
        Language::En => "Space breakdown",
    }
}

pub fn tr_top_n_categories(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("Top {count} 分类"),
        Language::En => format!("Top {count} categories"),
    }
}

pub fn tr_status_install_path(lang: Language, path: &str) -> String {
    match lang {
        Language::Zh => format!("已获取安装路径：{path}"),
        Language::En => format!("Install path: {path}"),
    }
}

pub fn tr_status_no_install_path(lang: Language, name: &str) -> String {
    match lang {
        Language::Zh => format!("软件「{name}」无独立安装路径"),
        Language::En => format!("\"{name}\" has no dedicated install directory"),
    }
}

/// 扫描失败原因。
///
/// `ScanError` 自己的 `Display` 是给 `mftscan` 命令行和日志用的（固定中文），
/// 界面上要跟随语言，所以在这里单独翻一份。
pub fn tr_scan_error(lang: Language, err: &crate::core::disk::ScanError) -> String {
    use crate::core::disk::ScanError;
    match (lang, err) {
        (Language::Zh, ScanError::AccessDenied) => "需要管理员权限才能读取 $MFT".into(),
        (Language::Zh, ScanError::NotNtfs) => "该卷不是 NTFS 或无法获取卷信息".into(),
        (Language::Zh, ScanError::Io(e)) => format!("读取失败：{e}"),
        (Language::En, ScanError::AccessDenied) => {
            "Administrator rights are required to read $MFT".into()
        }
        (Language::En, ScanError::NotNtfs) => {
            "Not an NTFS volume, or volume info is unavailable".into()
        }
        (Language::En, ScanError::Io(e)) => format!("Read failed: {e}"),
    }
}

/// 第一阶段扫完的状态：结果已经能用了，第二阶段还在后台跑。
pub fn tr_status_scan_fixed_done(lang: Language, total: &str) -> String {
    match lang {
        Language::Zh => format!("系统垃圾扫描完成，共 {total}；正在后台检索项目构建产物…"),
        Language::En => {
            format!("System junk scanned — {total}; still looking for build artifacts…")
        }
    }
}

/// 开发者类目在第二阶段跑完之前显示的占位。
pub fn tr_discovering(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "检索中…",
        Language::En => "Scanning…",
    }
}

/// 磁盘清理条上的「删除到回收站」开关。
pub fn tr_recycle_toggle(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "删除到回收站",
        Language::En => "Delete to Recycle Bin",
    }
}

/// 开关关闭时，右侧体积那行的说明：这些空间真的会被释放。
pub fn tr_to_be_freed(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "待彻底释放",
        Language::En => "To be freed",
    }
}

/// 开关打开时的说明。
///
/// 必须说清楚「不释放空间」——回收站里的文件还占着原来的簇，用户删完
/// 发现可用容量纹丝不动会以为程序没生效。
pub fn tr_to_be_recycled(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "移入回收站（暂不释放空间）",
        Language::En => "Moved to Recycle Bin (space not freed yet)",
    }
}

// ---------------------------------------------------------------------------
// macOS 完全磁盘访问权限（Full Disk Access）引导
// ---------------------------------------------------------------------------

pub fn tr_fda_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "授予「完全磁盘访问权限」",
        Language::En => "Grant Full Disk Access",
    }
}

pub fn tr_fda_desc(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "macOS 系统安全机制限制了对 Safari 缓存、邮件等系统目录的读取。开启完全磁盘访问权限后，QuickCleaner 可以进行更彻底的深度扫描与清理。",
        Language::En => "macOS privacy protections restrict access to Safari caches, Mail, and system data. Full Disk Access enables QuickCleaner to perform a comprehensive deep clean.",
    }
}

pub fn tr_fda_step1_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "1. 打开系统设置",
        Language::En => "1. Open System Settings",
    }
}

pub fn tr_fda_step1_desc(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "点击下方按钮，将自动直达「隐私与安全性 → 完全磁盘访问」页面",
        Language::En => {
            "Click the button below to navigate directly to Privacy & Security → Full Disk Access"
        }
    }
}

pub fn tr_fda_step2_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "2. 找到 QuickCleaner",
        Language::En => "2. Locate QuickCleaner",
    }
}

pub fn tr_fda_step2_desc(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "在应用列表中找到「QuickCleaner」（若不在列表中可点击「+」添加）",
        Language::En => "Find \"QuickCleaner\" in the application list (or click \"+\" to add it)",
    }
}

pub fn tr_fda_step3_title(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "3. 开启权限开关",
        Language::En => "3. Enable the Switch",
    }
}

pub fn tr_fda_step3_desc(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "开启开关后返回本软件，点击「检查授权状态」即可完成配置",
        Language::En => "Toggle the switch on, return here, and click \"Check Status\" to finish",
    }
}

pub fn tr_fda_notice(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "💡 即使暂不授权，您仍可正常清理第三方应用缓存和开发构建产物；开启后将解锁 Apple 自带应用与系统垃圾的深度清理。",
        Language::En => "💡 Even without authorization, you can still clean third-party caches and dev builds. Full access unlocks deep cleaning for Apple apps and system data.",
    }
}

pub fn tr_fda_btn_open_settings(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "一键打开系统设置",
        Language::En => "Open System Settings",
    }
}

pub fn tr_fda_btn_check(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "检查授权状态",
        Language::En => "Check Status",
    }
}

pub fn tr_fda_btn_later(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "稍后配置",
        Language::En => "Configure Later",
    }
}

pub fn tr_fda_dont_ask(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "不再自动弹出提示",
        Language::En => "Don't show automatically again",
    }
}

pub fn tr_fda_status_granted(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "完全磁盘访问",
        Language::En => "Full Disk Access",
    }
}

pub fn tr_fda_status_limited(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "磁盘受限 (点击配置)",
        Language::En => "Limited Access (Configure)",
    }
}

pub fn tr_fda_check_success(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "已成功获得完全磁盘访问权限！",
        Language::En => "Full Disk Access granted successfully!",
    }
}

pub fn tr_fda_check_failed(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "未检测到完全磁盘访问权限。若您已在系统设置中开启，请尝试重启本应用生效。",
        Language::En => "Full Disk Access not detected yet. If enabled in System Settings, try restarting the app.",
    }
}

// ---- 文件搜索 ----

pub fn tr_search_heading(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "文件搜索",
        Language::En => "File Search",
    }
}

pub fn tr_search_subheading(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "全盘索引，秒级检索任意文件",
        Language::En => "Full-disk index, instant file search",
    }
}

pub fn tr_file_search_placeholder(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "输入文件名关键词，支持 * 和 ? 通配符…",
        Language::En => "Type a filename (supports * and ? wildcards)…",
    }
}

pub fn tr_search_results(lang: Language, count: usize) -> String {
    match lang {
        Language::Zh => format!("{count} 条结果"),
        Language::En => format!("{count} result{}", if count == 1 { "" } else { "s" }),
    }
}

pub fn tr_search_empty(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "输入关键词开始搜索",
        Language::En => "Type to start searching",
    }
}

pub fn tr_search_no_results(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "未找到匹配文件",
        Language::En => "No matching files found",
    }
}

pub fn tr_search_indexing(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "正在构建全盘搜索索引…",
        Language::En => "Building full-disk search index…",
    }
}

pub fn tr_search_ready(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "搜索索引就绪",
        Language::En => "Search index ready",
    }
}

pub fn tr_search_no_index(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "无法构建搜索索引",
        Language::En => "Failed to build search index",
    }
}

pub fn tr_search_need_admin(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "文件搜索需要管理员权限才能读取 $MFT，请以管理员身份重新运行",
        Language::En => {
            "File search requires administrator privileges to read $MFT. Please relaunch as admin"
        }
    }
}

pub fn tr_search_open_in_explorer(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "在文件管理器中打开",
        Language::En => "Open in file manager",
    }
}

pub fn tr_search_col_name(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "名称",
        Language::En => "Name",
    }
}

pub fn tr_search_col_path(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "路径",
        Language::En => "Path",
    }
}

pub fn tr_search_col_size(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "大小",
        Language::En => "Size",
    }
}

pub fn tr_search_building_index(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "正在构建搜索索引，完成后即可搜索…",
        Language::En => "Building search index, search will be available when done…",
    }
}

pub fn tr_search_col_kind(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "类型",
        Language::En => "Type",
    }
}

pub fn tr_search_sort_kind(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "按类型聚合",
        Language::En => "By Type",
    }
}
