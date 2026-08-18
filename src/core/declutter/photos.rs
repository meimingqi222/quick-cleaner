//! 相似照片与连拍识别引擎 (Similar Photos Scanner)

use super::{format_timestamp_date, get_user_content_roots, is_photo_extension};
use crate::core::disk::SizeTree;
use crate::core::fs_query::{FSIndexEngine, FileIndexQuery, IndexedFile, QueryFilter};
use rayon::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::Hasher;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhotoItem {
    pub id: String,
    pub filename: String,
    pub path: PathBuf,
    pub path_display: String,
    pub dimensions: (u32, u32),
    pub size: u64,
    pub is_best_shot: bool,
    pub selected: bool,
    pub date_str: String,
    pub bg_gradient_seed: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhotoGroup {
    pub index_str: String,
    pub title_zh: String,
    pub title_en: String,
    pub photos: Vec<PhotoItem>,
}

impl PhotoGroup {
    pub fn cleanable_size(&self) -> u64 {
        self.photos
            .iter()
            .filter(|p| p.selected)
            .map(|p| p.size)
            .sum()
    }

    pub fn selected_count(&self) -> usize {
        self.photos.iter().filter(|p| p.selected).count()
    }
}

/// 扫描相似图片与连拍（通过 FSIndexEngine 快速提取候选 + 并行 dHash 聚类）
pub fn scan_similar_photos(live: &AtomicBool, tree: Option<&SizeTree>) -> Vec<PhotoGroup> {
    let photo_roots = get_user_content_roots();
    let engine = FSIndexEngine::new(tree);
    let filter = QueryFilter::new(photo_roots)
        .max_depth(20)
        .size_range(10_000, 200_000_000);

    let all_files = engine.query_files(&filter, live);

    // 1. 按目录对图片快速分桶（绝大多数相似照片与连拍都在同一目录或工作区）
    let mut dir_map: HashMap<PathBuf, Vec<IndexedFile>> = HashMap::new();
    let mut all_photo_files: Vec<IndexedFile> = Vec::new();

    for f in all_files {
        let ext = f
            .path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if is_photo_extension(&ext) && !is_ignored_photo_path(&f.path) {
            if let Some(parent) = f.path.parent() {
                dir_map
                    .entry(parent.to_path_buf())
                    .or_default()
                    .push(f.clone());
            }
            all_photo_files.push(f);
        }
    }

    // 优先收集目录内 >= 2 张图片的候选文件夹（相册、工作图片库等）
    // 保留 IndexedFile 中的 size/mtime，避免下面再调 std::fs::metadata。
    let mut candidate_set: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let mut candidates_with_meta: Vec<(PathBuf, u64, u64)> = Vec::new(); // (path, size, mtime)
    for (_, files) in dir_map {
        if files.len() >= 2 {
            for f in files {
                if candidate_set.insert(f.path.clone()) {
                    candidates_with_meta.push((f.path.clone(), f.size, f.mtime));
                }
            }
        }
    }

    // 若同目录候选不足，补充其他大图/常见图片
    if candidates_with_meta.len() < 500 {
        for f in all_photo_files {
            if candidate_set.insert(f.path.clone()) {
                candidates_with_meta.push((f.path.clone(), f.size, f.mtime));
                if candidates_with_meta.len() >= 1500 {
                    break;
                }
            }
        }
    }

    if candidates_with_meta.len() < 2 {
        return Vec::new();
    }

    candidates_with_meta.truncate(1500);

    // 2. 并行提取真实尺寸与感知哈希 (dHash)
    // size 和 mtime 已从索引获取，无需再调 std::fs::metadata。
    // 关键：dimensions 与 dual_hash 共用一次 image::open——之前各调一次，
    // 968 张图就是 1936 次完整 JPEG/PNG 解码，合并后砍掉一半解码开销。
    let candidates: Vec<PhotoCandidate> = candidates_with_meta
        .into_par_iter()
        .filter_map(|(path, size, mtime)| {
            if !live.load(std::sync::atomic::Ordering::Relaxed) {
                return None;
            }

            let (dims, dual_hash) = match image::open(&path) {
                Ok(img) => ((img.width(), img.height()), compute_dual_hash_from_image(&img)),
                Err(_) => (estimate_dimensions(size), None),
            };

            let stem_lower = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            let parent = path.parent().map(|p| p.to_path_buf());

            Some(PhotoCandidate {
                path,
                size,
                mtime,
                dimensions: dims,
                dual_hash,
                stem_lower,
                parent,
            })
        })
        .collect();

    if candidates.len() < 2 {
        return Vec::new();
    }

    let total_candidates = candidates.len();

    // 3. 全互联紧密聚类 (Complete-Linkage Clustering)：
    // 组内每个成员必须与该组所有已有成员均满足严格相似，彻底杜绝传递性误合并与超级大组
    let mut group_list: Vec<Vec<PhotoCandidate>> = Vec::new();

    for candidate in candidates {
        let mut matched_group_idx = None;

        for (g_idx, group) in group_list.iter().enumerate() {
            // 每组照片通常为 2~8 张，上限 12 张
            if group.len() >= 12 {
                continue;
            }
            // 组内所有成员必须全部相似
            let fits_all = group
                .iter()
                .all(|member| are_photos_similar(member, &candidate));
            if fits_all {
                matched_group_idx = Some(g_idx);
                break;
            }
        }

        if let Some(idx) = matched_group_idx {
            group_list[idx].push(candidate);
        } else {
            group_list.push(vec![candidate]);
        }
    }

    // 仅保留 >= 2 张照片的组
    let mut group_list: Vec<Vec<PhotoCandidate>> =
        group_list.into_iter().filter(|g| g.len() >= 2).collect();

    // 按可清理体积降序排序
    group_list.sort_by_key(|g| {
        let max_sz = g.iter().map(|c| c.size).max().unwrap_or(0);
        let total_sz: u64 = g.iter().map(|c| c.size).sum();
        std::cmp::Reverse(total_sz.saturating_sub(max_sz))
    });

    let mut result_groups = Vec::new();

    for (idx, mut grp) in group_list.into_iter().enumerate() {
        // 组内按拍摄时间或文件名排序
        grp.sort_by_key(|c| (c.mtime, c.path.clone()));

        let first_stem = grp[0]
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Photos".to_string());

        // 挑选最高分辨率或最大体积作为最佳品质照片
        let max_pixel_count = grp
            .iter()
            .map(|c| (c.dimensions.0 as u64) * (c.dimensions.1 as u64))
            .max()
            .unwrap_or(0);
        let max_size = grp
            .iter()
            .filter(|c| (c.dimensions.0 as u64) * (c.dimensions.1 as u64) == max_pixel_count)
            .map(|c| c.size)
            .max()
            .unwrap_or(0);

        let mut best_marked = false;

        let photos = grp
            .into_iter()
            .enumerate()
            .map(|(p_idx, c)| {
                let filename = c
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                let is_best = if !best_marked
                    && (c.dimensions.0 as u64) * (c.dimensions.1 as u64) == max_pixel_count
                    && c.size == max_size
                {
                    best_marked = true;
                    true
                } else {
                    false
                };

                let seed_color = generate_gradient_seed(&c.path);

                PhotoItem {
                    id: format!("photo-{idx}-{p_idx}"),
                    path_display: c.path.to_string_lossy().to_string(),
                    path: c.path,
                    filename,
                    dimensions: c.dimensions,
                    size: c.size,
                    is_best_shot: is_best,
                    selected: !is_best,
                    date_str: format_timestamp_date(c.mtime),
                    bg_gradient_seed: seed_color,
                }
            })
            .collect();

        result_groups.push(PhotoGroup {
            index_str: format!("{:02}", idx + 1),
            title_zh: format!("连拍/相似组: {first_stem}"),
            title_en: format!("Series: {first_stem}"),
            photos,
        });
    }

    result_groups.truncate(30);
    crate::log!(
        "[Declutter::Photos] 相似照片扫描完成: 候选 {} 张，聚类出 {} 组相似图片",
        total_candidates,
        result_groups.len()
    );
    result_groups
}

#[derive(Clone, Debug)]
struct PhotoCandidate {
    path: PathBuf,
    size: u64,
    mtime: u64,
    dimensions: (u32, u32),
    dual_hash: Option<(u64, u64)>, // (dHash, aHash)
    /// 预计算的小写文件名（不含扩展名），供聚类比较复用。
    /// 全互联聚类里每对候选都要比较 stem，之前每次 are_photos_similar
    /// 都做 2-4 次 to_lowercase() 分配，968 张图就是上万次堆分配。
    stem_lower: String,
    /// 预计算的父目录，避免每次比较都调 path.parent()。
    parent: Option<PathBuf>,
}

/// 计算双重感知指纹 (dHash 结构梯度 + aHash 空间明暗分布)
///
/// 接受已打开的 `&DynamicImage`，与 dimensions 提取共用一次解码。
fn compute_dual_hash_from_image(img: &image::DynamicImage) -> Option<(u64, u64)> {
    let gray = img
        .resize_exact(9, 8, image::imageops::FilterType::Triangle)
        .to_luma8();

    // 1. dHash (64-bit 梯度特征)
    let mut dhash = 0u64;
    let mut sum = 0u32;
    for y in 0..8 {
        for x in 0..8 {
            let left = gray.get_pixel(x, y)[0];
            let right = gray.get_pixel(x + 1, y)[0];
            if left > right {
                dhash |= 1 << (y * 8 + x);
            }
            sum += left as u32;
        }
    }

    // 2. aHash (64-bit 空间明暗分布特征)
    let avg = (sum / 64) as u8;
    let mut ahash = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            let val = gray.get_pixel(x, y)[0];
            if val >= avg {
                ahash |= 1 << (y * 8 + x);
            }
        }
    }

    Some((dhash, ahash))
}

