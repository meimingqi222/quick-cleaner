//! macOS 软件残留深度扫描骨架

use crate::core::apps::{
    InstalledApp, ResidualItem, ResidualKind, ResidualScanResult, ResidualSource,
};
use crate::core::cleaner::{clean_path, CleanProgress, CleanReport};
use std::path::PathBuf;

pub fn scan_residuals(app: &InstalledApp) -> ResidualScanResult {
    let mut items = Vec::new();
    let mut total_file_size = 0u64;

    if let Some(home) = dirs::home_dir() {
        let app_support = home.join("Library/Application Support").join(&app.name);
        if app_support.exists() {
            let size = walkdir::WalkDir::new(&app_support)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum();
            total_file_size += size;
            items.push(ResidualItem::certain(
                ResidualKind::Directory(app_support, size),
                ResidualSource::AppSupportDir,
            ));
        }
    }

    ResidualScanResult {
        app_name: app.name.clone(),
        items,
        total_file_size,
    }
}

pub fn clean_residuals(items: &[ResidualItem], prog: &CleanProgress) -> CleanReport {
    let mut report = CleanReport::default();
    for item in items {
        if let ResidualKind::Directory(path, _) | ResidualKind::File(path, _) = &item.kind {
            prog.note(path);
            let res = clean_path(path, prog);
            report.record(path, res);
        }
    }
    report
}

/// 复核候选残留是否仍然存在（对应 Windows 侧的「先扫描后卸载」流程）。
pub fn verify_residuals(items: Vec<ResidualItem>) -> Vec<ResidualItem> {
    items
        .into_iter()
        .filter(|it| match &it.kind {
            ResidualKind::File(p, _) | ResidualKind::Directory(p, _) => p.exists(),
            _ => true,
        })
        .collect()
}
