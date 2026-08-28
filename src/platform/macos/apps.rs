//! macOS 已安装应用程序 (.app Bundle) 枚举与卸载支持
//!
//! M5 实现：
//! - 从 `Info.plist` 读取 `CFBundleIdentifier`、`CFBundleShortVersionString`、应用名
//! - 枚举 `/Applications`、`/System/Applications`、`~/Applications`、`/Applications/Utilities`
//! - 系统自带应用标记为 `is_system_component`，不可卸载
//! - 卸载优先调用应用自带的卸载程序（`Contents/Resources/*.app/Uninstall *.app`），
//!   没有则移入废纸篓

use crate::core::apps::{AppRegRoot, InstalledApp};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// 枚举 macOS 系统中已安装的 .app 应用程序包
///
/// 扫描目录顺序：用户应用 → 系统应用 → 用户级 Applications → Utilities。
/// 系统应用（`/System/Applications`）标记为 `is_system_component`，不可卸载。
pub fn list_installed_apps(live: &AtomicBool) -> Vec<InstalledApp> {
    let home = dirs::home_dir();
    let app_dirs: Vec<(PathBuf, bool)> = vec![
        (PathBuf::from("/Applications"), false),
        (PathBuf::from("/System/Applications"), true),
        (PathBuf::from("/Applications/Utilities"), false),
        // 用户级 Applications 目录（部分用户会在这里装 app）
        home.as_ref()
            .map(|h| (h.join("Applications"), false))
            .unwrap_or((PathBuf::new(), false)),
    ];

    // 先只枚举 bundle 路径。后面的 plist、Spotlight 和体积计算可以并行，
    // 避免串行处理一百多个大型 .app 把启动时间线性拉长。
    let mut paths = Vec::new();
    for (root, is_system) in &app_dirs {
        if root.as_os_str().is_empty() || !live.load(Ordering::Relaxed) {
            continue;
        }
        if let Ok(rd) = std::fs::read_dir(root) {
            paths.extend(rd.flatten().filter_map(|entry| {
                let path = entry.path();
                (path.is_dir() && path.extension().is_some_and(|ext| ext == "app"))
                    .then_some((path, *is_system))
            }));
        }
    }

    // 一次 mdls 调用批量读取所有应用的最后使用时间，避免每个应用各启动
    // 一个 Spotlight 子进程。Spotlight 没有索引的应用会保留 None。
    let last_used = query_last_used_dates(&paths);
    // `du` 一次计算所有 bundle 的实际占用，避免 157 个应用各自启动
    // WalkDir；缺失项才回退到 Rust 遍历。
    let bundle_sizes = query_bundle_sizes(&paths);
    let mut apps: Vec<InstalledApp> = paths
        .par_iter()
        .filter(|_| live.load(Ordering::Relaxed))
        .filter_map(|(path, is_system)| {
            let used = last_used.get(path).cloned().unwrap_or((None, 0));
            let size = bundle_sizes
                .get(path)
                .copied()
                .unwrap_or_else(|| dir_size(path));
            parse_app_bundle(path, *is_system, used, size)
        })
        .collect();

    // 按名称排序，方便 UI 展示
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

/// 从 .app bundle 的 `Info.plist` 读取元数据。
fn parse_app_bundle(
    path: &Path,
    is_system: bool,
    (last_used_date, last_used_raw): (Option<String>, u64),
    size: u64,
) -> Option<InstalledApp> {
    let name = path.file_stem()?.to_string_lossy().to_string();
    let (install_date, install_date_raw) = app_install_date(path);

    // 读取 Info.plist
    let info_plist = path.join("Contents").join("Info.plist");
    let (bundle_id, version) = if info_plist.exists() {
        read_info_plist(&info_plist)
    } else {
        (None, None)
    };

    // bundle identifier 作为唯一 ID，回退到应用名
    let id = bundle_id.clone().unwrap_or_else(|| name.clone());

    // 查找卸载程序：部分应用在 Resources 目录下有 Uninstall .app
    let uninstaller = find_uninstaller(path);

    Some(InstalledApp {
        id,
        name: name.clone(),
        version: version.unwrap_or_default(),
        publisher: if is_system {
            String::from("Apple")
        } else {
            String::from("macOS Application")
        },
        last_used_date,
        last_used_raw,
        install_date,
        install_date_raw,
        install_location: Some(path.to_path_buf()),
        display_icon: None,
        uninstall_string: uninstaller
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        quiet_uninstall_string: None,
        estimated_size: size,
        registry_root: if is_system {
            AppRegRoot::SystemApp
        } else {
            AppRegRoot::Hkcu
        },
        registry_subpath: bundle_id.unwrap_or_default(),
        is_system_component: is_system,
        uninstaller_missing: uninstaller.is_none(),
    })
}

/// 一次性获取所有 bundle 的实际磁盘占用。
///
/// `du -sk` 使用 macOS 文件系统的分配块口径，和清理工具展示的「可释放空间」
/// 一致。路径作为独立参数传入，不经过 shell，因此不会产生命令注入问题。
fn query_bundle_sizes(paths: &[(PathBuf, bool)]) -> HashMap<PathBuf, u64> {
    let mut result = HashMap::new();
    if paths.is_empty() {
        return result;
    }

    let mut command = std::process::Command::new("du");
    command.arg("-sk");
    for (path, _) in paths {
        command.arg(path);
    }
    let Ok(output) = command.output() else {
        return result;
    };
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((size, path)) = line.split_once('\t') else {
            continue;
        };
        let Ok(kib) = size.trim().parse::<u64>() else {
            continue;
        };
        result.insert(PathBuf::from(path), kib.saturating_mul(1024));
    }
    result
}

