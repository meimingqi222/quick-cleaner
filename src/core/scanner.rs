//! 垃圾扫描与体积统计引擎

use crate::core::categories::{CategoryId, ScanTarget};
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

#[derive(Clone, Debug)]
pub struct ScanItem {
    pub path: PathBuf,
    pub label: String,
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

/// 对一批目标做并行扫描。
///
/// 并行有两层：目标之间（`par_iter`）和单棵树内部（`walk` 里对子目录再 `par_iter`）。
pub fn scan_all(targets: &[ScanTarget], live: &AtomicBool) -> Vec<CategorySummary> {
    // 两个来源并行跑：固定路径表，以及代码目录里的发现式扫描。
    let (mut results, discovered) = rayon::join(
        || -> Vec<ScanItem> {
            targets
                .par_iter()
                .filter(|t| t.path.exists())
                .filter_map(|t| scan_dir(&t.path, &t.label, t.category, live))
                .collect()
        },
        || crate::core::devscan::discover(live),
    );
    results.extend(discovered);

    // 按类别聚合。发现式类目条目可能上百，按体积降序更实用。
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
fn scan_dir(dir: &Path, label: &str, category: CategoryId, live: &AtomicBool) -> Option<ScanItem> {
    if !live.load(Ordering::Relaxed) {
        return None;
    }
    let acc = walk(dir, live);
    Some(ScanItem {
        path: dir.to_path_buf(),
        label: label.to_string(),
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

/// 递归遍历，子目录之间用 rayon 并行。
fn walk(dir: &Path, live: &AtomicBool) -> Acc {
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
    let sub = subdirs
        .par_iter()
        .map(|p| walk(p, live))
        .reduce(Acc::default, Acc::merge);
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
}
