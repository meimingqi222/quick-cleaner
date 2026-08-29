//! 应用图标进程内缓存
//!
//! 应用列表可能有一两百个 .app，逐个在主线程提取图标会卡 UI。
//! 这里维护一个全局缓存，后台线程提取 PNG bytes 后转成
//! `Arc<gpui::Image>` 存入，UI 渲染时直接取用。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use gpui::{Image, ImageFormat};

/// 缓存条目：`Some` 表示已成功提取，`None` 表示尝试过但没拿到
/// （避免反复尝试不存在的图标）。
type CacheEntry = Option<Arc<Image>>;

static ICON_CACHE: Mutex<Option<HashMap<PathBuf, CacheEntry>>> = Mutex::new(None);

/// 从缓存中取图标。
///
/// - 返回 `Some(img)`：缓存命中且有图标
/// - 返回 `None`：尚未加载，或已尝试但该应用没有可用图标
///   （调用方应回退到首字母占位符）
pub fn try_get_icon(path: &Path) -> Option<Arc<Image>> {
    let cache = ICON_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache
        .as_ref()
        .and_then(|map| map.get(path))
        .and_then(|entry| entry.clone())
}

/// 这个路径是否已经尝试过（无论成功与否）。
///
/// 与 [`try_get_icon`] 的区别：那个只在**有图标**时返回 Some，取不到图标的
/// 路径每次都像没加载过。进程表每 2 秒采一拍，靠它才能把「已经确认没图标」
/// 的进程排除掉，否则每一拍都要重新去读一遍 bundle。
pub fn is_cached(path: &Path) -> bool {
    let cache = ICON_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.as_ref().is_some_and(|map| map.contains_key(path))
}

/// 在后台线程提取一个应用的图标并存入缓存。
///
/// 返回 `true` 表示成功提取到图标，`false` 表示该应用没有可用图标。
/// 已缓存的路径不会重复提取。
pub fn load_icon(path: PathBuf) -> bool {
    // 先检查是否已缓存
    {
        let cache = ICON_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(map) = cache.as_ref() {
            if map.contains_key(&path) {
                return map.get(&path).and_then(|e| e.clone()).is_some();
            }
        }
    }

    // 平台提取
    let png = load_icon_platform(&path);
    let image = png.map(|bytes| Arc::new(Image::from_bytes(ImageFormat::Png, bytes)));

    let mut cache = ICON_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let map = cache.get_or_insert_with(HashMap::new);
    map.insert(path, image.clone());

    image.is_some()
}

/// 先从 bundle 并行抽图标，返回还需要走 AppKit 的路径。
///
/// 这一步是纯文件读取，一百个应用通常几十毫秒；UI 应在返回后立刻重绘，
/// 不要等 NSWorkspace 回退。
pub fn load_icons_from_bundle(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    use rayon::prelude::*;
    paths
        .into_par_iter()
        .filter_map(|path| {
            if try_get_icon(&path).is_some() {
                return None;
            }
            match load_icon_from_bundle(&path) {
                true => None,
                false => Some(path),
            }
        })
        .collect()
}

fn load_icon_from_bundle(path: &Path) -> bool {
    let png = {
        #[cfg(any(target_os = "macos", windows))]
        {
            crate::platform::app_icon_from_bundle(path)
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            None
        }
    };
    let Some(bytes) = png else {
        return false;
    };
    let image = Arc::new(Image::from_bytes(ImageFormat::Png, bytes));
    let mut cache = ICON_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let map = cache.get_or_insert_with(HashMap::new);
    map.insert(path.to_path_buf(), Some(image));
    true
}

/// 批量加载图标，返回成功加载的数量。
///
/// 会走 AppKit 回退，供 bundle 里抽不到 PNG 的少数应用使用。
pub fn load_icons(paths: Vec<PathBuf>) -> usize {
    paths.into_iter().filter(|p| load_icon(p.clone())).count()
}

/// 清空缓存（应用列表整体刷新时调用）。
pub fn clear() {
    let mut cache = ICON_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    *cache = None;
}

#[cfg(any(target_os = "macos", windows))]
fn load_icon_platform(path: &Path) -> Option<Vec<u8>> {
    crate::platform::app_icon_png(path)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn load_icon_platform(_path: &Path) -> Option<Vec<u8>> {
    None
}
