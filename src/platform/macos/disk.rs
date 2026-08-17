//! macOS 磁盘与权限查询

use crate::core::disk::{ScanError, ScanResult, VolumeId};
use std::path::PathBuf;

extern "C" {
    fn geteuid() -> u32;
}

/// macOS 上「已提权」等价于 euid == 0。
///
/// 直接声明 libc 符号，避免只为一个调用引入 `libc` 依赖。
pub fn is_elevated() -> bool {
    unsafe { geteuid() == 0 }
}

/// macOS 上可供分析的物理磁盘。
///
/// 现代 macOS（Catalina+）把系统卷和数据卷分开挂载：
/// - `/` 是合成根目录，指向只读系统卷
/// - `/Volumes/Macintosh HD` 是数据卷，用户实际文件在这里
/// - 同一 APFS 容器内还有 Preboot/Recovery/VM/Update/.hidden 等系统卷
///
/// 这里用 `statfs` 的 `f_mntfromname` 按物理盘分组，同一物理盘合并
/// 成一项：挂载点优先用 `/`（能扫完整系统），标签用数据卷名称
///（如 "Macintosh HD"）。
pub fn list_volumes() -> Vec<VolumeId> {
    // (mount, total, label) 每个设备保留最该显示的一项
    let mut by_device: std::collections::HashMap<String, (Option<PathBuf>, PathBuf, u64, String)> =
        std::collections::HashMap::new();

    // 先处理 `/` 根卷，找到它的底层设备
    if let Ok(root_cpath) = std::ffi::CString::new("/") {
        let mut buf: libc::statfs = unsafe { std::mem::zeroed() };
        // SAFETY: "/" 是合法路径
        if unsafe { libc::statfs(root_cpath.as_ptr(), &mut buf) } == 0 {
            let dev = unsafe {
                std::ffi::CStr::from_ptr(buf.f_mntfromname.as_ptr() as *const libc::c_char)
            }
            .to_string_lossy()
            .into_owned();
            let dev_key = dev
                .rsplit_once('s')
                .filter(|(_, suffix)| suffix.chars().all(|c| c.is_ascii_digit()))
                .map(|(prefix, _)| prefix.to_string())
                .unwrap_or(dev);
            let block = buf.f_bsize as u64;
            let total = buf.f_blocks * block;
            by_device.insert(
                dev_key,
                (
                    Some(PathBuf::from("/")),
                    PathBuf::from("/"),
                    total,
                    String::new(),
                ),
            );
        }
    }

    // 收集 /Volumes 下的候选
    if let Ok(entries) = std::fs::read_dir("/Volumes") {
        for entry in entries.flatten() {
            let path = entry.path();
            // 跳过 `.DS_Store` 等隐藏/非目录文件
            if !path.is_dir() {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            if name.starts_with('.') || name.contains(".hidden") {
                continue;
            }

            let c_path = match std::ffi::CString::new(path.to_string_lossy().as_bytes()) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let mut buf: libc::statfs = unsafe { std::mem::zeroed() };
            // SAFETY: c_path 是合法的 NUL 结尾字符串
            if unsafe { libc::statfs(c_path.as_ptr(), &mut buf) } != 0 {
                continue;
            }

            // 只纳入 APFS 和 HFS+ 卷
            let fstype = unsafe {
                std::ffi::CStr::from_ptr(buf.f_fstypename.as_ptr() as *const libc::c_char)
            };
            let fstype_str = fstype.to_string_lossy();
            if fstype_str != "apfs" && fstype_str != "hfs" {
                continue;
            }

            // 读出底层设备名，去掉 `s1`、`s2` 等 APFS 子卷后缀作为物理盘 key
            let dev = unsafe {
                std::ffi::CStr::from_ptr(buf.f_mntfromname.as_ptr() as *const libc::c_char)
            }
            .to_string_lossy()
            .into_owned();
            let dev_key = dev
                .rsplit_once('s')
                .filter(|(_, suffix)| suffix.chars().all(|c| c.is_ascii_digit()))
                .map(|(prefix, _)| prefix.to_string())
                .unwrap_or(dev);

            let block = buf.f_bsize as u64;
            let total = buf.f_blocks * block;

            // 同一物理盘：优先用 `/` 做挂载点，标签用数据卷名字
            by_device
                .entry(dev_key)
                .and_modify(|(_root, _, t, label)| {
                    // 更新标签为更有意义的卷名（只要名字不空就替换空标签）
                    if !name.is_empty() && (label.is_empty() || name.len() > label.len()) {
                        *label = name.clone();
                    }
                    // 同一容器容量相同，保存总量即可
                    *t = total;
                })
                .or_insert((None, path, total, name));
        }
    }

    // 取出每个物理盘，按容量降序排列
    let mut physical: Vec<(PathBuf, u64, String)> = by_device
        .into_values()
        .map(|(root, fallback, total, label)| {
            // 如果有根挂载点 `/`，优先用它，否则用 /Volumes 下的卷
            let mount = root.unwrap_or(fallback);
            let display = if label.is_empty() {
                mount.display().to_string()
            } else {
                label
            };
            (mount, total, display)
        })
        .collect();
    physical.sort_by_key(|b| std::cmp::Reverse(b.1));

    let mut out = Vec::with_capacity(physical.len());
    for (mount, _, name) in physical {
        let label = if name.len() > 22 {
            format!("{}…", &name[..22])
        } else {
            name
        };
        out.push(VolumeId::from_mount_point_with_label(mount, label));
    }

    out
}

/// 整树空间分析：用并行 `getattrlistbulk` 遍历器扫描指定卷的根目录。
///
/// macOS 上没有 NTFS `$MFT` 那样的在线快速枚举结构，因此走 M2 实现的
/// 并行目录遍历器（`platform::macos::walk`）。实测 8 线程在 `~/Library`
/// （约 92 万文件）上约 12 秒，比 `du -sh`（44 秒）快 3 倍以上。
///
/// `top_n` 参数在 macOS 上不使用（目录排行榜仅 Windows 命令行工具需要）。
pub fn scan_volume(vol: &VolumeId, _top_n: usize) -> Result<ScanResult, ScanError> {
    let root = vol.mount_point();
    let live = std::sync::atomic::AtomicBool::new(true);
    let result = crate::platform::macos::walk::scan_root(root, vol.clone(), &live)?;

    // 缓存扫描结果（M4 缓存层）
    let top_dirs: Vec<crate::platform::macos::cache::CachedDir> = result
        .tree
        .children(result.tree.root())
        .iter()
        .take(100)
        .map(|n| crate::platform::macos::cache::CachedDir {
            name: n.name.clone(),
            size: n.size,
            file_count: n.file_count,
        })
        .collect();
    let cache = crate::platform::macos::cache::ScanCache::from_scan(vol, &result, top_dirs);
    cache.save();

    Ok(result)
}

/// 卷的（总容量, 可用容量），单位字节。
///
/// 走 `statfs(2)`。注意 APFS 的容器内多个卷共享空间，`f_blocks` / `f_bavail`
/// 报的是**整个容器**的数字，所以对 `/` 和 `/System/Volumes/Data` 查询会得到
/// 相同的结果——聚合多个卷时绝不能相加。`list_volumes` 只返回根卷 `/` 和
/// `/Volumes` 下的外接盘，不会重复列出 APFS 容器卷，因此 UI 层直接显示即可。
///
/// 另外 `f_bavail` 不含 purgeable（本地快照、可重新下载的内容），会比
/// 「关于本机 → 储存空间」显示的可用量略小。要完全对齐得用
/// `NSURLVolumeAvailableCapacityForImportantUsageKey`，那是后续的事。
pub fn get_volume_space(vol: &VolumeId) -> Option<(u64, u64)> {
    let mount = vol.mount_point();
    let c_path = std::ffi::CString::new(mount.to_string_lossy().as_bytes()).ok()?;

    // SAFETY: c_path 是合法的 NUL 结尾字符串，buf 在调用期间独占且大小正确。
    let mut buf: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c_path.as_ptr(), &mut buf) } != 0 {
        return None;
    }

    let block = buf.f_bsize as u64;
    Some((buf.f_blocks * block, buf.f_bavail * block))
}

