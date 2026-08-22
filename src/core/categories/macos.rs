//! macOS 专属：损坏登录项、APFS 本地快照、外接卷废纸篓、Group Containers、.DS_Store

use super::{target, target_with_recommendation, ScanTarget};
use crate::core::categories::CategoryId;
use crate::core::i18n::Text;
use crate::core::model::snapshot_path;
use std::path::{Path, PathBuf};

/// macOS 专属清理目标
#[cfg(target_os = "macos")]
pub(super) fn push_macos_targets(t: &mut Vec<ScanTarget>, home: &Path) {
    // APFS 本地快照：通过 `tmutil listlocalsnapshots /` 发现，
    // 用 `tmutil deletelocalsnapshots <date>` 删除。
    // 这里只做发现，实际清理在 cleaner 模块用 `tmutil` 执行。
    push_local_snapshots(t);

    // 其他卷的废纸篓：外接盘上的 `.Trashes/<uid>/`。
    // 本机废纸篓 `~/.Trash` 上面已加，但每个外接卷都有自己的 `.Trashes`，
    // 删到外接盘的文件不会出现在 `~/.Trash` 里。
    push_external_volume_trashes(t);

    // Group Containers 下的缓存、临时文件、日志
    // 沙盒应用共享的容器目录，很多应用在这里堆缓存。
    push_group_container_caches(t, home);

    // .DS_Store 文件清理（限定常见目录，不做全盘扫描）
    push_dsstore_targets(t, home);

    // 遗留 LaunchAgent：仅收配置无效或绝对执行路径已经不存在的 plist。
    // 清理时走废纸篓而非永久删除，系统级条目会由 Finder 请求授权。
    push_broken_login_items(t, home);

    // /Applications/JetBrains 下的旧版 IDE 安装（发现式枚举，但目标形态
    // 固定，不走 devscan 管线）。清理侧有独立的窄口子放行，见
    // safety::is_old_ide_install_target。
    push_old_ide_targets(t);
}

/// `/Applications/JetBrains` 下的旧版 IDE 安装。
///
/// JetBrains 的版本化安装天然落在 `/Applications` 这个受保护前缀下——
/// 那层保护是给磁盘透镜手选路径兜底的，不能整体放开。所以这里只做
/// **发现**：按产品分组、每组只保留最新版本，其余列给用户勾选；能否
/// 真正删除由 `clean_targets` 的专属分支经
/// [`crate::core::safety::is_old_ide_install_target`] 重新校验，处置送
/// 废纸篓。解析不出产品+版本的目录（Daemon、Toolbox）不列入。
#[cfg(target_os = "macos")]
pub(super) fn push_old_ide_targets(t: &mut Vec<ScanTarget>) {
    const BASE: &str = "/Applications/JetBrains";
    let Ok(entries) = std::fs::read_dir(BASE) else {
        return;
    };
    let names: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    for (name, family, version) in select_old_versions(&names) {
        t.push(target(
            PathBuf::from(BASE).join(&name),
            Text::new(
                format!("{family} {}.{}（旧版本）", version.0, version.1),
                format!("{family} {}.{} (old version)", version.0, version.1),
            ),
            CategoryId::OldIdeInstall,
        ));
    }
}

/// 从目录名列表里挑出「同产品已有更高版本」的旧版本。
///
/// 独立成纯函数以便单测；实际磁盘枚举在调用方。O(n²) 不在乎——
/// JetBrains 目录下通常只有个位数条目。
#[cfg(target_os = "macos")]
fn select_old_versions(names: &[String]) -> Vec<(String, &'static str, (u32, u32))> {
    let parsed: Vec<(usize, &'static str, (u32, u32))> = names
        .iter()
        .enumerate()
        .filter_map(|(i, n)| {
            crate::core::safety::parse_jetbrains_versioned_name(n).map(|(f, v)| (i, f, v))
        })
        .collect();
    let mut out = Vec::new();
    for &(i, family, version) in &parsed {
        let has_newer = parsed
            .iter()
            .any(|&(_, f2, v2)| f2 == family && v2 > version);
        if has_newer {
            out.push((names[i].clone(), family, version));
        }
    }
    out
}

#[cfg(target_os = "macos")]
pub(super) fn push_broken_login_items(t: &mut Vec<ScanTarget>, home: &Path) {
    for root in [
        home.join("Library/LaunchAgents"),
        PathBuf::from("/Library/LaunchAgents"),
    ] {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "plist")
                || !entry.file_type().is_ok_and(|kind| kind.is_file())
                // 应用更新时执行文件与 plist 可能短暂不同步。至少持续一天
                // 才视为遗留项，避免恰好在安装/更新窗口里误报。
                || !super::helpers::is_older_than(&path, std::time::Duration::from_secs(24 * 60 * 60))
                || !super::helpers::is_broken_launch_agent(&path)
            {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            t.push(target(
                path,
                Text::new(
                    format!("损坏的登录项 · {name}"),
                    format!("Broken login item · {name}"),
                ),
                CategoryId::BrokenLoginItems,
            ));
        }
    }
}

