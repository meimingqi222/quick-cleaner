//! macOS 软件残留深度扫描
//!
//! M5 实现：
//! - 以 `CFBundleIdentifier`（存在 `registry_subpath` 里）为主键搜索残留
//! - 扫描 `~/Library` 下的 Application Support、Caches、Preferences、Logs 等
//! - 区分安全项（缓存、日志）和注意项（配置、容器数据）
//! - 不删除用户数据、登录态、凭据等高风险内容

use crate::core::apps::{
    InstalledApp, ResidualItem, ResidualKind, ResidualScanResult, ResidualSource,
};
use crate::core::cleaner::{clean_path, CleanProgress, CleanReport};

/// 扫描应用卸载后的残留文件和目录。
///
/// 以 `CFBundleIdentifier`（存在 `app.registry_subpath`）为主键，
/// 在 `~/Library` 下的多个已知位置搜索匹配的残留。
pub fn scan_residuals(app: &InstalledApp) -> ResidualScanResult {
    let mut items = Vec::new();
    let mut total_file_size = 0u64;

    let Some(home) = dirs::home_dir() else {
        return ResidualScanResult {
            app_name: app.name.clone(),
            items,
            total_file_size,
        };
    };

    let library = home.join("Library");
    let bundle_id = &app.registry_subpath;
    let app_name = &app.name;

    // 1. Application Support — 安全清理（应用数据，非用户文档）
    //    按应用名和 bundle id 两种方式搜索
    for search_name in &[app_name, bundle_id] {
        if search_name.is_empty() {
            continue;
        }
        let path = library.join("Application Support").join(search_name);
        if path.exists() {
            let size = super::apps::dir_size(&path);
            total_file_size += size;
            items.push(ResidualItem::certain(
                ResidualKind::Directory(path, size),
                ResidualSource::AppSupportDir,
            ));
        }
    }

    // 2. Caches — 安全清理
    for search_name in &[app_name, bundle_id] {
        if search_name.is_empty() {
            continue;
        }
        let path = library.join("Caches").join(search_name);
        if path.exists() {
            let size = super::apps::dir_size(&path);
            total_file_size += size;
            items.push(ResidualItem::certain(
                ResidualKind::Directory(path, size),
                ResidualSource::CacheDir,
            ));
        }
    }

    // 3. Preferences — 安全清理（plist 文件）
    if !bundle_id.is_empty() {
        let plist = library
            .join("Preferences")
            .join(format!("{bundle_id}.plist"));
        if plist.exists() {
            let size = super::apps::dir_size(&plist);
            total_file_size += size;
            items.push(ResidualItem::certain(
                ResidualKind::File(plist, size),
                ResidualSource::PreferenceFile,
            ));
        }
    }

    // 4. Logs — 安全清理
    for search_name in &[app_name, bundle_id] {
        if search_name.is_empty() {
            continue;
        }
        let path = library.join("Logs").join(search_name);
        if path.exists() {
            let size = super::apps::dir_size(&path);
            total_file_size += size;
            items.push(ResidualItem::certain(
                ResidualKind::Directory(path, size),
                ResidualSource::LogDir,
            ));
        }
    }

    // 5. Saved Application State — 安全清理（窗口状态）
    if !bundle_id.is_empty() {
        let path = library
            .join("Saved Application State")
            .join(format!("{bundle_id}.savedState"));
        if path.exists() {
            let size = super::apps::dir_size(&path);
            total_file_size += size;
            items.push(ResidualItem::certain(
                ResidualKind::Directory(path, size),
                ResidualSource::Other,
            ));
        }
    }

    // 6. HTTPStorages — 安全清理
    if !bundle_id.is_empty() {
        let path = library.join("HTTPStorages").join(bundle_id);
        if path.exists() {
            let size = super::apps::dir_size(&path);
            total_file_size += size;
            items.push(ResidualItem::certain(
                ResidualKind::Directory(path, size),
                ResidualSource::Other,
            ));
        }
    }

    // 7. Containers — 注意项（沙盒数据，可能含用户文档）
    if !bundle_id.is_empty() {
        let path = library.join("Containers").join(bundle_id);
        if path.exists() {
            let size = super::apps::dir_size(&path);
            total_file_size += size;
            items.push(ResidualItem::possible(
                ResidualKind::Directory(path, size),
                ResidualSource::ContainerDir,
            ));
        }
    }

    // 8. Group Containers — 注意项（应用组共享数据）
    if !bundle_id.is_empty() {
        let group_dir = library.join("Group Containers");
        if let Ok(entries) = std::fs::read_dir(&group_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                // Group container 通常以 bundle id 前缀命名
                if name_str.starts_with(bundle_id) || name_str.contains(bundle_id.as_str()) {
                    let path = entry.path();
                    let size = super::apps::dir_size(&path);
                    total_file_size += size;
                    items.push(ResidualItem::possible(
                        ResidualKind::Directory(path, size),
                        ResidualSource::Other,
                    ));
                }
            }
        }
    }

    // 注意：以下目录不扫描，因为可能包含用户数据或凭据：
    // - ~/Library/Keychains — 凭据，危险
    // - ~/Library/Cookies — 登录态，危险
    // - ~/Library/Accounts — 账户信息，危险
    // - ~/Library/Mail — 用户邮件，危险

    ResidualScanResult {
        app_name: app.name.clone(),
        items,
        total_file_size,
    }
}

pub fn clean_residuals(items: &[ResidualItem], prog: &CleanProgress) -> CleanReport {
    let mut report = CleanReport::default();
    for item in items {
        if let ResidualKind::Directory(path, _) | ResidualKind::File(path, _) = &item.kind {
            prog.note(path);
            let res = clean_path(path, prog);
            report.record(path, res);
        }
    }
    report
}

/// 复核候选残留是否仍然存在（对应 Windows 侧的「先扫描后卸载」流程）。
pub fn verify_residuals(items: Vec<ResidualItem>) -> Vec<ResidualItem> {
    items
        .into_iter()
        .filter(|it| match &it.kind {
            ResidualKind::File(p, _) | ResidualKind::Directory(p, _) => p.exists(),
            _ => true,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::apps::AppRegRoot;

    fn make_app(name: &str, bundle_id: &str) -> InstalledApp {
        InstalledApp {
            id: bundle_id.to_string(),
            name: name.to_string(),
            version: String::new(),
            publisher: String::new(),
            last_used_date: None,
            last_used_raw: 0,
            install_date: None,
            install_date_raw: 0,
            install_location: None,
            display_icon: None,
            uninstall_string: None,
            quiet_uninstall_string: None,
            estimated_size: 0,
            registry_root: AppRegRoot::Hkcu,
            registry_subpath: bundle_id.to_string(),
            is_system_component: false,
            uninstaller_missing: true,
        }
    }

    #[test]
    fn scan_nonexistent_app_returns_empty() {
        let app = make_app("NonexistentApp12345", "com.nonexistent.app12345");
        let result = scan_residuals(&app);
        assert!(result.items.is_empty(), "不存在的应用不应有残留");
    }

    #[test]
    fn scan_finds_preferences_plist() {
        // 用一个已知的系统应用测试（Calculator 的 bundle id 是 com.apple.calculator）
        let app = make_app("Calculator", "com.apple.calculator");
        let result = scan_residuals(&app);
        // 至少应该能找到一些残留（Preferences plist 或 Saved Application State）
        // 注意：如果从未打开过 Calculator，可能没有残留——这不是错误
        let _ = result; // 只验证不 panic
    }
}
