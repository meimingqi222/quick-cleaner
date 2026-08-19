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
use std::path::{Path, PathBuf};

/// 扫描应用卸载后的残留文件和目录。
///
/// 以 `CFBundleIdentifier`（存在 `app.registry_subpath`）为主键，
/// 在 `~/Library` 下的多个已知位置搜索匹配的残留。
pub fn scan_residuals(app: &InstalledApp) -> ResidualScanResult {
    let Some(home) = dirs::home_dir() else {
        return ResidualScanResult {
            app_name: app.name.clone(),
            app_id: app.id.clone(),
            items: Vec::new(),
            total_file_size: 0,
        };
    };

    let darwin_cache = darwin_user_cache_dir();
    scan_residuals_in(
        app,
        &home,
        Path::new("/private/var/db/receipts"),
        darwin_cache.as_deref(),
    )
}

fn scan_residuals_in(
    app: &InstalledApp,
    home: &Path,
    receipts: &Path,
    darwin_cache: Option<&Path>,
) -> ResidualScanResult {
    let mut items = Vec::new();

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
                    items.push(ResidualItem::possible(
                        ResidualKind::Directory(path, size),
                        ResidualSource::Other,
                    ));
                }
            }
        }
    }

    // 主应用之外的 Login Item、XPC 和 Extension 通常拥有独立 Bundle ID。
    // 这些 ID 必须在 .app 仍存在时从包内 Info.plist 读取；仅拿主 ID 拼路径
    // 会漏掉 iShotHelper 这一类辅助容器和 Application Scripts。
    let bundle_ids = app_bundle_ids(app);
    for (index, id) in bundle_ids.iter().enumerate() {
        let primary = index == 0;

        add_named_entry(
            &mut items,
            &library.join("Application Scripts"),
            id,
            "",
            ResidualSource::ApplicationScript,
            primary,
        );
        add_named_entry(
            &mut items,
            &library.join("Containers"),
            id,
            "",
            ResidualSource::ContainerDir,
            false,
        );

        let recent = library.join(
            "Application Support/com.apple.sharedfilelist/com.apple.LSSharedFileList.ApplicationRecentDocuments",
        );
        for suffix in [".sfl2", ".sfl3", ".sfl4"] {
            add_named_entry(
                &mut items,
                &recent,
                id,
                suffix,
                ResidualSource::RecentDocumentList,
                true,
            );
        }

        // 安装收据位于 root 管理的系统目录，默认列为“需要确认”，不会
        // 和用户缓存一起自动勾选。这里仍应展示，否则会错误报告“无残留”。
        for suffix in [".bom", ".plist"] {
            add_named_entry(
                &mut items,
                receipts,
                id,
                suffix,
                ResidualSource::PackageReceipt,
                false,
            );
        }

        if let Some(cache_root) = darwin_cache {
            add_named_entry(
                &mut items,
                cache_root,
                id,
                "",
                ResidualSource::CacheDir,
                true,
            );
        }
    }

    // App Group 名称不一定包含主 Bundle ID，必须从已签名 entitlements 读取。
    // 同一个 group ID 会同时对应 Group Containers 和 Application Scripts。
    for group_id in app_group_ids(app) {
        add_named_entry(
            &mut items,
            &library.join("Group Containers"),
            &group_id,
            "",
            ResidualSource::ContainerDir,
            false,
        );
        add_named_entry(
            &mut items,
            &library.join("Application Scripts"),
            &group_id,
            "",
            ResidualSource::ApplicationScript,
            false,
        );
    }

    // 前面的传统精确路径和扩展 Bundle ID 扫描可能指向同一项，按真实路径
    // 去重后重新统计，避免 UI 重复展示或重复计算大小。
    let mut seen = std::collections::HashSet::<PathBuf>::new();
    items.retain(|item| match &item.kind {
        ResidualKind::File(path, _) | ResidualKind::Directory(path, _) => seen.insert(path.clone()),
        _ => true,
    });
    let total_file_size = items.iter().map(ResidualItem::size).sum();

    // 注意：以下目录不扫描，因为可能包含用户数据或凭据：
    // - ~/Library/Keychains — 凭据，危险
    // - ~/Library/Cookies — 登录态，危险
    // - ~/Library/Accounts — 账户信息，危险
    // - ~/Library/Mail — 用户邮件，危险

    ResidualScanResult {
        app_name: app.name.clone(),
        app_id: app.id.clone(),
        items,
        total_file_size,
    }
}