/// 发现 APFS 本地快照。
///
/// `tmutil listlocalsnapshots /` 输出形如：
/// ```text
/// com.apple.TimeMachine.2024-01-15-123456
/// ```
/// 每个快照用一个虚拟路径 `tmutil://snapshot/<name>` 表示，
/// scanner 对这种路径走 `tmutil` 而不是文件系统枚举。
/// 实际大小无法直接获取（APFS 快照是 COW 的，共享数据块），
/// 这里用 0 占位，UI 展示时标注「快照」即可。
#[cfg(target_os = "macos")]
pub(super) fn push_local_snapshots(t: &mut Vec<ScanTarget>) {
    let output = std::process::Command::new("tmutil")
        .arg("listlocalsnapshots")
        .arg("/")
        .output();
    let Ok(out) = output else {
        return;
    };
    if !out.status.success() {
        return;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    for name in parse_snapshot_names(&stdout) {
        // 虚拟路径：scanner 跳过称重，cleaner 路由到 tmutil
        let virtual_path = snapshot_path(&name);
        t.push(target(
            virtual_path,
            Text::new(
                format!("本地快照 · {name}"),
                format!("Local snapshot · {name}"),
            ),
            CategoryId::LocalSnapshots,
        ));
    }
}

/// 从 `tmutil listlocalsnapshots` 的输出里挑出快照名。
///
/// 输出首行是表头（`Snapshots for disk (/):`，措辞随 macOS 版本变过），
/// 不是快照名——不过滤会造出无效目标，清理时 tmutil 对它报错、白白计入
/// 失败数。快照名永远是单个 token（形如
/// `com.apple.TimeMachine.2026-08-23-101010.local`），用「不含任何空白」
/// 甄别，不硬编码表头文本。
#[cfg(target_os = "macos")]
fn parse_snapshot_names(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.contains(char::is_whitespace))
        .map(str::to_string)
        .collect()
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::parse_snapshot_names;

    #[test]
    fn snapshot_names_skip_header_and_blank_lines() {
        let out = "Snapshots for disk (/):\n\
                   com.apple.TimeMachine.2026-08-23-101010.local\n\
                   \n\
                   com.apple.TimeMachine.2026-08-24-020202.local\n";
        assert_eq!(
            parse_snapshot_names(out),
            vec![
                "com.apple.TimeMachine.2026-08-23-101010.local".to_string(),
                "com.apple.TimeMachine.2026-08-24-020202.local".to_string(),
            ]
        );
    }

    #[test]
    fn snapshot_names_empty_output_yields_nothing() {
        assert!(parse_snapshot_names("").is_empty());
    }
}

/// 外接卷的废纸篓：`/Volumes/<volume>/.Trashes/<uid>/`。
///
/// macOS 上每个卷有自己的 `.Trashes` 目录，里面按 uid 分子目录。
/// 删到外接盘的文件不会出现在 `~/.Trash` 里，只在该卷的 `.Trashes/<uid>` 下。
/// 根卷 `/` 的废纸篓就是 `~/.Trash`，上面已加，这里只处理 `/Volumes` 下的外接盘。
#[cfg(target_os = "macos")]
pub(super) fn push_external_volume_trashes(t: &mut Vec<ScanTarget>) {
    let uid = unsafe { libc::getuid() };
    let Ok(volumes) = std::fs::read_dir("/Volumes") else {
        return;
    };
    for entry in volumes.flatten() {
        let vol_path = entry.path();
        let trashes = vol_path.join(".Trashes");
        if !trashes.is_dir() {
            continue;
        }
        let user_trash = trashes.join(uid.to_string());
        if !user_trash.is_dir() {
            continue;
        }
        let vol_name = vol_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| vol_path.display().to_string());
        t.push(target(
            user_trash,
            Text::new(
                format!("废纸篓 · {vol_name}"),
                format!("Trash · {vol_name}"),
            ),
            CategoryId::RecycleBin,
        ));
    }
}