/// 判断两张照片是否为真正的相似照片、连拍或同源变体
fn are_photos_similar(a: &PhotoCandidate, b: &PhotoCandidate) -> bool {
    let same_dir = a.parent == b.parent;

    let (w1, h1) = a.dimensions;
    let (w2, h2) = b.dimensions;
    let same_aspect = if h1 > 0 && h2 > 0 {
        let r1 = w1 as f32 / h1 as f32;
        let r2 = w2 as f32 / h2 as f32;
        (r1 - r2).abs() < 0.08
    } else {
        false
    };

    let stem_a = &a.stem_lower;
    let stem_b = &b.stem_lower;
    let is_burst = is_sequential_camera_name(stem_a, stem_b);
    let same_stem = !stem_a.is_empty()
        && !stem_b.is_empty()
        && (stem_a == stem_b || stem_a.starts_with(stem_b) || stem_b.starts_with(stem_a));

    // 1. 双重感知哈希比较 (Dual Perceptual Hash)
    if let (Some((dh_a, ah_a)), Some((dh_b, ah_b))) = (a.dual_hash, b.dual_hash) {
        let dist_d = (dh_a ^ dh_b).count_ones();
        let dist_a = (ah_a ^ ah_b).count_ones();

        // 强相似度：宽高比吻合 + 梯度相似 + 明暗分布相似
        if same_aspect && dist_d <= 5 && dist_a <= 5 {
            return true;
        }

        // 同目录连拍/同源图：同目录 + 宽高比吻合 + (连拍序列或同基名) + 宽松阈值
        if same_dir && same_aspect && (is_burst || same_stem) && dist_d <= 8 && dist_a <= 8 {
            return true;
        }

        return false;
    }

    // 2. 如果罕见图片格式无法解码哈希，走严格连拍/同名规则
    if same_dir && same_aspect {
        let time_diff = a.mtime.abs_diff(b.mtime);
        let same_dims = a.dimensions == b.dimensions && a.dimensions.0 > 0;
        if is_burst && same_dims && time_diff <= 10 {
            return true;
        }
        if same_stem && same_dims {
            return true;
        }
    }

    false
}