/// 批量读取应用的最后使用时间。
///
/// 策略：
/// 1. 优先用 `mdls -raw -name kMDItemLastUsedDate` 批量查 Spotlight 元数据。
///    Spotlight 只对通过 LaunchServices/Finder 启动的应用更新此字段。
/// 2. 对 Spotlight 返回 `(null)` 或缺失的应用，回退到检查用户数据目录的
///    mtime：`~/Library/Application Support/<bid>/`、`~/Library/Preferences/<bid>.plist`、
///    `~/Library/Caches/<bid>/`、`~/Library/Saved Application State/<bid>.savedState`、
///    `~/Library/HTTPStorages/<bid>`、`~/Library/WebKit/<bid>`、
///    `~/Library/Containers/<bid>`（沙盒应用）。
///    取所有可用信号中最新的 mtime 作为最后使用时间。
fn query_last_used_dates(paths: &[(PathBuf, bool)]) -> HashMap<PathBuf, (Option<String>, u64)> {
    let mut result = HashMap::new();
    if paths.is_empty() {
        return result;
    }

    // 阶段一：Spotlight kMDItemLastUsedDate
    let mut command = std::process::Command::new("mdls");
    command.arg("-raw").arg("-name").arg("kMDItemLastUsedDate");
    for (path, _) in paths {
        command.arg(path);
    }
    let Ok(output) = command.output() else {
        return result;
    };

    // mdls 即使部分应用未索引也会返回 NUL 分隔的值，exit code 可能为 0。
    // 但如果值数量不匹配，放弃 Spotlight 结果，全部走回退。
    let spotlight_values: Vec<String> = if output.status.success() {
        output
            .stdout
            .split(|byte| *byte == 0)
            .map(|value| String::from_utf8_lossy(value).trim().to_string())
            .collect()
    } else {
        Vec::new()
    };

    // 同时查 bundle identifier，回退阶段需要
    let bundle_ids = query_bundle_ids(paths);

    if spotlight_values.len() == paths.len() {
        for ((path, _), value) in paths.iter().zip(&spotlight_values) {
            if let Some(date) = parse_metadata_date(value) {
                result.insert(path.clone(), date);
            }
        }
    }

    // 阶段二：对 Spotlight 没给到日期的应用，用用户数据目录 mtime 回退
    let home = dirs::home_dir();
    if let Some(home) = &home {
        for (path, _) in paths {
            // 已有 Spotlight 日期的跳过
            if result.contains_key(path) {
                continue;
            }
            let bid = bundle_ids.get(path);
            if let Some(ts) = bid.and_then(|b| fallback_last_used(home, b)) {
                if ts > 0 {
                    let dt = chrono::DateTime::from_timestamp(ts as i64, 0).unwrap_or_default();
                    let date_str = dt.format("%Y-%m-%d").to_string();
                    result.insert(path.clone(), (Some(date_str), ts));
                }
            }
        }
    }

    result
}

