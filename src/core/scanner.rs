//! 垃圾扫描与体积统计引擎

use crate::core::categories::{CategoryId, ScanTarget};
use crate::core::fs_query::FileIndexQuery;
use crate::core::i18n::Text;
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

#[derive(Clone, Debug)]
pub struct ScanItem {
    pub path: PathBuf,
    /// 双语标签，渲染时按当前语言取。扫描跑在后台线程上，那时还不知道
    /// 用户之后会切到哪种语言，而切语言不该触发重扫。
    pub label: Text,
    pub size: u64,
    pub file_count: u64,
    pub category: CategoryId,
    pub last_modified: u64,
    /// 是否由“推荐清理”默认勾选。必须由具体规则决定，不能只看分类。
    pub recommended: bool,
}

#[derive(Clone, Debug)]
pub struct CategorySummary {
    pub category: CategoryId,
    pub total_size: u64,
    pub items: Vec<ScanItem>,
}

/// 一轮完整扫描：固定路径表 + 发现式扫描。
///
/// 界面上**不该**直接用它——它要等最慢的那条通道跑完才返回，本机实测几十秒，
/// 其中 90% 以上花在发现式扫描上。界面走 [`scan_fixed`] + [`scan_discovered`]
/// 两阶段，先把秒级出结果的部分显示出来。这里保留一次性版本给命令行与测试用。
pub fn scan_all(targets: &[ScanTarget], live: &AtomicBool) -> Vec<CategorySummary> {
    let (mut cats, discovered) =
        rayon::join(|| scan_fixed(targets, live), || scan_discovered(live, None));
    merge_discovered(&mut cats, discovered);
    cats
}

/// **第一阶段**：扫固定路径表（`%TEMP%`、各种缓存目录……）。
///
/// 目录位置是已知的，只需要称重。本机实测约 4 秒——注意这个数字**不是**
/// 被体积撑起来的：`go\pkg\mod` 是 0 字节却要 2.8 秒，`Kiro\logs` 只有
/// 3.4 MB 却要 4 秒。瓶颈是文件**数**，几十万次目录元数据查询。
/// 想快只有一条路：别去遍历。见 [`scan_fixed_with_tree`]。
pub fn scan_fixed(targets: &[ScanTarget], live: &AtomicBool) -> Vec<CategorySummary> {
    scan_fixed_inner(targets, live, None)
}

/// 阶段一的查表版：目标落在 `tree` 所属卷上时直接读 MFT 的聚合体积，
/// 一次查表就是 O(路径深度)，完全不碰目录项。
///
/// 树上查不到的目标（不在这个卷上、或者是 MFT 快照之后新建的）自动退回
/// 遍历，因此结果集与 [`scan_fixed`] 完全等价。
///
/// 唯一的差异在口径：MFT 的聚合体积统计卷上全部文件，而遍历会跳过
/// `desktop.ini` 和符号链接，因此同一目录两条路径给出的体积会有零点几个
/// 百分点的偏移。这个不一致在阶段二的双通道之间本来就存在。
/// `last_modified` 在查表路径下拿不到（MFT 记录里没有可直接用的值），
/// 置 0 ——该字段目前全项目没有读取方。
pub fn scan_fixed_with_tree(
    targets: &[ScanTarget],
    live: &AtomicBool,
    tree: &crate::core::disk::SizeTree,
) -> Vec<CategorySummary> {
    scan_fixed_inner(targets, live, Some(tree))
}

/// 耗时最长的前 `n` 个目标，格式化成日志片段。
///
/// 单拎出来是因为它只服务于那一行日志：排序 + 格式化以前是无条件执行的，
/// 结果却只是拼进一个字符串。
fn slowest_targets(measured: &[(ScanItem, std::time::Duration)], n: usize) -> Vec<String> {
    let mut slowest: Vec<&(ScanItem, std::time::Duration)> = measured.iter().collect();
    slowest.sort_unstable_by_key(|(_, d)| std::cmp::Reverse(*d));
    slowest
        .iter()
        .take(n)
        .map(|(it, d)| {
            format!(
                "{:?} {} {}",
                d,
                crate::core::model::fmt_size(it.size),
                it.path.display()
            )
        })
        .collect()
}

