//! UI 视图层国际化（i18n）文案映射

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