/// `~/Library/Group Containers` 下的缓存、临时文件和日志。
///
/// 沙盒应用通过 App Group 共享数据，Group Containers 下也有 Caches / tmp / Logs。
/// 跳过包含密码管理器关键词的目录（1Password、Keychain 等）。
#[cfg(target_os = "macos")]
pub(super) fn push_group_container_caches(t: &mut Vec<ScanTarget>, home: &Path) {
    let group_root = home.join("Library/Group Containers");
    let Ok(rd) = std::fs::read_dir(&group_root) else {
        return;
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        // 跳过密码管理器和敏感应用
        let sensitive = [
            "1password",
            "keychain",
            "bitwarden",
            "lastpass",
            "keepass",
            "dashlane",
            "enpass",
        ];
        if sensitive.iter().any(|s| name.contains(s)) {
            continue;
        }
        let group_dir = entry.path();
        if !group_dir.is_dir() {
            continue;
        }
        // Caches 子目录
        let caches = group_dir.join("Library/Caches");
        if caches.is_dir() {
            // App Group 标识经常不含产品名，靠关键词黑名单无法可靠识别
            // 密码管理器、同步工具等敏感应用，因此只展示、不默认清理。
            t.push(target_with_recommendation(
                caches,
                Text::new(
                    format!("组容器缓存 · {}", entry.file_name().to_string_lossy()),
                    format!(
                        "Group Container Cache · {}",
                        entry.file_name().to_string_lossy()
                    ),
                ),
                CategoryId::UserTemp,
                false,
            ));
        }
        // tmp 子目录
        let tmp = group_dir.join("Library/tmp");
        if tmp.is_dir() {
            t.push(target(
                tmp,
                Text::new(
                    format!("组容器临时 · {}", entry.file_name().to_string_lossy()),
                    format!(
                        "Group Container Temp · {}",
                        entry.file_name().to_string_lossy()
                    ),
                ),
                CategoryId::UserTemp,
            ));
        }
        // Logs 子目录
        let logs = group_dir.join("Library/Logs");
        if logs.is_dir() {
            t.push(target(
                logs,
                Text::new(
                    format!("组容器日志 · {}", entry.file_name().to_string_lossy()),
                    format!(
                        "Group Container Logs · {}",
                        entry.file_name().to_string_lossy()
                    ),
                ),
                CategoryId::Logs,
            ));
        }
    }
}

/// `.DS_Store` 文件清理。
///
/// `.DS_Store` 是 Finder 自动生成的目录元数据文件，删除后 Finder 会重新生成。
/// 不做全盘扫描（太慢），只扫常见目录：桌面、文档、下载、用户根目录、Applications。
#[cfg(target_os = "macos")]
pub(super) fn push_dsstore_targets(t: &mut Vec<ScanTarget>, home: &Path) {
    let scan_dirs: &[&str] = &[
        "Desktop",
        "Documents",
        "Downloads",
        "Movies",
        "Music",
        "Pictures",
        "Public",
    ];
    for dir in scan_dirs {
        let path = home.join(dir);
        if !path.is_dir() {
            continue;
        }
        // 扫描该目录（仅一层）下的 .DS_Store 文件
        let Ok(rd) = std::fs::read_dir(&path) else {
            continue;
        };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".DS_Store" {
                t.push(target(
                    entry.path(),
                    Text::new(
                        format!(".DS_Store · ~/{dir}"),
                        format!(".DS_Store · ~/{dir}"),
                    ),
                    CategoryId::UserCache,
                ));
            }
            // 子目录里的 .DS_Store（只下一层，不做深度遍历）
            if entry.file_type().is_ok_and(|ft| ft.is_dir()) {
                let sub_ds = entry.path().join(".DS_Store");
                if sub_ds.is_file() {
                    t.push(target(
                        sub_ds,
                        Text::new(
                            format!(".DS_Store · ~/{dir}/{name}"),
                            format!(".DS_Store · ~/{dir}/{name}"),
                        ),
                        CategoryId::UserCache,
                    ));
                }
            }
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod old_ide_tests {
    use super::select_old_versions;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// 每个产品只报旧版本：最新版、无版本目录、单版本产品都不列入。
    #[test]
    fn only_superseded_versions_are_listed() {
        let out = select_old_versions(&names(&[
            "IntelliJIDEA2026.2",
            "IntelliJIDEA2025.2",
            "IntelliJIDEA2024.3",
            "PyCharm2024.3",
            "Daemon",
            "Toolbox",
        ]));
        // 两个旧版 IntelliJ 都该列出，最新的 2026.2 与解析不出的不算
        assert_eq!(out.len(), 2);
        assert!(out
            .iter()
            .any(|(n, f, v)| n == "IntelliJIDEA2025.2" && *f == "IntelliJ IDEA" && *v == (2025, 2)));
        assert!(out
            .iter()
            .any(|(n, _, _)| n == "IntelliJIDEA2024.3"));
        // 只有一个版本的产品（PyCharm2024.3）没有更新版本，不列
        assert!(!out.iter().any(|(n, _, _)| n.starts_with("PyCharm")));
    }

    /// 版本比较要按 (年, 次版本) 语义来：2024.10 > 2024.9，
    /// 字符串比较会得出相反结论。
    #[test]
    fn versions_compare_numerically() {
        let out = select_old_versions(&names(&["WebStorm2024.9", "WebStorm2024.10"]));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "WebStorm2024.9");
    }

    /// 目录不存在或为空时什么都不报（push_old_ide_targets 的
    /// read_dir 失败路径），这里用空列表覆盖同一逻辑入口。
    #[test]
    fn empty_listing_is_noop() {
        assert!(select_old_versions(&[]).is_empty());
        assert!(select_old_versions(&names(&["Daemon", "Toolbox"])).is_empty());
    }
}