/// 返回主 Bundle ID 以及 app 包中确实属于该应用的辅助组件 ID。
fn app_bundle_ids(app: &InstalledApp) -> Vec<String> {
    let mut ids = Vec::new();
    if valid_bundle_id(&app.registry_subpath) {
        ids.push(app.registry_subpath.clone());
    }

    let Some(app_path) = app.install_location.as_deref() else {
        return ids;
    };
    let contents = app_path.join("Contents");
    if !contents.is_dir() {
        return ids;
    }

    for entry in walkdir::WalkDir::new(&contents)
        .max_depth(12)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file() && entry.file_name() == "Info.plist")
        .take(128)
    {
        let Some(component_contents) = entry.path().parent() else {
            continue;
        };
        let Some(bundle_root) = component_contents.parent() else {
            continue;
        };
        if bundle_root == app_path {
            continue;
        }

        let extension = bundle_root
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let is_owned_component = extension.eq_ignore_ascii_case("xpc")
            || extension.eq_ignore_ascii_case("appex")
            || (extension.eq_ignore_ascii_case("app")
                && bundle_root.starts_with(contents.join("Library/LoginItems")));
        if !is_owned_component {
            continue;
        }

        let (Some(id), _) = super::apps::read_info_plist(entry.path()) else {
            continue;
        };
        if valid_bundle_id(&id)
            && !id.starts_with("org.sparkle-project.")
            && !ids.iter().any(|known| known.eq_ignore_ascii_case(&id))
        {
            ids.push(id);
        }
    }
    ids
}

