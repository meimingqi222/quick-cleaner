//! 应用更新器落在缓存目录里的更新包产物
//!
//! electron-updater 与 Squirrel.Mac 的产物形态极强：目录里就是「下载一次、
//! 装完即废」的安装包，占了体积却没有留下任何用户数据。但按应用名登记追不
//! 上新应用，所以这里改成**探测目录顶层内容**：命中签名的目录按子项拆开，
//! 只有更新包叶子进「应用更新包」并默认勾选，其余子项仍以「分不清」的身份
//! 单独展示。

use super::{target_with_recommendation, ScanTarget};
use crate::core::categories::CategoryId;
use crate::core::i18n::Text;
use std::path::Path;
use std::time::Duration;

/// 更新包叶子要默认勾选，至少得先滞留这么久。
///
/// Squirrel.Mac 换版时把 `update.<随机串>/X.app` 拷去 `/Applications`，
/// electron-updater 装完才回收 `pending/`。刚下完的更新包删了只是让应用重
/// 下一遍，但确实会让「马上要装的更新」倒退；滞留超过这个时长说明那次事务
/// 要么早已完成、要么根本没继续，留在盘上的纯粹是垃圾。
const STALE_AFTER: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// `read_dir` 条目的类型，折叠成判定用得到的三态。
///
/// `Other` 涵盖符号链接：链接指向哪儿看不出来，永远不当更新包产物。
#[derive(Clone, Copy)]
pub(super) enum EntryKind {
    Dir,
    File,
    Other,
}

impl EntryKind {
    fn from_file_type(ft: std::fs::FileType) -> Self {
        if ft.is_symlink() {
            EntryKind::Other
        } else if ft.is_dir() {
            EntryKind::Dir
        } else if ft.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        }
    }
}

/// 顶层条目是不是更新包产物。
///
/// `update.` 前缀只对目录认：Squirrel.Mac 的暂存目录叫
/// `update.5fD8IKe`，随机后缀是它独有的特征，而应用自己的 `update.log`
/// 之类不能算。
fn is_updater_artifact(name: &str, kind: EntryKind) -> bool {
    // 名字表必须**连类型一起**认：同名不同型是典型的伪装——应用自己恰好
    // 叫 `pending` 的文件、指向别处的 `pending` 符号链接都不能算。
    const EXACT_DIRS: &[&str] = &[
        // electron-updater：暂存子目录
        "pending",
    ];
    const EXACT_FILES: &[&str] = &[
        // electron-updater：macOS 通用命名、更新清单
        "update.zip",
        "latest-mac.yml",
        "latest.yml",
        "update-info.json",
        // Squirrel.Mac：换版助手的状态与日志（stderr 是实机看到的名字，
        // 不是 errors）
        "ShipItState.plist",
        "ShipIt_stdout.log",
        "ShipIt_stderr.log",
    ];
    match kind {
        EntryKind::Dir => EXACT_DIRS.contains(&name) || name.starts_with("update."),
        EntryKind::File => {
            EXACT_FILES.contains(&name)
                // electron-updater 的差量块映射（`current.blockmap` 等走后缀）
                || name.ends_with(".blockmap")
        }
        EntryKind::Other => false,
    }
}

/// 目录名 → 展示用产品名：去掉更新器加的后缀，其余原样保留。
pub(super) fn display_stem(name: &str) -> String {
    ["-updater", ".ShipIt"]
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .unwrap_or(name)
        .to_string()
}

/// 探测 `dir` 顶层内容，把其中的更新包产物登记为清理目标。
///
/// 命中时**不**登记 `dir` 本身：`scan_fixed_inner` 逐目标独立称重后相加、
/// 不做嵌套去重，父子同时入表会让展示体积凭空翻倍。父目录剩下的顶层子项
/// 由调用方继续处理（见 `push_residual_children`）。
///
/// 返回是否命中签名。
pub(super) fn push_updater_artifacts(t: &mut Vec<ScanTarget>, dir: &Path, stem: &str) -> bool {
    let Some(children) = top_level(dir) else {
        return false;
    };
    let artifacts: Vec<String> = children
        .into_iter()
        .filter(|(name, kind)| is_updater_artifact(name, *kind))
        .map(|(name, _)| name)
        .collect();
    if artifacts.is_empty() {
        return false;
    }
    for name in artifacts {
        let path = dir.join(&name);
        let stale = super::helpers::is_older_than(&path, STALE_AFTER);
        t.push(target_with_recommendation(
            path,
            Text::same(format!("{stem} · {name}")),
            CategoryId::UpdaterPackages,
            stale,
        ));
    }
    true
}

/// 该目录顶层里**不属于**更新包产物的子项，由调用方作为「分不清」的一项展示。
///
/// 只在 macOS 上用得上：那里 `<app>` 按定义就是应用的缓存命名空间，拆开后
/// 剩下的子项仍然值得单独一览。Windows 的 `%LOCALAPPDATA%/<app>-updater`
/// 不是缓存命名空间，残留就是应用自己的状态，不下钻。
#[cfg(target_os = "macos")]
pub(super) fn residual_children(dir: &Path) -> Vec<String> {
    let Some(children) = top_level(dir) else {
        return Vec::new();
    };
    children
        .into_iter()
        .filter(|(name, kind)| !is_updater_artifact(name, *kind))
        .map(|(name, _)| name)
        .collect()
}