fn scan_fixed_inner(
    targets: &[ScanTarget],
    live: &AtomicBool,
    tree: Option<&crate::core::disk::SizeTree>,
) -> Vec<CategorySummary> {
    let t0 = std::time::Instant::now();
    // 统一走 FSIndexEngine：有树先查表（带卷/挂载点守卫），查不到回退
    // scanner::measure_target 并行遍历，两通道口径完全一致。
    let engine = crate::core::fs_query::FSIndexEngine::new(tree);
    // 逐个目标计时。目标是并行称重的，墙钟时间等于**最慢那一个**，
    // 所以合计耗时没有意义，排行榜才有。
    let measured: Vec<(ScanItem, std::time::Duration)> = targets
        .par_iter()
        .filter(|t| {
            // tmutil:// 虚拟路径（APFS 本地快照）不走文件系统 exists() 检查
            if t.path.to_string_lossy().starts_with("tmutil://") {
                return true;
            }
            // 固定目标本身是符号链接时绝不扫描。否则称重可能来自链接目标，
            // 清理时也可能在目标被替换的竞态下触碰链接指向的数据。
            std::fs::symlink_metadata(&t.path).is_ok_and(|md| !md.file_type().is_symlink())
        })
        .filter_map(|t| {
            let started = std::time::Instant::now();
            // tmutil:// 虚拟路径：大小未知（APFS 快照是 COW 的），用 0 占位
            if t.path.to_string_lossy().starts_with("tmutil://") {
                return Some((
                    ScanItem {
                        path: t.path.clone(),
                        label: t.label.clone(),
                        size: 0,
                        file_count: 0,
                        category: t.category,
                        last_modified: 0,
                        recommended: t.recommended,
                    },
                    started.elapsed(),
                ));
            }
            engine
                .measure_path(&t.path, live)
                .map(|(size, files, newest)| {
                    (
                        ScanItem {
                            path: t.path.clone(),
                            label: t.label.clone(),
                            size,
                            file_count: files,
                            category: t.category,
                            last_modified: newest,
                            recommended: t.recommended,
                        },
                        started.elapsed(),
                    )
                })
        })
        .collect();

    let top = slowest_targets(&measured, 5);

    let results: Vec<ScanItem> = measured.into_iter().map(|(it, _)| it).collect();
    let total: u64 = results.iter().map(|it| it.size).sum();
    crate::log!(
        "阶段一 scan_fixed 完成：{:?}，{}/{} 个目标命中，合计 {}；最慢 5 个：{}",
        t0.elapsed(),
        results.len(),
        targets.len(),
        crate::core::model::fmt_size(total),
        top.join(" | ")
    );
    aggregate(results)
}

/// 在 MFT 树上查一个目录的递归体积与文件数。查不到返回 `None`，调用方退回遍历。
pub(crate) fn measure_via_tree(
    tree: &crate::core::disk::SizeTree,
    path: &Path,
) -> Option<(u64, u64)> {
    // Windows 上树是按卷构建的，volume_of 必须精确匹配。
    // macOS 上树是按用户目录构建的（mount_point = /Users/<user>），
    // volume_of 返回的是文件系统挂载点（/），两者不等。
    // 改为：只要路径在树的挂载点之下就尝试查表，strip_prefix 和 find_path
    // 会负责精确匹配。
    #[cfg(windows)]
    {
        if &volume_of(path)? != tree.volume() {
            return None;
        }
    }
    #[cfg(not(windows))]
    {
        // macOS：路径必须在树的挂载点（用户目录）之下
        if !path.starts_with(tree.volume().mount_point()) {
            return None;
        }
    }
    // `find_path` 逐层匹配，某一层对不上就提前收工。因此长度对不上就说明
    // 这条路径不在树里（典型情况：MFT 快照之后才建出来的目录）。
    let relative = path.strip_prefix(tree.volume().mount_point()).ok()?;
    let want = relative
        .components()
        .filter(|c| matches!(c, std::path::Component::Normal(_)))
        .count();
    let chain = tree.find_path(path);
    if chain.len() != want + 1 {
        return None;
    }
    let node = *chain.last()?;
    Some((tree.size_of(node), tree.file_count_of(node)))
}

