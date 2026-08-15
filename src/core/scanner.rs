//! 垃圾扫描与体积统计引擎

use crate::core::categories::{CategoryId, ScanTarget};
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
    let (mut cats, discovered) = rayon::join(
        || scan_fixed(targets, live),
        || scan_discovered(live),
    );
    merge_discovered(&mut cats, discovered);
    cats
}

/// **第一阶段**：扫固定路径表（`%TEMP%`、各种缓存目录……）。
///
/// 目录位置是已知的，只需要称重，本机实测约 1 秒。
pub fn scan_fixed(targets: &[ScanTarget], live: &AtomicBool) -> Vec<CategorySummary> {
    let results: Vec<ScanItem> = targets
        .par_iter()
        .filter(|t| t.path.exists())
        .filter_map(|t| scan_dir(&t.path, &t.label, t.category, live))
        .collect();
    aggregate(results)
}

/// **第二阶段**：发现式扫描构建产物。
///
/// 这些目录散落在用户所有代码目录里，位置不确定，只能靠全盘检索，
/// 是整轮扫描里最贵的一步（本机冷缓存 25 秒量级）。放在第二阶段异步补齐。
pub fn scan_discovered(live: &AtomicBool) -> Vec<ScanItem> {
    crate::core::devscan::discover(live)
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
            cat.items.sort_unstable_by(|a, b| b.size.cmp(&a.size));
        }
        cat.total_size = cat.items.iter().map(|it| it.size).sum();
    }
}

/// 把散装条目按类别聚合成汇总。发现式类目条目可能上百，按体积降序更实用。
fn aggregate(results: Vec<ScanItem>) -> Vec<CategorySummary> {
    let mut out: Vec<CategorySummary> = Vec::new();
    for cat in CategoryId::ALL {
        let mut items: Vec<ScanItem> = results
            .iter()
            .filter(|it| it.category == cat)
            .cloned()
            .collect();
        if cat.is_discovered() {
            items.sort_unstable_by(|a, b| b.size.cmp(&a.size));
        }
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

/// 扫描单个目录：大小 = 子树全部文件大小；文件数 = 全部文件数。
fn scan_dir(dir: &Path, label: &Text, category: CategoryId, live: &AtomicBool) -> Option<ScanItem> {
    if !live.load(Ordering::Relaxed) {
        return None;
    }
    let acc = walk(dir, live);
    Some(ScanItem {
        path: dir.to_path_buf(),
        label: label.clone(),
        size: acc.size,
        file_count: acc.files,
        category,
        last_modified: acc.newest,
    })
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
            if entry.file_name().to_string_lossy().eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            acc.files += 1;
            if let Ok(md) = entry.metadata() {
                acc.size += md.len();
                if let Ok(m) = md.modified() {
                    let t = m.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
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

    fn item(path: &str, size: u64, cat: CategoryId) -> ScanItem {
        ScanItem {
            path: PathBuf::from(path),
            label: path.into(),
            size,
            file_count: size / 10,
            category: cat,
            last_modified: 0,
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
            summary(CategoryId::UserTemp, vec![item(r"C:\t", 200, CategoryId::UserTemp)]),
            summary(CategoryId::DevBuild, vec![item(r"D:\p\target", 5000, CategoryId::DevBuild)]),
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
        println!("  exists() 过滤: {:?}（剩 {} 个）", t0.elapsed(), existing.len());

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

        per.sort_by(|a, b| b.0.cmp(&a.0));
        println!("  最慢的 15 个：");
        for (d, path, size) in per.iter().take(15) {
            println!("    {:>9.2?}  {:>10}  {}", d, crate::core::model::fmt_size(*size), path);
        }

        let t2 = Instant::now();
        let discovered = crate::core::devscan::discover(&live);
        println!("  发现式扫描: {:?}（{} 条）", t2.elapsed(), discovered.len());

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

        let dev = cats.iter().find(|c| c.category == CategoryId::DevBuild).unwrap();
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

        let dev = cats.iter().find(|c| c.category == CategoryId::DevBuild).unwrap();
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

        let dev = cats.iter().find(|c| c.category == CategoryId::DevBuild).unwrap();
        assert_eq!(dev.items.len(), 1, "同一路径不能出现两条");
        assert_eq!(dev.total_size, 1000);

        let _ = std::fs::remove_dir_all(&dir);
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

        let dev = cats.iter().find(|c| c.category == CategoryId::DevBuild).unwrap();
        assert_eq!(dev.items[0].size, 5000);
        assert_eq!(dev.items[1].size, 100);
        assert_eq!(dev.total_size, 5100);

        let _ = std::fs::remove_dir_all(&small);
        let _ = std::fs::remove_dir_all(&big);
    }
}
