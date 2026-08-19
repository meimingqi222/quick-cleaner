//! 磁盘透镜占比计算与渲染

use super::disk_components::BreakdownItem;
use crate::core::disk::{Node, ScanResult, SizeTree};
use crate::core::i18n::Language;
use crate::core::model::fmt_size;
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::views::DiskTab;
use crate::ui::Root;
use gpui::{div, prelude::*, px, rgb};

/// 按当前 Tab 算出占比清单，连带右上角那三行摘要文字。
///
/// 两种 Tab 的口径完全不同，各自成函数：目录树看「当前目录下各子项的
/// 占比」，大文件榜看「按后缀聚类」。这里只负责挑一条路。
pub(super) fn compute_breakdown(
    root: &Root,
    scan: &ScanResult,
    cur: u32,
    children: &[&Node],
    colors: &[u32; 6],
) -> (String, String, String, Vec<BreakdownItem>) {
    let tree = &scan.tree;
    match root.disk.tab {
        DiskTab::Tree => breakdown_for_tree(root, tree, cur, children, colors),
        DiskTab::Files => breakdown_for_files(root, tree, colors),
    }
}

/// 目录树视图的占比清单：当前目录下各子项各占多少。
///
/// 站在盘符根上时口径不一样——要把「可用空间」也算成一块，让环形图
/// 表示整块盘而不只是已用部分。
fn breakdown_for_tree(
    root: &Root,
    tree: &SizeTree,
    cur: u32,
    children: &[&Node],
    colors: &[u32; 6],
) -> (String, String, String, Vec<BreakdownItem>) {
    let lang = root.language;
    let cur_size = tree.size_of(cur);
    let cur_name = if cur == tree.root() {
        tr_volume_root(lang, tree.volume())
    } else {
        tree.name_of(cur)
    };

    let is_root = cur == tree.root();
    if let Some((tot, fre)) = if is_root { root.disk.space } else { None } {
        let used = tot.saturating_sub(fre);
        let used_pct = ((used as f64 / tot.max(1) as f64) * 100.0).round() as u64;

        // 磁盘总容量/已用/空闲统一用 1024 进制，与文件/目录大小口径一致
        let used_str = fmt_size(used);
        let fre_str = fmt_size(fre);

        let mut items = Vec::new();
        let mut top_sum = 0u64;

        // 根目录下各子目录的大小是「apparent size」，会因 APFS 快照/克隆/硬链接
        // 超过物理容量。圆环图和百分比不能再用「子目录/磁盘总量」这种口径，
        // 否则 System 直接 100%、Users 又 60%、加起来超过 100%。
        // 改用根树自身总大小 cur_size 做分母，最后再归一化，让各块之和=1。
        let cur_size = tree.size_of(cur).max(1);
        let min_visible_ratio = 0.01;
        let visible_children: Vec<_> = children
            .iter()
            .filter(|c| (c.size as f64 / cur_size as f64) >= min_visible_ratio)
            .take(4)
            .collect();

        for (i, c) in visible_children.iter().enumerate() {
            let ratio = c.size as f64 / cur_size as f64;
            top_sum += c.size;
            items.push(BreakdownItem {
                name: c.name.clone(),
                size: c.size,
                ratio,
                color: colors[i % (colors.len() - 2)],
                is_dir: c.is_dir,
                idx: Some(c.idx),
            });
        }

        // 其他已用 = 物理已用 - 已经展示的子目录。可能因为快照使得 top_sum > used，
        // 此时 top 目录已经「覆盖」全部物理空间，不再显示「其他已用」。
        if used > top_sum && used - top_sum > 1024 * 1024 {
            let rem = used - top_sum;
            let ratio = rem as f64 / cur_size as f64;
            let rem_count = children.len().saturating_sub(visible_children.len());
            let others_label = match lang {
                Language::Zh => format!("其他已用 {} 项", rem_count),
                Language::En => format!("Other {} items", rem_count),
            };
            items.push(BreakdownItem {
                name: others_label,
                size: rem,
                ratio,
                color: colors[5], // Slate Gray
                is_dir: false,
                idx: None,
            });
        }

        // 空闲可用空间条目（以翡翠绿清晰展现）
        if fre > 0 {
            let free_ratio = fre as f64 / cur_size as f64;
            let free_label = match lang {
                Language::Zh => "空闲可用空间".to_string(),
                Language::En => "Free Space".to_string(),
            };
            items.push(BreakdownItem {
                name: free_label,
                size: fre,
                ratio: free_ratio,
                color: colors[4], // 0x10b981 Emerald Green
                is_dir: false,
                idx: None,
            });
        }

        // 归一化所有块的 ratio，使圆环和百分比一致且总和为 1
        let ratio_sum: f64 = items.iter().map(|it| it.ratio).sum();
        if ratio_sum > 0.0 {
            for it in &mut items {
                it.ratio /= ratio_sum;
            }
        }

        let used_count_str = match lang {
            Language::Zh => format!("已用 {used_pct}% · 空闲 {}", fre_str),
            Language::En => format!("Used {used_pct}% · Free {}", fre_str),
        };

        // 中间大数字显示物理已用空间，而不是磁盘总量
        (cur_name, used_str, used_count_str, items)
    } else {
        let total_children_size: u64 = children.iter().map(|c| c.size).sum();
        let base_size = total_children_size.max(cur_size).max(1);

        let mut items = Vec::new();
        let mut top_sum = 0u64;

        for (i, c) in children.iter().take(4).enumerate() {
            let ratio = (c.size as f64 / base_size as f64).clamp(0.0, 1.0);
            top_sum += c.size;
            items.push(BreakdownItem {
                name: if c.name.is_empty() {
                    match lang {
                        Language::Zh => format!("{}: 根目录", tree.volume()),
                        Language::En => format!("{}: Root", tree.volume()),
                    }
                } else {
                    c.name.clone()
                },
                size: c.size,
                ratio,
                color: colors[i % (colors.len() - 2)],
                is_dir: c.is_dir,
                idx: Some(c.idx),
            });
        }

        if cur_size > top_sum && cur_size - top_sum > 1024 {
            let rem = cur_size - top_sum;
            let ratio = (rem as f64 / base_size as f64).clamp(0.0, 1.0);
            let others_label = match lang {
                Language::Zh => format!("其他 {} 项", children.len().saturating_sub(4)),
                Language::En => format!("Other {} items", children.len().saturating_sub(4)),
            };
            items.push(BreakdownItem {
                name: others_label,
                size: rem,
                ratio,
                color: colors[5],
                is_dir: false,
                idx: None,
            });
        }

        let sub_count_str = match lang {
            Language::Zh => format!("当前目录共 {} 个子项", children.len()),
            Language::En => format!("{} items in folder", children.len()),
        };

        (cur_name, fmt_size(cur_size), sub_count_str, items)
    }
}