/// 路径所在的卷。
///
/// Windows 上从 `X:` 盘符前缀提取；macOS / Unix 上用 `list_volumes()` 枚举的
/// 挂载点里找包含该路径的那个。找不到（UNC、相对路径、外接盘未挂载）返回 `None`。
fn volume_of(path: &Path) -> Option<crate::core::disk::VolumeId> {
    #[cfg(windows)]
    {
        let s = path.to_string_lossy();
        let mut it = s.chars();
        let c = it.next()?.to_ascii_uppercase();
        if c.is_ascii_alphabetic() && it.next() == Some(':') {
            return Some(crate::core::disk::VolumeId::from_drive_letter(c));
        }
        None
    }
    #[cfg(not(windows))]
    {
        // macOS / Unix：路径必须以某个挂载点为前缀。
        // 挂载点列表来自 `platform::list_volumes()`，根卷 `/` 永远在里面，
        // 所以绝对路径至少能匹配到 `/`。相对路径和 UNC 不属于任何卷。
        if !path.is_absolute() {
            return None;
        }
        let volumes = crate::platform::list_volumes();
        // 选最长前缀匹配：`/Volumes/外接盘/foo` 应匹配 `/Volumes/外接盘`
        // 而不是 `/`。按挂载点长度降序排列后取第一个匹配的。
        let mut best: Option<(usize, &crate::core::disk::VolumeId)> = None;
        for vol in &volumes {
            let mount = vol.mount_point();
            if path.starts_with(mount) {
                let len = mount.as_os_str().len();
                if best.is_none_or(|(blen, _)| len > blen) {
                    best = Some((len, vol));
                }
            }
        }
        best.map(|(_, v)| v.clone())
    }
}

/// 固定路径目标最集中的那个卷。
///
/// 阶段一要查表就得先解析一个卷的 MFT，只解析得起一个（一棵全盘树约
/// 350 MB）。目标散落在多个盘上时选命中最多的那个，剩下的照旧遍历。
pub fn dominant_volume(targets: &[ScanTarget]) -> Option<crate::core::disk::VolumeId> {
    let mut count: std::collections::HashMap<crate::core::disk::VolumeId, usize> =
        std::collections::HashMap::new();
    for t in targets {
        if let Some(v) = volume_of(&t.path) {
            *count.entry(v).or_default() += 1;
        }
    }
    count.into_iter().max_by_key(|&(_, n)| n).map(|(v, _)| v)
}

/// **第二阶段**：发现式扫描构建产物。
///
/// 这些目录散落在用户所有代码目录里，位置不确定，只能靠全盘检索，
/// 是整轮扫描里最贵的一步（本机冷缓存 25 秒量级）。放在第二阶段异步补齐。
pub fn scan_discovered(
    live: &AtomicBool,
    prescanned: Option<crate::core::disk::ScanResult>,
) -> Vec<ScanItem> {
    let t0 = std::time::Instant::now();
    let items = crate::core::devscan::discover(live, prescanned);
    let total: u64 = items.iter().map(|it| it.size).sum();
    crate::log!(
        "阶段二 scan_discovered 完成：{:?}，{} 条，合计 {}",
        t0.elapsed(),
        items.len(),
        crate::core::model::fmt_size(total)
    );
    items
}

/// macOS 专用：接受 `Arc<ScanResult>` 的 scan_discovered 变体。
/// 避免从 UI 层 clone 6.6M 条目的 ScanResult。
#[cfg(not(windows))]
pub fn scan_discovered_arc(
    live: &AtomicBool,
    prescanned: Option<std::sync::Arc<crate::core::disk::ScanResult>>,
) -> Vec<ScanItem> {
    let t0 = std::time::Instant::now();
    let items = crate::core::devscan::discover_arc(live, prescanned);
    let total: u64 = items.iter().map(|it| it.size).sum();
    crate::log!(
        "阶段二 scan_discovered 完成：{:?}，{} 条，合计 {}",
        t0.elapsed(),
        items.len(),
        crate::core::model::fmt_size(total)
    );
    items
}