/// 批量查 bundle identifier，用于回退阶段定位用户数据目录。
fn query_bundle_ids(paths: &[(PathBuf, bool)]) -> HashMap<PathBuf, String> {
    let mut result = HashMap::new();
    if paths.is_empty() {
        return result;
    }
    let mut command = std::process::Command::new("mdls");
    command
        .arg("-raw")
        .arg("-name")
        .arg("kMDItemCFBundleIdentifier");
    for (path, _) in paths {
        command.arg(path);
    }
    let Ok(output) = command.output() else {
        return result;
    };
    if !output.status.success() {
        return result;
    }
    let values: Vec<String> = output
        .stdout
        .split(|byte| *byte == 0)
        .map(|value| String::from_utf8_lossy(value).trim().to_string())
        .collect();
    if values.len() != paths.len() {
        return result;
    }
    for ((path, _), value) in paths.iter().zip(values) {
        let v = value.trim();
        if !v.is_empty() && v != "(null)" {
            result.insert(path.clone(), v.to_string());
        }
    }
    result
}

/// 回退策略：检查应用的用户数据目录 mtime，取最新的作为最后使用时间。
///
/// 应用每次启动都会更新 Preferences plist、Caches 或 Application Support 目录的
/// mtime。即使 Spotlight 没有记录 kMDItemLastUsedDate（CLI 启动、索引未更新等），
/// 这些目录的 mtime 也能反映最近使用时间。
fn fallback_last_used(home: &Path, bundle_id: &str) -> Option<u64> {
    let candidates = [
        home.join("Library/Application Support").join(bundle_id),
        home.join("Library/Preferences")
            .join(format!("{bundle_id}.plist")),
        home.join("Library/Caches").join(bundle_id),
        home.join("Library/Saved Application State")
            .join(format!("{bundle_id}.savedState")),
        home.join("Library/HTTPStorages").join(bundle_id),
        home.join("Library/WebKit").join(bundle_id),
        // 沙盒应用的数据在 Containers 下
        home.join("Library/Containers").join(bundle_id),
    ];

    let mut best: Option<u64> = None;
    for path in &candidates {
        if let Ok(meta) = std::fs::metadata(path) {
            if let Ok(mtime) = meta.modified() {
                if let Ok(ts) = mtime.duration_since(std::time::UNIX_EPOCH) {
                    let secs = ts.as_secs();
                    if best.is_none_or(|b| secs > b) {
                        best = Some(secs);
                    }
                }
            }
        }
    }
    best
}

/// 把 Spotlight 日期转成 UI 日期和排序用的 Unix 秒数。
fn parse_metadata_date(raw: &str) -> Option<(Option<String>, u64)> {
    let value = raw.trim();
    if value.is_empty() || value == "(null)" {
        return None;
    }

    let parsed = chrono::DateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S %z")
        .or_else(|_| chrono::DateTime::parse_from_rfc3339(value))
        .ok()?;
    let timestamp = parsed.timestamp();
    (timestamp > 0).then(|| {
        (
            Some(parsed.format("%Y-%m-%d").to_string()),
            timestamp as u64,
        )
    })
}

