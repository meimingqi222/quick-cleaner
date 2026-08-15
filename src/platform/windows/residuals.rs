//! Windows 软件残留（文件与注册表）深度探测与清理

use crate::core::apps::{is_safe_app_token, AppRegRoot, InstalledApp, ResidualKind, ResidualScanResult};
use crate::core::cleaner::{clean_path, CleanProgress, CleanReport};
use crate::core::safety::{is_protected_residual_path, is_system_root_dir};
use crate::platform::windows::apps::dir_or_file_size;
use crate::platform::windows::registry::{delete_reg_tree, to_wide};
use std::path::PathBuf;

use winapi::shared::minwindef::{DWORD, HKEY};
use winapi::shared::winerror::ERROR_SUCCESS;
use winapi::um::winnt::{KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
use winapi::um::winreg::{
    RegCloseKey, RegOpenKeyExW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
};

fn sanitize_token(s: &str) -> String {
    s.trim()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == ' ')
        .collect::<String>()
        .trim()
        .to_string()
}

fn reg_key_exists(root: HKEY, subpath: &str, sam: DWORD) -> bool {
    let wide = to_wide(subpath);
    let mut h_key: HKEY = std::ptr::null_mut();
    unsafe {
        if RegOpenKeyExW(root, wide.as_ptr(), 0, sam, &mut h_key) as u32 == ERROR_SUCCESS {
            RegCloseKey(h_key);
            true
        } else {
            false
        }
    }
}

/// 扫描指定软件在磁盘与注册表中的残留项
pub fn scan_residuals(app: &InstalledApp) -> ResidualScanResult {
    let mut items = Vec::new();
    let mut total_file_size = 0u64;

    let app_clean_name = sanitize_token(&app.name);
    let pub_clean_name = sanitize_token(&app.publisher);

    // 1. 检查 InstallLocation 目录残留
    if let Some(loc) = &app.install_location {
        if loc.exists() && !is_system_root_dir(loc) {
            let size = dir_or_file_size(loc);
            total_file_size += size;
            if loc.is_dir() {
                items.push(ResidualKind::Directory(loc.clone(), size));
            } else {
                items.push(ResidualKind::File(loc.clone(), size));
            }
        }
    }

    // 2. 检查常见应用数据残留目录 (%AppData%, %LocalAppData%, %ProgramData%)
    if is_safe_app_token(&app_clean_name) {
        let mut check_roots = Vec::new();
        if let Some(appdata) = dirs::data_dir() {
            check_roots.push(appdata);
        }
        if let Some(local_appdata) = dirs::data_local_dir() {
            check_roots.push(local_appdata);
        }
        if let Ok(programdata) = std::env::var("ProgramData") {
            check_roots.push(PathBuf::from(programdata));
        }

        for root_dir in check_roots {
            // 直接匹配: <Root>/<AppName>
            let direct = root_dir.join(&app_clean_name);
            if direct.exists() && !is_protected_residual_path(&direct) {
                let size = dir_or_file_size(&direct);
                total_file_size += size;
                items.push(ResidualKind::Directory(direct, size));
            }

            // 厂商子目录匹配: <Root>/<Publisher>/<AppName>
            if is_safe_app_token(&pub_clean_name) {
                let pub_child = root_dir.join(&pub_clean_name).join(&app_clean_name);
                if pub_child.exists() && !is_protected_residual_path(&pub_child) {
                    let size = dir_or_file_size(&pub_child);
                    total_file_size += size;
                    items.push(ResidualKind::Directory(pub_child, size));
                }
            }
        }
    }

    // 3. 检查注册表残留项 (HKCU & HKLM Software)
    {
        if is_safe_app_token(&app_clean_name) {
            let reg_targets = [
                (AppRegRoot::Hkcu, HKEY_CURRENT_USER, r"Software", KEY_READ),
                (AppRegRoot::Hklm, HKEY_LOCAL_MACHINE, r"SOFTWARE", KEY_READ | KEY_WOW64_64KEY),
                (AppRegRoot::Hklm32, HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node", KEY_READ | KEY_WOW64_32KEY),
            ];

            for (reg_root, h_root, base_path, sam) in reg_targets {
                let direct_sub = format!("{base_path}\\{app_clean_name}");
                if reg_key_exists(h_root, &direct_sub, sam) {
                    items.push(ResidualKind::RegistryKey(reg_root, direct_sub));
                }

                if is_safe_app_token(&pub_clean_name) {
                    let pub_sub = format!("{base_path}\\{pub_clean_name}\\{app_clean_name}");
                    if reg_key_exists(h_root, &pub_sub, sam) {
                        items.push(ResidualKind::RegistryKey(reg_root, pub_sub));
                    }
                }
            }
        }
    }

    dedup_items(&mut items);

    ResidualScanResult {
        app_name: app.name.clone(),
        items,
        total_file_size,
    }
}

/// 去重：`Vec::dedup` 只能消掉相邻重复项，而这里的 items 并非有序，
/// 需要按内容做一次全量去重（数量很小，线性即可）。
fn dedup_items(items: &mut Vec<ResidualKind>) {
    let mut seen: Vec<ResidualKind> = Vec::with_capacity(items.len());
    items.retain(|it| {
        if seen.contains(it) {
            false
        } else {
            seen.push(it.clone());
            true
        }
    });
}

/// 执行残留清理
pub fn clean_residuals(
    items: &[ResidualKind],
    prog: &CleanProgress,
) -> CleanReport {
    let mut report = CleanReport::default();

    for item in items {
        if prog.cancelled() {
            break;
        }

        match item {
            ResidualKind::Directory(path, _) | ResidualKind::File(path, _) => {
                prog.note(path);
                let res = clean_path(path, prog);
                report.record(path, res);
            }
            ResidualKind::RegistryKey(root, subpath) => {
                {
                    let h_root = match root {
                        AppRegRoot::Hkcu => HKEY_CURRENT_USER,
                        AppRegRoot::Hklm | AppRegRoot::Hklm32 | AppRegRoot::SystemApp => HKEY_LOCAL_MACHINE,
                    };
                    let ok = delete_reg_tree(h_root, subpath);
                    if ok {
                        report.ok += 1;
                    } else {
                        report.failed.push(PathBuf::from(format!("{}\\{}", root.label(), subpath)));
                    }
                }
            }
        }
    }

    report
}