fn is_sequential_camera_name(stem_a: &str, stem_b: &str) -> bool {
    if stem_a.is_empty() || stem_b.is_empty() {
        return false;
    }

    let prefixes = ["img_", "dsc_", "pxl_", "sam_", "dji_", "photo_"];

    for prefix in prefixes {
        if stem_a.starts_with(prefix) && stem_b.starts_with(prefix) {
            return true;
        }
    }

    false
}

fn is_ignored_photo_path(p: &Path) -> bool {
    for comp in p.components() {
        let name = comp.as_os_str().to_string_lossy();
        if name.starts_with('.') {
            return true;
        }
        let s = name.to_lowercase();
        if matches!(
            s.as_str(),
            "node_modules"
                | "target"
                | "dist"
                | "build"
                | "out"
                | "bin"
                | "obj"
                | "pkg"
                | "vendor"
                | "pods"
                | "deriveddata"
                | "bower_components"
                | "venv"
                | "env"
                | "__pycache__"
                | "library"
                | "appdata"
                | "application data"
                | "application support"
                | "cache"
                | "caches"
                | "temp"
                | "tmp"
                | "logs"
                | "gems"
                | "site-packages"
                | "docs"
                | "doc"
                | "documentation"
                | "manual"
                | "manuals"
                | "sdk"
                | "javadoc"
                | "site"
                | "help"
        ) {
            return true;
        }
    }
    let s = p.to_string_lossy().to_lowercase();
    s.contains(".photoslibrary/database")
        || s.contains(".photoslibrary/scopes")
        || s.contains(".photoslibrary/search")
        || s.contains(".photoslibrary/private")
        || s.contains(".photoslibrary/resources")
        || s.contains(".gdb")
}

fn estimate_dimensions(file_size: u64) -> (u32, u32) {
    if file_size > 5_000_000 {
        (4032, 3024)
    } else if file_size > 2_000_000 {
        (3000, 2000)
    } else if file_size > 800_000 {
        (2048, 1536)
    } else {
        (1920, 1080)
    }
}

fn generate_gradient_seed(path: &Path) -> u32 {
    let mut hasher = DefaultHasher::new();
    hasher.write(path.to_string_lossy().as_bytes());
    let h = hasher.finish();
    let palettes = [
        0x0284c7, 0x0369a1, 0x075985, 0x475569, 0x334155, 0x4f46e5, 0x4338ca, 0x0d9488, 0x0f766e,
    ];
    palettes[(h as usize) % palettes.len()]
}