/// 把第二阶段的结果并进已有的分类汇总。
///
/// 合并前会剔除**已经不存在**的路径：第二阶段跑了几十秒，这期间用户完全
/// 可能已经清掉了其中一些目录，把它们并进列表会显示成能清理却清不掉的幽灵条目。
pub fn merge_discovered(cats: &mut [CategorySummary], items: Vec<ScanItem>) {
    let mut by_cat: std::collections::HashMap<CategoryId, Vec<ScanItem>> =
        std::collections::HashMap::new();
    for item in items.into_iter().filter(|it| it.path.exists()) {
        by_cat.entry(item.category).or_default().push(item);
    }

    for cat in cats.iter_mut() {
        let Some(mut found) = by_cat.remove(&cat.category) else {
            continue;
        };
        // 同一路径可能在第一阶段的固定表里已经有了，别重复计数
        let existing: HashSet<&Path> = cat.items.iter().map(|i| i.path.as_path()).collect();
        found.retain(|it| !existing.contains(it.path.as_path()));

        cat.items.append(&mut found);
        if cat.category.is_discovered() {
            cat.items
                .sort_unstable_by_key(|b| std::cmp::Reverse(b.size));
        }
        cat.total_size = cat.items.iter().map(|it| it.size).sum();
    }
}

/// 把散装条目按类别聚合成汇总，并按体积降序展示。
fn aggregate(results: Vec<ScanItem>) -> Vec<CategorySummary> {
    let mut out: Vec<CategorySummary> = Vec::new();
    for cat in CategoryId::ALL {
        let mut items: Vec<ScanItem> = results
            .iter()
            .filter(|it| it.category == cat)
            .cloned()
            .collect();
        // 动态缓存目标原本按文件系统枚举顺序展示，几 GB 的大项可能藏在
        // 中间，而列表底部只剩几 KB 的 .DS_Store，看起来会与汇总值不符。
        items.sort_unstable_by_key(|b| std::cmp::Reverse(b.size));
        let total: u64 = items.iter().map(|it| it.size).sum();
        out.push(CategorySummary {
            category: cat,
            total_size: total,
            items,
        });
    }
    out
}

/// 清理完成后就地更新扫描结果，返回被清空的条目路径。
///
/// 替代「清理完再整轮重扫」：`CleanReport` 已经精确记录了哪些目标失败，
/// 剩下的就是成功的。重扫唯一的作用只是刷新列表，而开发垃圾的发现式
/// 扫描要几十秒，让用户点完清理干等一分钟毫无必要。释放量也不依赖重扫
/// ——`CleanProgress` 边删边记的字节数比前后差值更准。
///
/// 成功清理的条目**直接从列表移除**：它要么已经不存在（开发产物整个删掉），
/// 要么已经空了（系统缓存目录清空内容保留自身），两种情况都没有任何可清理
/// 的东西了，继续挂在列表上只是噪音。失败的条目保持原样，用户可以再试。
pub fn apply_clean_result(
    cats: &mut [CategorySummary],
    attempted: &[PathBuf],
    failed: &[PathBuf],
) -> Vec<PathBuf> {
    let attempted: HashSet<&Path> = attempted.iter().map(|p| p.as_path()).collect();
    let failed: HashSet<&Path> = failed.iter().map(|p| p.as_path()).collect();

    let mut cleared = Vec::new();
    for cat in cats.iter_mut() {
        cat.items.retain(|item| {
            let p = item.path.as_path();
            let done = attempted.contains(p) && !failed.contains(p);
            if done {
                cleared.push(item.path.clone());
            }
            !done
        });
        cat.total_size = cat.items.iter().map(|i| i.size).sum();
    }
    cleared
}

/// 一棵子树的累计结果。
#[derive(Clone, Copy, Default)]
struct Acc {
    size: u64,
    files: u64,
    newest: u64,
}

impl Acc {
    fn merge(mut self, other: Acc) -> Acc {
        self.size += other.size;
        self.files += other.files;
        self.newest = self.newest.max(other.newest);
        self
    }
}

/// 测算单个目标（文件或目录子树），返回 `(总字节数, 文件数, 最新 mtime 秒)`。
///
/// `FSIndexEngine::measure_path` 的树查回退通道，与 [`measure_dir`] 共用
/// 同一套遍历口径：
/// - 并行：前几层 rayon（见 [`walk_at`]）；
/// - 口径：macOS 用磁盘块分配（blocks × 512），Windows 用逻辑大小；
/// - 跳过符号链接（只计链接本身没意义，清理还可能碰到目标数据）；
/// - 忽略 `desktop.ini`（空目录噪音）。
///
/// 路径不存在、符号链接或非普通文件类型返回 `None`。
pub(crate) fn measure_target(path: &Path, live: &AtomicBool) -> Option<(u64, u64, u64)> {
    if !live.load(Ordering::Relaxed) {
        return None;
    }
    let md = std::fs::symlink_metadata(path).ok()?;
    if md.file_type().is_symlink() {
        return None;
    }
    let acc = if md.is_file() {
        Acc {
            size: allocated_file_size(&md),
            files: 1,
            newest: md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs()),
        }
    } else if md.is_dir() {
        walk(path, live)
    } else {
        return None;
    };
    Some((acc.size, acc.files, acc.newest))
}