/// 以 `.app` bundle 目录的 birth time 作为安装时间。
///
/// macOS 没有类似 Windows Uninstall 注册表的统一安装日期。bundle 目录的
/// creation time 是安装/复制应用时最接近的可靠值；若文件系统不提供 birth time，
/// 回退到 modification time。
fn app_install_date(path: &Path) -> (Option<String>, u64) {
    let Ok(metadata) = std::fs::metadata(path) else {
        return (None, 0);
    };
    let time = metadata.created().or_else(|_| metadata.modified()).ok();
    time.map(format_system_time).unwrap_or((None, 0))
}

fn format_system_time(time: std::time::SystemTime) -> (Option<String>, u64) {
    let timestamp = time
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if timestamp == 0 {
        return (None, 0);
    }
    let date = chrono::DateTime::<chrono::Local>::from(time)
        .format("%Y-%m-%d")
        .to_string();
    (Some(date), timestamp)
}

/// 从 `Info.plist` 读取 `CFBundleIdentifier` 和 `CFBundleShortVersionString`。
///
/// 用 `defaults read` 命令而不是引入 `plist` crate——`defaults` 在所有
/// macOS 上都有，且能处理二进制和 XML 两种格式。
pub(crate) fn read_info_plist(plist_path: &Path) -> (Option<String>, Option<String>) {
    let Ok(output) = std::process::Command::new("defaults")
        .arg("read")
        .arg(plist_path)
        .output()
    else {
        return (None, None);
    };
    if !output.status.success() {
        return (None, None);
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let bundle_id = defaults_value(&text, "CFBundleIdentifier");
    let version = defaults_value(&text, "CFBundleShortVersionString");
    (bundle_id, version)
}

/// 从 `defaults read` 的字典输出中提取简单的字符串字段。
fn defaults_value(text: &str, key: &str) -> Option<String> {
    let prefix = format!("{key} = ");
    text.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(&prefix))
        .map(|value| {
            value
                .trim_end_matches(';')
                .trim()
                .trim_matches('"')
                .to_string()
        })
        .filter(|value| !value.is_empty())
}