/// 探测 `root` 的**一层**子目录，把其中的更新包产物登记为清理目标。
///
/// 与按应用名列清单的区别正是这轮学到的那条：名字表会腐烂。实测一张 6 个
/// 名字的更新器目录表在本机只剩 1 个命中，而真实存在的 4 个一个都没列到。
/// 展开一层逐个探内容，命中才产出目标，新应用不需要改代码。
///
/// 刻意**不做** `residual_children` 展开：`%LOCALAPPDATA%` 的顶层是各应用的
/// 私有状态目录，不像 `~/Library/Caches` 那样整体算缓存命名空间，展开只会把
/// 界面淹掉。所以这里只在命中签名时产出目标，其余目录保持不可见。
///
/// `cfg(any(windows, test))`：只有 Windows 调用它，但留着测试入口，让这段
/// 平台专用代码能在非 Windows 主机上被类型检查（在临时目录上真跑一遍）。
#[cfg(any(windows, test))]
pub(super) fn push_updater_dirs_under(t: &mut Vec<ScanTarget>, root: &Path) {
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    for entry in rd.flatten() {
        // 只要目录：更新器的暂存产物住在它自己的目录里，顶层散落文件不碰
        if !entry.file_type().is_ok_and(|ft| ft.is_dir()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        push_updater_artifacts(t, &entry.path(), &display_stem(&name));
    }
}

/// 顶层条目 `(名字, 类型)`。目录读不到（权限、竞态删除）返回 `None`。
fn top_level(dir: &Path) -> Option<Vec<(String, EntryKind)>> {
    let rd = std::fs::read_dir(dir).ok()?;
    Some(
        rd.flatten()
            .filter_map(|e| {
                let kind = e.file_type().ok().map(EntryKind::from_file_type)?;
                Some((e.file_name().to_string_lossy().into_owned(), kind))
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::{display_stem, is_updater_artifact, EntryKind};

    /// electron-updater：`pending/` 暂存目录、`update.zip`、差量块映射。
    /// 类型敏感——同名不同型不能误判。
    #[test]
    fn detects_electron_updater_artifacts() {
        assert!(is_updater_artifact("pending", EntryKind::Dir));
        assert!(is_updater_artifact("update.zip", EntryKind::File));
        assert!(is_updater_artifact("current.blockmap", EntryKind::File));
        assert!(is_updater_artifact("latest-mac.yml", EntryKind::File));
        assert!(is_updater_artifact("update-info.json", EntryKind::File));
    }

    /// Squirrel.Mac：随机后缀的暂存目录 + 换版助手的状态文件。
    #[test]
    fn detects_squirrel_mac_artifacts() {
        assert!(is_updater_artifact("update.5fD8IKe", EntryKind::Dir));
        assert!(is_updater_artifact("ShipItState.plist", EntryKind::File));
        assert!(is_updater_artifact("ShipIt_stdout.log", EntryKind::File));
    }

    /// 符号链接一律不认：链接指向的内容无从判断，扫描阶段也会整条丢弃。
    #[test]
    fn symlinks_are_never_artifacts() {
        for name in [
            "pending",
            "update.zip",
            "update.5fD8IKe",
            "current.blockmap",
        ] {
            assert!(
                !is_updater_artifact(name, EntryKind::Other),
                "{name} 作为链接不应被认成更新包"
            );
        }
    }

    /// `update.` 前缀只对目录认，普通应用自己的 `update.log` 不算。
    /// 同理，一个恰好叫 `pending` 的文件不是 electron-updater 的暂存目录。
    #[test]
    fn lookalikes_are_not_artifacts() {
        assert!(!is_updater_artifact("update.log", EntryKind::File));
        assert!(!is_updater_artifact("pending", EntryKind::File));
        assert!(!is_updater_artifact("current.blockmap", EntryKind::Dir));
        assert!(!is_updater_artifact("Cache.db", EntryKind::File));
        assert!(!is_updater_artifact("fsCachedData", EntryKind::Dir));
    }

    #[test]
    fn display_stem_strips_updater_suffixes() {
        assert_eq!(display_stem("@zcodedesktop-updater"), "@zcodedesktop");
        assert_eq!(display_stem("notion.id.ShipIt"), "notion.id");
        assert_eq!(display_stem("TRAE SOLO CN"), "TRAE SOLO CN");
    }

    /// 展开一层逐个探内容：命中签名才产出目标。
    ///
    /// 这条测试同时钉住两件事——名字表之外的新应用能被自动发现（`brand-new-app`
    /// 不在任何表里），以及没命中的目录一个目标都不产出（`%LOCALAPPDATA%`
    /// 顶层是私有状态目录，展开会把界面淹掉）。
    #[test]
    fn one_level_probe_only_emits_matched_dirs() {
        use super::push_updater_dirs_under;
        use crate::core::categories::CategoryId;

        let root = std::env::temp_dir().join(format!("qc_updater_roots_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let hit = root.join("brand-new-app-updater");
        std::fs::create_dir_all(hit.join("pending")).unwrap();
        std::fs::write(hit.join("pending/app.zip"), b"pkg").unwrap();
        std::fs::write(hit.join("update.zip"), b"pkg").unwrap();
        // 未命中：应用的私有状态目录
        std::fs::create_dir_all(root.join("SomeApp/state")).unwrap();
        // 顶层散落文件：连探都不该探
        std::fs::write(root.join("loose.txt"), b"x").unwrap();

        let mut targets = Vec::new();
        push_updater_dirs_under(&mut targets, &root);
        let paths: Vec<std::path::PathBuf> = targets.iter().map(|t| t.path.clone()).collect();

        assert!(
            paths.contains(&hit.join("pending")) && paths.contains(&hit.join("update.zip")),
            "没按名字表也能发现新应用的更新包"
        );
        assert!(!paths.contains(&hit), "父目录入了表，会和子项双算体积");
        assert!(targets
            .iter()
            .all(|t| t.category == CategoryId::UpdaterPackages));
        assert!(
            !paths.iter().any(|p| p.starts_with(root.join("SomeApp"))),
            "没命中签名的目录不该有任何目标进表"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