/// 大文件榜的占比清单：全盘最大的那批文件按后缀聚类。
fn breakdown_for_files(
    root: &Root,
    tree: &SizeTree,
    colors: &[u32; 6],
) -> (String, String, String, Vec<BreakdownItem>) {
    let lang = root.language;

    let files = tree.largest_files(200);
    let total_files_size: u64 = files.iter().map(|f| f.size).sum();
    let base_size = total_files_size.max(1);

    // 按后缀类型聚类
    let mut media_sz = 0u64;
    let mut bin_sz = 0u64;
    let mut arch_sz = 0u64;
    let mut doc_sz = 0u64;
    let mut other_sz = 0u64;

    for f in &files {
        let ext = std::path::Path::new(&f.name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        match ext.as_str() {
            "mp4" | "mkv" | "avi" | "mov" | "flv" | "mp3" | "wav" => media_sz += f.size,
            "exe" | "dll" | "sys" | "node" | "pak" | "bin" | "msi" => bin_sz += f.size,
            "zip" | "rar" | "7z" | "tar" | "gz" | "iso" | "vmdk" | "vhdx" => arch_sz += f.size,
            "dat" | "db" | "log" | "txt" | "pdf" | "docx" | "xlsx" | "sqlite" => doc_sz += f.size,
            _ => other_sz += f.size,
        }
    }

    let cat_data = match lang {
        Language::Zh => [
            ("媒体视频 (Media)", media_sz, colors[0]),
            ("程序/动态库 (Bin)", bin_sz, colors[1]),
            ("压缩镜像 (Archive)", arch_sz, colors[2]),
            ("文档数据 (Data)", doc_sz, colors[3]),
            ("其他文件 (Others)", other_sz, colors[5]),
        ],
        Language::En => [
            ("Media & Video", media_sz, colors[0]),
            ("Binaries & DLLs", bin_sz, colors[1]),
            ("Archives & Images", arch_sz, colors[2]),
            ("Documents & Data", doc_sz, colors[3]),
            ("Other Files", other_sz, colors[5]),
        ],
    };

    let items: Vec<BreakdownItem> = cat_data
        .into_iter()
        .filter(|(_, sz, _)| *sz > 0)
        .map(|(name, sz, col)| BreakdownItem {
            name: name.to_string(),
            size: sz,
            ratio: (sz as f64 / base_size as f64).clamp(0.0, 1.0),
            color: col,
            is_dir: false,
            idx: None,
        })
        .collect();

    let files_summary_title = match lang {
        Language::Zh => "全盘大文件分布".to_string(),
        Language::En => "Largest Files Breakdown".to_string(),
    };
    let files_count_str = match lang {
        Language::Zh => format!("前 {} 个大文件汇总", files.len()),
        Language::En => format!("Top {} large files", files.len()),
    };

    (
        files_summary_title,
        fmt_size(total_files_size),
        files_count_str,
        items,
    )
}

/// 顶部那条按占比分段着色的横条。
pub(super) fn render_proportion_bar(breakdown: &[BreakdownItem]) -> gpui::Div {
    // 动态多彩占比条
    let mut proportion_bar = div()
        .w_full()
        .h(px(10.))
        .rounded_full()
        .overflow_hidden()
        .bg(rgb(SURF_HIGH))
        .flex();

    for item in breakdown {
        let pct = (item.ratio * 100.0) as f32;
        if pct > 0.5 {
            proportion_bar = proportion_bar.child(
                div()
                    .h_full()
                    .flex_none()
                    .w(gpui::relative(item.ratio as f32))
                    .bg(rgb(item.color)),
            );
        }
    }
    proportion_bar
}