/// 查找应用自带的卸载程序。
///
/// 部分应用（如 Adobe、VMware）在 `Contents/Resources/` 下有独立的
/// `Uninstall *.app`。找到则返回其路径，让 UI 优先调用它。
fn find_uninstaller(app_path: &Path) -> Option<PathBuf> {
    let resources = app_path.join("Contents").join("Resources");
    if let Ok(entries) = std::fs::read_dir(&resources) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.extension().is_some_and(|ext| ext == "app") {
                let name = path.file_stem()?.to_string_lossy().to_lowercase();
                if name.starts_with("uninstall") {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// 在访达中定位指定路径
pub fn reveal_in_explorer(path: &Path) {
    if !path.exists() {
        return;
    }
    let _ = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn();
}

/// 使用系统默认程序打开文件或目录
pub fn open_in_default_app(path: &Path) {
    if !path.exists() {
        return;
    }
    let _ = std::process::Command::new("open").arg(path).spawn();
}

/// 目录的**实际磁盘占用**（不是逻辑大小）。
///
/// APFS 有透明压缩、clone 和稀疏文件，`metadata().len()`（逻辑大小）会显著
/// 高于删掉它真正能释放的空间——实测 `~/Library` 逻辑 157 GB、实际占用只有
/// 86 GB，差了近一倍。清理工具承诺的是「能释放多少」，所以必须按分配块算。
pub(crate) fn dir_size(path: &Path) -> u64 {
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| alloc_size(&m))
        .sum()
}

/// 单个文件的实际占用：`st_blocks` 以 512 字节为单位，与 `du` 的口径一致。
pub(crate) fn alloc_size(m: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    m.blocks() * 512
}

/// macOS 上「卸载」优先调用应用自带的卸载程序，没有则移入废纸篓。
///
/// 系统自带的 App 在密封只读的系统卷上，删不掉也不该删，直接挡回去。
pub fn run_uninstaller_and_wait(app: &InstalledApp) -> Result<(), String> {
    let Some(loc) = &app.install_location else {
        return Err("未找到应用程序路径".into());
    };
    if !loc.exists() {
        return Err("应用程序路径已不存在".into());
    }
    if is_system_app(loc) {
        return Err("系统自带应用位于只读的系统卷上，无法卸载".into());
    }

    // 优先调用应用自带的卸载程序
    if let Some(uninstaller) = &app.uninstall_string {
        let uninstaller_path = PathBuf::from(uninstaller);
        if uninstaller_path.exists() {
            let status = std::process::Command::new("open")
                .arg("--wait-apps")
                .arg(&uninstaller_path)
                .status();
            match status {
                Ok(exit) if exit.success() && wait_until_removed(loc) => return Ok(()),
                Ok(exit) if exit.success() => {
                    return Err(format!(
                        "自带卸载程序已退出，但应用仍然存在：{}",
                        loc.display()
                    ));
                }
                Ok(exit) => {
                    return Err(format!("自带卸载程序退出异常：{exit}"));
                }
                Err(error) => {
                    return Err(format!("无法启动自带卸载程序：{error}"));
                }
            }
        }
    }

    // 回退：移入废纸篓
    super::trash::move_to_trash(loc)?;
    if loc.exists() {
        Err(format!("卸载后应用程序仍然存在：{}", loc.display()))
    } else {
        Ok(())
    }
}

/// 官方卸载器有时会在主窗口退出前后才完成最后一次 bundle 移除，给文件系统
/// 一个很短的收敛窗口，避免刚退出就误判并再次送废纸篓。
fn wait_until_removed(path: &Path) -> bool {
    for _ in 0..50 {
        if !path.exists() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    !path.exists()
}

/// 是否位于系统卷（SSV 密封只读）。
fn is_system_app(path: &Path) -> bool {
    path.starts_with("/System/")
}

/// `mdfind` 反查最多等多久。
///
/// Spotlight 正在重建索引时 `mdfind` 可以挂很久，而这个调用在「用户点了
/// 清理残留正在等」的路径上。2 秒是「正常情况下绰绰有余、异常情况下不让
/// 用户干等」的折中：本机正常返回在 100ms 量级。
const MDFIND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// 这个 bundle id 对应的应用，是不是还装在这台机器上的某个位置？
///
/// 这是残留清理的最后一道判据。残留扫描的整个前提是「这个 app 已经被卸载
/// 了」，一旦这个前提是错的——用户把应用挪到了别的目录、或者卸载器其实没
/// 卸干净、或者同一个 bundle id 还有另一份安装——那么"残留"就是活应用的
/// 配置、登录态和许可证，删掉不可逆。
///
/// 三态，语义必须严格区分：
/// - `Some(true)`：Spotlight 明确报告还有这个 bundle id 的安装 → **不许删**
/// - `Some(false)`：`mdfind` 正常跑完、明确返回空 → 可以删
/// - `None`：**测不出**（超时、命令起不来、非零退出）→ 按 `Some(true)`
///   一样处理，不许删
///
/// `None` 必须 fail closed，理由是 Mole 踩过的坑：Spotlight 卡顿或索引不
/// 全的时候，`mdfind` 会「正常返回一个空结果」，看起来跟"确实没装"一模
/// 一样。
///
/// **退出码在这里靠不住**（本机实测）：`mdfind` 连查询串语法错误都返回
/// exit 0，错误信息写到 stderr、stdout 留空。只看「exit 0 且 stdout 空」
/// 就会把一次根本没执行的查询判成"应用已卸载"，然后放行删除——这正是要
/// 防的那类 fail-open。所以判据是**三个条件同时成立**才算"确实没装"：
/// 退出码为 0、stderr 为空、stdout 为空。
///
/// 结论**不缓存**：同理，一次「没装」的判断不能拿去复用，Spotlight 的索
/// 引状态随时会变，而缓存会把一次偶然的空结果固化成后续所有删除的依据。
/// 拼 `mdfind` 的查询串。
///
/// bundle id 用双引号包住，内部的反斜杠与引号都要转义。id 理论上不含这些
/// 字符，但查询串是拼出来的——不转义就等于把外部输入直接塞进查询语法，
/// 一个精心构造的 id 可以闭合引号再接上别的条件，把查询变成"匹配一切"或
/// 者干脆让它语法错误。转义顺序必须是**先反斜杠后引号**，反过来会把刚插
/// 入的转义反斜杠又转义一遍。
fn mdfind_bundle_query(bundle_id: &str) -> String {
    let escaped = bundle_id.replace('\\', "\\\\").replace('"', "\\\"");
    format!("kMDItemCFBundleIdentifier == \"{escaped}\"")
}

pub fn bundle_is_still_installed(bundle_id: &str) -> Option<bool> {
    if bundle_id.trim().is_empty() {
        // 没有 bundle id 就无从查证 → 测不出，交给调用方 fail closed。
        return None;
    }
    let query = mdfind_bundle_query(bundle_id);
    let run = crate::core::proc::run_with_timeout("/usr/bin/mdfind", &[&query], MDFIND_TIMEOUT)?;
    if !run.ok {
        return None;
    }
    if run.stderr.iter().any(|b| !b.is_ascii_whitespace()) {
        // 查询根本没跑起来（语法错误、Spotlight 服务异常）。退出码是 0，
        // 但这不是一个"没找到"的结论。
        return None;
    }
    // stdout 有内容 = Spotlight 报告了至少一个安装位置 = 还装着。
    // 这里**不能取反**：函数名和调用方约定的都是"是否仍然安装"。
    if run.stdout.iter().any(|b| !b.is_ascii_whitespace()) {
        return Some(true);
    }

    // 走到这里意味着「查不到 = 可以删」，是唯一会授权删除的分支，所以
    // 多付一次哨兵查询来证明索引本身是活的。
    //
    // 需要这一步的原因，是调试这个函数时实测到的现象：Spotlight 索引被
    // 关掉或尚未覆盖某个位置时，`mdfind` 对每个查询都干净地返回 exit 0 +
    // 空 stdout + 空 stderr——和"这个应用确实卸载了"一模一样。没有哨兵的
    // 话，一台关了索引的机器上这个检查会对**所有**应用返回"已卸载"，整道
    // 防线静默失效，而且失效方向是放行删除。
    //
    // 哨兵查的是"这台机器上有没有任何应用包"。任何一台 macOS 都必然有一
    // 堆，查不到就只可能是索引不可用，此时返回"测不出"让调用方 fail closed。
    if !spotlight_index_is_usable() {
        return None;
    }
    Some(false)
}

/// Spotlight 索引现在能不能用？
///
/// 判据是一条在任何 macOS 上都必定有结果的查询：机器上存在应用包。返回
/// `false` 只说明"索引此刻答不了问题"，不说明机器上没有应用。
fn spotlight_index_is_usable() -> bool {
    let Some(run) = crate::core::proc::run_with_timeout(
        "/usr/bin/mdfind",
        &[
            "-count",
            "kMDItemContentType == \"com.apple.application-bundle\"",
        ],
        MDFIND_TIMEOUT,
    ) else {
        return false;
    };
    if !run.ok || run.stderr.iter().any(|b| !b.is_ascii_whitespace()) {
        return false;
    }
    String::from_utf8_lossy(&run.stdout)
        .trim()
        .parse::<u64>()
        .is_ok_and(|n| n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_apps_finds_something() {
        let live = AtomicBool::new(true);
        let apps = list_installed_apps(&live);
        // 至少应该能找到一些系统应用
        assert!(!apps.is_empty(), "应当能枚举到至少一个应用");
        // 至少有一个是系统组件
        assert!(
            apps.iter().any(|a| a.is_system_component),
            "应当至少有一个系统自带应用"
        );
        assert!(
            apps.iter().any(|a| a.install_date.is_some()),
            "应用目录应当至少有一个可用的创建日期"
        );
        assert!(
            apps.iter().any(|a| a.id.contains('.')),
            "Info.plist 应当至少解析出一个 bundle identifier"
        );
    }

    #[test]
    fn system_app_detection() {
        assert!(is_system_app(Path::new(
            "/System/Applications/Calculator.app"
        )));
        assert!(!is_system_app(Path::new("/Applications/MyApp.app")));
    }

    #[test]
    fn metadata_date_parser_handles_spotlight_values() {
        let parsed = parse_metadata_date("2026-08-14 05:59:44 +0000").expect("日期应可解析");
        assert_eq!(parsed.0.as_deref(), Some("2026-08-14"));
        assert!(parsed.1 > 0);
        assert!(parse_metadata_date("(null)").is_none());
    }

    /// 空 bundle id 无从查证 → 必须是"测不出"，不能是"没装"。
    #[test]
    fn empty_bundle_id_is_unknown() {
        assert_eq!(bundle_is_still_installed(""), None);
        assert_eq!(bundle_is_still_installed("   "), None);
    }

    /// 机器上确实装着的应用必须被认出来。Safari 在系统卷上，任何 macOS
    /// 都有，且在 `/Applications` 下（Spotlight 默认覆盖的位置）。
    ///
    /// 这条**故意写成硬断言**而不是"查不到就跳过"：查不到恰恰意味着这道
    /// 防线在这台机器上失效了，那是需要看到的失败，不是可以忽略的环境差异。
    #[test]
    fn installed_bundle_id_is_detected() {
        assert_eq!(
            bundle_is_still_installed("com.apple.Safari"),
            Some(true),
            "Safari 装在 /Applications 下却没被认出来——这道防线已失效"
        );
    }

    /// 不存在的 bundle id 必须得到明确的"没装"，否则残留清理永远被拦住。
    #[test]
    fn unknown_bundle_id_reports_gone() {
        assert_eq!(
            bundle_is_still_installed("com.quickcleaner.definitely-not-installed-xyz"),
            Some(false)
        );
    }

    /// 索引哨兵在一台正常的机器上必须报可用——否则上面那条"没装"的结论
    /// 会被 fail closed 挡掉，残留清理功能整体不可用。
    #[test]
    fn spotlight_sentinel_reports_usable_on_a_normal_mac() {
        assert!(
            spotlight_index_is_usable(),
            "哨兵查询报告 Spotlight 索引不可用"
        );
    }

    /// 查询串构造必须把 id 完整转义掉，任何 id 都不能改变查询的结构。
    ///
    /// 这条防的是查询注入：一个含引号的 id 若不转义，可以闭合字符串再接
    /// 上别的条件，把"查这一个 bundle"变成"匹配一切"（于是永远判定还装着，
    /// 残留功能失效）或者语法错误（`mdfind` 对语法错误也返回 exit 0，
    /// 见 `bundle_is_still_installed` 的文档）。
    #[test]
    fn query_escapes_quotes_and_backslashes() {
        assert_eq!(
            mdfind_bundle_query("com.example.App"),
            "kMDItemCFBundleIdentifier == \"com.example.App\""
        );
        // 引号被转义，没有提前闭合
        assert_eq!(
            mdfind_bundle_query("a\"b"),
            "kMDItemCFBundleIdentifier == \"a\\\"b\""
        );
        // 反斜杠先转义，不会把后面引号的转义符再吃掉一层
        assert_eq!(
            mdfind_bundle_query("a\\b"),
            "kMDItemCFBundleIdentifier == \"a\\\\b\""
        );
        // 注入尝试：闭合引号后接 OR 条件——转义后整体仍是一个字符串字面量
        let injected = mdfind_bundle_query("x\" || kMDItemFSName == \"");
        assert!(
            !injected.contains("|| kMDItemFSName == \"\""),
            "注入片段不该以裸语法出现：{injected}"
        );
    }


}