pub fn relaunch_as_admin_if_needed() -> bool {
    true
}

/// macOS 的界面语言：按 POSIX 环境变量判断。
///
/// 更准的做法是读 `AppleLanguages` 用户默认值，但那要拉 Objective-C 运行时；
/// 环境变量在终端启动和 Finder 启动下都够用，何况这只是首次启动的默认值，
/// 用户切一次就会被 `core::settings` 记住。
pub fn detect_system_language() -> crate::core::i18n::Language {
    crate::core::i18n::Language::from_locale_tag(&crate::platform::posix_locale_tag())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `statfs` 至少要能拿到自洽的数字：容量为正、可用不超过容量。
    #[test]
    fn volume_space_is_plausible() {
        let vol = VolumeId::from_mount_point(std::path::PathBuf::from("/"));
        let (total, free) = get_volume_space(&vol).expect("statfs(\"/\") 应当成功");
        assert!(total > 0, "容量为 0");
        assert!(free <= total, "可用 {free} 超过了容量 {total}");
    }

    /// `list_volumes` 至少返回根卷 `/`。
    #[test]
    fn list_volumes_includes_root() {
        let vols = list_volumes();
        assert!(!vols.is_empty(), "至少应该返回根卷");
        assert_eq!(
            vols[0].mount_point(),
            std::path::Path::new("/"),
            "第一个卷应该是根卷"
        );
    }

    /// `scan_volume` 不再返回 `NotNtfs`，而是真正扫描。
    /// 用临时目录测试，避免扫描整个 `/` 太慢。
    ///
    /// `scan_volume` 完成后会无条件 `cache.save()`，必须把 `QUICKCLEANER_CACHE_DIR`
    /// 指向临时目录，否则跑一次 `cargo test` 就把用户真实的 `scan-cache.json`
    /// 冲掉。用 `isolate_cache_dir` 拿到 guard，保证环境变量不会被并行的
    /// `cache_round_trip` 用例踩掉。
    #[test]
    fn scan_volume_does_not_return_not_ntfs() {
        let _guard = crate::platform::macos::cache::isolate_cache_dir(
            "scan_volume_does_not_return_not_ntfs",
        );

        let tmp = std::env::temp_dir();
        let vol = VolumeId::from_mount_point(tmp.clone());
        let result = scan_volume(&vol, 0);
        // 扫描可能因为权限问题部分失败，但不应返回 NotNtfs
        assert!(
            !matches!(result, Err(ScanError::NotNtfs)),
            "scan_volume 不应返回 NotNtfs"
        );
        if let Ok(scan) = result {
            assert!(
                scan.file_count > 0 || scan.dir_count > 0,
                "临时目录不应该是空的"
            );
        }
    }
}