fn allocated_file_size(md: &std::fs::Metadata) -> u64 {
    #[cfg(windows)]
    {
        md.len()
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::MetadataExt;
        md.blocks().saturating_mul(512)
    }
}

/// 测算一个目录的子树体积。
///
/// 返回 `(总字节数, 文件数, 最新修改时间)`。`devscan` 用它给发现出来的
/// 目录称重，避免再写一份并行遍历。
pub fn measure_dir(dir: &Path, live: &AtomicBool) -> (u64, u64, u64) {
    let acc = walk(dir, live);
    (acc.size, acc.files, acc.newest)
}

/// 前几层子目录之间用 rayon 并行，再深就串行。
///
/// `node_modules` / `target` 这类树又宽又深，每层几十个只装几个文件的小目录。
/// 原来每一层都开 `par_iter`，光是 rayon 的任务切分与 join 开销就盖过了真正的
/// 读目录工作。前两层分叉已经足够喂饱线程池（一个 `node_modules` 顶层就有几百个
/// 包目录），下面串行反而更快。
const PAR_DEPTH: usize = 2;

/// 递归遍历，浅层用 rayon 并行。
fn walk(dir: &Path, live: &AtomicBool) -> Acc {
    walk_at(dir, live, 0)
}

fn walk_at(dir: &Path, live: &AtomicBool, depth: usize) -> Acc {
    let mut acc = Acc::default();
    if !live.load(Ordering::Relaxed) {
        return acc;
    }

    let Ok(rd) = std::fs::read_dir(dir) else {
        return acc;
    };

    let mut subdirs: Vec<PathBuf> = Vec::new();
    for entry in rd.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            subdirs.push(entry.path());
        } else if ft.is_file() {
            // 忽略 desktop.ini（避免空目录显示噪点）
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("desktop.ini")
            {
                continue;
            }
            acc.files += 1;
            if let Ok(md) = entry.metadata() {
                acc.size += allocated_file_size(&md);
                if let Ok(m) = md.modified() {
                    let t = m
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    acc.newest = acc.newest.max(t);
                }
            }
        }
    }

    if subdirs.is_empty() {
        return acc;
    }
    let sub = if depth < PAR_DEPTH && subdirs.len() > 1 {
        subdirs
            .par_iter()
            .map(|p| walk_at(p, live, depth + 1))
            .reduce(Acc::default, Acc::merge)
    } else {
        subdirs
            .iter()
            .map(|p| walk_at(p, live, depth + 1))
            .fold(Acc::default(), Acc::merge)
    };
    acc.merge(sub)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_of_reads_the_drive_letter() {
        #[cfg(windows)]
        {
            use crate::core::disk::VolumeId;
            assert_eq!(
                volume_of(Path::new(r"C:\Users\me")),
                Some(VolumeId::from_drive_letter('C'))
            );
            // 小写盘符要归一化，否则和 SizeTree::volume() 比不上
            assert_eq!(
                volume_of(Path::new(r"d:\code")),
                Some(VolumeId::from_drive_letter('D'))
            );
            assert_eq!(
                volume_of(Path::new(r"C:\")),
                Some(VolumeId::from_drive_letter('C'))
            );
        }
        // UNC 与相对路径没有盘符，只能走遍历
        assert_eq!(volume_of(Path::new(r"\\server\share\x")), None);
        assert_eq!(volume_of(Path::new("relative/path")), None);
        assert_eq!(volume_of(Path::new("")), None);

        // macOS / Unix：绝对路径至少匹配到根卷 `/`
        #[cfg(not(windows))]
        {
            let root_vol = volume_of(Path::new("/Users/me/Library/Caches"));
            assert!(root_vol.is_some(), "绝对路径应当匹配到某个卷");
            assert_eq!(
                root_vol.unwrap().mount_point(),
                std::path::Path::new("/"),
                "无外接盘时应匹配到根卷"
            );
            // 相对路径不属于任何卷
            assert_eq!(volume_of(Path::new("foo/bar")), None);
        }
    }

    #[test]
    fn dominant_volume_picks_the_busiest_drive() {
        let mk = |p: &str| ScanTarget {
            path: PathBuf::from(p),
            label: Text::same("t"),
            category: CategoryId::UserTemp,
            recommended: true,
        };
        #[cfg(windows)]
        let targets = [mk(r"C:\a"), mk(r"C:\b"), mk(r"D:\c"), mk(r"\\unc\share")];
        #[cfg(windows)]
        {
            use crate::core::disk::VolumeId;
            assert_eq!(
                dominant_volume(&targets),
                Some(VolumeId::from_drive_letter('C'))
            );
        }
        #[cfg(not(windows))]
        {
            // macOS 上所有绝对路径都落在根卷 `/`，所以 dominant_volume 应返回 `/`
            let mac_targets = [mk("/Users/me/.npm"), mk("/Users/me/.cargo"), mk("/tmp")];
            let dom = dominant_volume(&mac_targets);
            assert!(dom.is_some(), "应当能选出主导卷");
            assert_eq!(
                dom.unwrap().mount_point(),
                std::path::Path::new("/"),
                "无外接盘时主导卷应是根卷"
            );
        }
        assert_eq!(dominant_volume(&[]), None);
    }

    #[test]
    fn fixed_file_target_reports_its_real_size() {
        let path = std::env::temp_dir().join("qc_scan_single_file");
        std::fs::write(&path, b"metadata").unwrap();
        let live = AtomicBool::new(true);

        let (size, files, newest) = measure_target(&path, &live).unwrap();

        assert_eq!(
            size,
            allocated_file_size(&std::fs::metadata(&path).unwrap())
        );
        assert_eq!(files, 1);
        assert!(newest > 0, "单文件应带 mtime");
        let _ = std::fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn fixed_scan_rejects_symlink_root() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join("qc_scan_symlink_root");
        let target = root.join("target");
        let link = root.join("link");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("keep"), b"x").unwrap();
        symlink(&target, &link).unwrap();
        let targets = [ScanTarget {
            path: link,
            label: Text::same("link"),
            category: CategoryId::UserTemp,
            recommended: true,
        }];

        let cats = scan_fixed(&targets, &AtomicBool::new(true));

        assert!(cats.iter().all(|cat| cat.items.is_empty()));
        let _ = std::fs::remove_dir_all(root);
    }

    fn item(path: &str, size: u64, cat: CategoryId) -> ScanItem {
        ScanItem {
            path: PathBuf::from(path),
            label: path.into(),
            size,
            file_count: size / 10,
            category: cat,
            last_modified: 0,
            recommended: cat.default_selected(),
        }
    }

    fn summary(cat: CategoryId, items: Vec<ScanItem>) -> CategorySummary {
        CategorySummary {
            total_size: items.iter().map(|i| i.size).sum(),
            category: cat,
            items,
        }
    }

    #[test]
    fn successful_targets_are_zeroed_and_totals_recomputed() {
        let mut cats = vec![summary(
            CategoryId::UserTemp,
            vec![
                item(r"C:\a", 1000, CategoryId::UserTemp),
                item(r"C:\b", 500, CategoryId::UserTemp),
            ],
        )];
        assert_eq!(cats[0].total_size, 1500);

        let cleared = apply_clean_result(
            &mut cats,
            &[PathBuf::from(r"C:\a"), PathBuf::from(r"C:\b")],
            &[],
        );

        assert_eq!(cleared.len(), 2);
        assert_eq!(cats[0].total_size, 0);
        // 清完就没有可清的了，条目应从列表消失而不是留一堆 0 B 的空壳
        assert!(cats[0].items.is_empty());
    }

    /// 被占用而删除失败的条目必须保留体积，否则界面会谎报已经清干净了。
    #[test]
    fn failed_targets_keep_their_size() {
        let mut cats = vec![summary(
            CategoryId::UserTemp,
            vec![
                item(r"C:\ok", 1000, CategoryId::UserTemp),
                item(r"C:\locked", 700, CategoryId::UserTemp),
            ],
        )];

        let cleared = apply_clean_result(
            &mut cats,
            &[PathBuf::from(r"C:\ok"), PathBuf::from(r"C:\locked")],
            &[PathBuf::from(r"C:\locked")],
        );

        assert_eq!(cleared, vec![PathBuf::from(r"C:\ok")]);
        assert_eq!(cats[0].total_size, 700);
        assert_eq!(cats[0].items.len(), 1, "失败的条目必须留在列表里供重试");
        assert_eq!(cats[0].items[0].path, PathBuf::from(r"C:\locked"));
        assert_eq!(cats[0].items[0].size, 700);
    }

    /// 没被本次清理选中的条目不能受影响。
    #[test]
    fn untouched_items_are_left_alone() {
        let mut cats = vec![summary(
            CategoryId::PackageCache,
            vec![
                item(r"C:\picked", 100, CategoryId::PackageCache),
                item(r"C:\other", 900, CategoryId::PackageCache),
            ],
        )];

        apply_clean_result(&mut cats, &[PathBuf::from(r"C:\picked")], &[]);

        assert_eq!(cats[0].items.len(), 1);
        assert_eq!(cats[0].items[0].path, PathBuf::from(r"C:\other"));
        assert_eq!(cats[0].total_size, 900);
    }

    #[test]
    fn works_across_multiple_categories() {
        let mut cats = vec![
            summary(
                CategoryId::UserTemp,
                vec![item(r"C:\t", 200, CategoryId::UserTemp)],
            ),
            summary(
                CategoryId::DevBuild,
                vec![item(r"D:\p\target", 5000, CategoryId::DevBuild)],
            ),
        ];

        apply_clean_result(&mut cats, &[PathBuf::from(r"D:\p\target")], &[]);

        assert_eq!(cats[0].total_size, 200, "未涉及的类别不该变");
        assert_eq!(cats[1].total_size, 0);
    }

    /// 启动扫描的耗时画像：跑一次真实扫描，把时间摊到「固定路径表」与
    /// 「发现式扫描」两条通道上，并列出最慢的 15 个固定目标。
    ///
    /// 手动跑：`cargo test --lib scan_timing_profile -- --ignored --nocapture`
    /// （想看 MFT 通道的数字要用管理员身份跑）
    #[test]
    #[ignore]
    fn scan_timing_profile() {
        use crate::core::categories::all_targets;
        use std::time::Instant;

        let live = AtomicBool::new(true);
        let targets = all_targets();
        println!("固定目标 {} 个", targets.len());

        let t0 = Instant::now();
        let existing: Vec<_> = targets.iter().filter(|t| t.path.exists()).collect();
        println!(
            "  exists() 过滤: {:?}（剩 {} 个）",
            t0.elapsed(),
            existing.len()
        );

        let t1 = Instant::now();
        let mut per: Vec<(std::time::Duration, String, u64)> = existing
            .par_iter()
            .map(|t| {
                let s = Instant::now();
                let acc = walk(&t.path, &live);
                (s.elapsed(), t.path.display().to_string(), acc.size)
            })
            .collect();
        println!("  固定路径表并行扫描合计: {:?}", t1.elapsed());

        per.sort_by_key(|b| std::cmp::Reverse(b.0));
        println!("  最慢的 15 个：");
        for (d, path, size) in per.iter().take(15) {
            println!(
                "    {:>9.2?}  {:>10}  {}",
                d,
                crate::core::model::fmt_size(*size),
                path
            );
        }

        let t2 = Instant::now();
        let discovered = crate::core::devscan::discover(&live, None);
        println!(
            "  发现式扫描: {:?}（{} 条）",
            t2.elapsed(),
            discovered.len()
        );

        let t3 = Instant::now();
        let cats = scan_all(&targets, &live);
        let total: u64 = cats.iter().map(|c| c.total_size).sum();
        println!(
            "整轮 scan_all: {:?}，合计 {}",
            t3.elapsed(),
            crate::core::model::fmt_size(total)
        );
    }

    // ---- 第二阶段结果的合并 ----

    /// 造一个真实存在的临时目录，供「合并前过滤已不存在的路径」那条规则用。
    fn real_dir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("qc_merge_{tag}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn empty_cats() -> Vec<CategorySummary> {
        CategoryId::ALL
            .iter()
            .map(|&c| CategorySummary {
                category: c,
                total_size: 0,
                items: Vec::new(),
            })
            .collect()
    }

    #[test]
    fn merged_items_land_in_their_own_category_and_update_totals() {
        let dir = real_dir("basic");
        let mut cats = empty_cats();

        let mut it = item(&dir.to_string_lossy(), 4096, CategoryId::DevBuild);
        it.path = dir.clone();
        merge_discovered(&mut cats, vec![it]);

        let dev = cats
            .iter()
            .find(|c| c.category == CategoryId::DevBuild)
            .unwrap();
        assert_eq!(dev.items.len(), 1);
        assert_eq!(dev.total_size, 4096);
        // 其它类别不该被牵连
        assert!(cats
            .iter()
            .filter(|c| c.category != CategoryId::DevBuild)
            .all(|c| c.items.is_empty() && c.total_size == 0));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 第二阶段跑了几十秒，这期间用户可能已经把某些目录清掉了。
    /// 那些路径不能再并进列表，否则界面上会出现清不掉的幽灵条目。
    #[test]
    fn vanished_paths_are_dropped_on_merge() {
        let gone = std::env::temp_dir().join("qc_merge_definitely_not_here_8f21");
        let _ = std::fs::remove_dir_all(&gone);
        assert!(!gone.exists());

        let mut cats = empty_cats();
        let mut it = item("placeholder", 9999, CategoryId::DevBuild);
        it.path = gone;
        merge_discovered(&mut cats, vec![it]);

        let dev = cats
            .iter()
            .find(|c| c.category == CategoryId::DevBuild)
            .unwrap();
        assert!(dev.items.is_empty());
        assert_eq!(dev.total_size, 0);
    }

    /// 同一路径两个阶段都报了的话只能算一次，否则总量翻倍。
    #[test]
    fn duplicate_paths_are_not_counted_twice() {
        let dir = real_dir("dup");
        let mut cats = empty_cats();

        let mut first = item(&dir.to_string_lossy(), 1000, CategoryId::DevBuild);
        first.path = dir.clone();
        cats.iter_mut()
            .find(|c| c.category == CategoryId::DevBuild)
            .unwrap()
            .items
            .push(first);

        let mut again = item(&dir.to_string_lossy(), 1000, CategoryId::DevBuild);
        again.path = dir.clone();
        merge_discovered(&mut cats, vec![again]);

        let dev = cats
            .iter()
            .find(|c| c.category == CategoryId::DevBuild)
            .unwrap();
        assert_eq!(dev.items.len(), 1, "同一路径不能出现两条");
        assert_eq!(dev.total_size, 1000);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fixed_category_items_are_sorted_by_size() {
        let cats = aggregate(vec![
            item("/cache/tiny", 8 * 1024, CategoryId::UserTemp),
            item("/cache/large", 4 * 1024 * 1024 * 1024, CategoryId::UserTemp),
            item("/cache/medium", 128 * 1024 * 1024, CategoryId::UserTemp),
        ]);

        let user_temp = cats
            .iter()
            .find(|c| c.category == CategoryId::UserTemp)
            .unwrap();
        let sizes: Vec<u64> = user_temp.items.iter().map(|item| item.size).collect();
        assert_eq!(
            sizes,
            vec![4 * 1024 * 1024 * 1024, 128 * 1024 * 1024, 8 * 1024]
        );
    }

    /// 发现式类目条目可能上百，合并后要按体积降序，最大的排在最前面。
    #[test]
    fn discovered_items_stay_sorted_by_size() {
        let small = real_dir("sort_small");
        let big = real_dir("sort_big");
        let mut cats = empty_cats();

        let mut a = item(&small.to_string_lossy(), 100, CategoryId::DevBuild);
        a.path = small.clone();
        let mut b = item(&big.to_string_lossy(), 5000, CategoryId::DevBuild);
        b.path = big.clone();
        merge_discovered(&mut cats, vec![a, b]);

        let dev = cats
            .iter()
            .find(|c| c.category == CategoryId::DevBuild)
            .unwrap();
        assert_eq!(dev.items[0].size, 5000);
        assert_eq!(dev.items[1].size, 100);
        assert_eq!(dev.total_size, 5100);

        let _ = std::fs::remove_dir_all(&small);
        let _ = std::fs::remove_dir_all(&big);
    }
}