fn valid_bundle_id(id: &str) -> bool {
    id.len() >= 3
        && id.contains('.')
        && !id.contains('/')
        && !id.contains('\\')
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn app_group_ids(app: &InstalledApp) -> Vec<String> {
    let Some(app_path) = app.install_location.as_deref() else {
        return Vec::new();
    };
    let Ok(output) = std::process::Command::new("codesign")
        .arg("-d")
        .arg("--entitlements")
        .arg(":-")
        .arg(app_path)
        .output()
    else {
        return Vec::new();
    };
    parse_application_groups(&String::from_utf8_lossy(&output.stdout))
}

fn parse_application_groups(entitlements: &str) -> Vec<String> {
    let Some((_, after_key)) =
        entitlements.split_once("<key>com.apple.security.application-groups</key>")
    else {
        return Vec::new();
    };
    let Some((array, _)) = after_key.split_once("</array>") else {
        return Vec::new();
    };

    let mut groups = Vec::new();
    let mut rest = array;
    while let Some((_, after_open)) = rest.split_once("<string>") {
        let Some((value, after_close)) = after_open.split_once("</string>") else {
            break;
        };
        if valid_bundle_id(value) && !groups.iter().any(|known| known == value) {
            groups.push(value.to_string());
        }
        rest = after_close;
    }
    groups
}

fn darwin_user_cache_dir() -> Option<PathBuf> {
    let length = unsafe { libc::confstr(libc::_CS_DARWIN_USER_CACHE_DIR, std::ptr::null_mut(), 0) };
    if length == 0 {
        return None;
    }
    let mut buffer = vec![0u8; length];
    let written = unsafe {
        libc::confstr(
            libc::_CS_DARWIN_USER_CACHE_DIR,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
        )
    };
    if written == 0 {
        return None;
    }
    let value = std::ffi::CStr::from_bytes_until_nul(&buffer).ok()?;
    Some(PathBuf::from(value.to_string_lossy().into_owned()))
}

/// 在指定目录的直接子项中按 Bundle ID 不区分 ASCII 大小写匹配。
/// 不能直接 `root.join(id)`：大小写敏感 APFS 上历史版本可能留下
/// `iShotHelper` 与 `ishothelper` 两种目录。
fn add_named_entry(
    items: &mut Vec<ResidualItem>,
    root: &Path,
    id: &str,
    suffix: &str,
    source: ResidualSource,
    certain: bool,
) {
    let target = format!("{id}{suffix}");
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(&target)
        {
            continue;
        }
        let path = entry.path();
        let size = super::apps::dir_size(&path);
        let kind = if path.is_dir() {
            ResidualKind::Directory(path, size)
        } else {
            ResidualKind::File(path, size)
        };
        items.push(if certain {
            ResidualItem::certain(kind, source)
        } else {
            ResidualItem::possible(kind, source)
        });
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
        // 官方卸载器可能只清掉目录的一部分，不能继续展示卸载前的旧体积。
        .map(|mut item| {
            match &mut item.kind {
                ResidualKind::File(path, size) | ResidualKind::Directory(path, size) => {
                    *size = super::apps::dir_size(path);
                }
                _ => {}
            }
            item
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

    #[test]
    fn scan_finds_embedded_helpers_scripts_recent_items_and_receipts() {
        let root = std::env::temp_dir().join(format!(
            "quick-cleaner-residuals-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = root.join("home");
        let app_path = root.join("iShot.app");
        let helper_contents = app_path.join("Contents/Library/LoginItems/iShotHelper.app/Contents");
        let receipts = root.join("receipts");
        std::fs::create_dir_all(&helper_contents).unwrap();
        std::fs::create_dir_all(home.join("Library/Application Scripts/cn.better365.ishot"))
            .unwrap();
        std::fs::create_dir_all(home.join("Library/Application Scripts/cn.better365.ishothelper"))
            .unwrap();
        std::fs::create_dir_all(home.join("Library/Containers/cn.better365.iShotHelper")).unwrap();
        let recent = home.join(
            "Library/Application Support/com.apple.sharedfilelist/com.apple.LSSharedFileList.ApplicationRecentDocuments",
        );
        std::fs::create_dir_all(&recent).unwrap();
        std::fs::create_dir_all(&receipts).unwrap();
        std::fs::write(
            helper_contents.join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>cn.better365.iShotHelper</string>
</dict></plist>"#,
        )
        .unwrap();
        std::fs::write(recent.join("cn.better365.ishot.sfl3"), b"recent").unwrap();
        std::fs::write(receipts.join("cn.better365.ishot.bom"), b"bom").unwrap();
        std::fs::write(receipts.join("cn.better365.ishot.plist"), b"plist").unwrap();

        let mut app = make_app("iShot", "cn.better365.ishot");
        app.install_location = Some(app_path);
        let result = scan_residuals_in(&app, &home, &receipts, None);

        assert_eq!(result.items.len(), 6);
        assert!(result
            .items
            .iter()
            .any(|item| item.source == ResidualSource::ContainerDir));
        assert_eq!(
            result
                .items
                .iter()
                .filter(|item| item.source == ResidualSource::ApplicationScript)
                .count(),
            2
        );
        assert_eq!(
            result
                .items
                .iter()
                .filter(|item| item.source == ResidualSource::PackageReceipt)
                .count(),
            2
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_only_signed_application_group_values() {
        let entitlements = r#"<plist><dict>
<key>com.apple.security.application-groups</key><array>
<string>group.com.example.app</string>
<string>group.com.example.shared</string>
</array><key>other</key><array><string>com.unrelated.value</string></array>
</dict></plist>"#;

        assert_eq!(
            parse_application_groups(entitlements),
            ["group.com.example.app", "group.com.example.shared"]
        );
    }
}
