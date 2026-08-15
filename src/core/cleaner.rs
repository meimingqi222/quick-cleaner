//! 核心清理引擎与安全防护

use crate::core::safety::is_protected;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

/// 清理进度。后台线程边删边更新，UI 定时读快照渲染。
#[derive(Debug, Default)]
pub struct CleanProgress {
    /// 预期要删的文件总数（来自扫描结果，用于算百分比）
    pub total_files: AtomicU64,
    /// 预期要释放的总字节数
    pub total_bytes: AtomicU64,
    /// 已删除的文件数
    pub files: AtomicU64,
    /// 已释放的字节数（删除前从目录枚举结果里读到的真实大小）
    pub bytes: AtomicU64,
    /// 删除失败的条目数
    pub failed: AtomicU64,
    /// 置位后后台线程会尽快停下
    pub cancel: AtomicBool,
    /// 当前正在处理的路径，只用于显示
    pub current: Mutex<String>,
}

/// 某一刻的进度快照，给 UI 用。
#[derive(Clone, Debug, Default)]
pub struct CleanSnapshot {
    pub total_files: u64,
    pub total_bytes: u64,
    pub files: u64,
    pub bytes: u64,
    pub failed: u64,
    pub cancelled: bool,
    pub current: String,
}

impl CleanProgress {
    pub fn new(total_files: u64, total_bytes: u64) -> Self {
        Self {
            total_files: AtomicU64::new(total_files),
            total_bytes: AtomicU64::new(total_bytes),
            ..Default::default()
        }
    }

    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// 记下当前处理的路径。用 try_lock 减少竞争。
    pub fn note(&self, path: &Path) {
        if let Ok(mut c) = self.current.try_lock() {
            c.clear();
            c.push_str(&path.to_string_lossy());
        }
    }

    pub fn snapshot(&self) -> CleanSnapshot {
        CleanSnapshot {
            total_files: self.total_files.load(Ordering::Relaxed),
            total_bytes: self.total_bytes.load(Ordering::Relaxed),
            files: self.files.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            cancelled: self.cancelled(),
            current: self.current.lock().map(|c| c.clone()).unwrap_or_default(),
        }
    }
}

impl CleanSnapshot {
    /// 完成比例，0.0~1.0。优先按文件数算，没有文件数基准时退回按字节。
    pub fn ratio(&self) -> f32 {
        if self.total_files > 0 {
            (self.files as f64 / self.total_files as f64).clamp(0., 1.) as f32
        } else if self.total_bytes > 0 {
            (self.bytes as f64 / self.total_bytes as f64).clamp(0., 1.) as f32
        } else {
            0.
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CleanResult {
    Ok,
    Skipped,
    Failed,
}

/// 一次清理的汇总结果。失败路径会被记录下来供 UI 展示。
#[derive(Clone, Debug, Default)]
pub struct CleanReport {
    pub ok: usize,
    pub skipped: usize,
    pub failed: Vec<PathBuf>,
}

impl CleanReport {
    pub fn record(&mut self, path: &Path, r: CleanResult) {
        match r {
            CleanResult::Ok => self.ok += 1,
            CleanResult::Skipped => self.skipped += 1,
            CleanResult::Failed => self.failed.push(path.to_path_buf()),
        }
    }

    pub fn merge(&mut self, other: CleanReport) {
        self.ok += other.ok;
        self.skipped += other.skipped;
        self.failed.extend(other.failed);
    }
}

/// 清掉只读位。
fn clear_readonly(path: &Path, md: &std::fs::Metadata) {
    let mut perms = md.permissions();
    if !perms.readonly() {
        return;
    }
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    let _ = std::fs::set_permissions(path, perms);
}

/// 递归删除一棵树，边删边把进度记到 `p` 上。
pub fn delete_tree(path: &Path, p: &CleanProgress) -> CleanResult {
    if p.cancelled() {
        return CleanResult::Skipped;
    }

    let md = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return CleanResult::Skipped,
    };
    let ft = md.file_type();

    // 链接/junction：只删链接本身，绝不递归进目标
    if ft.is_symlink() {
        let ok = std::fs::remove_dir(path).is_ok() || std::fs::remove_file(path).is_ok();
        return if ok {
            p.files.fetch_add(1, Ordering::Relaxed);
            CleanResult::Ok
        } else {
            p.failed.fetch_add(1, Ordering::Relaxed);
            CleanResult::Failed
        };
    }

    if !ft.is_dir() {
        return delete_file(path, md.len(), p);
    }

    // 目录：先把内容清空，再删自己
    p.note(path);
    let mut subdirs: Vec<PathBuf> = Vec::new();
    let mut files: Vec<(PathBuf, u64)> = Vec::new();
    match std::fs::read_dir(path) {
        Ok(rd) => {
            for entry in rd.flatten() {
                let Ok(ft) = entry.file_type() else { continue };
                if ft.is_symlink() || ft.is_dir() {
                    subdirs.push(entry.path());
                } else {
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    files.push((entry.path(), size));
                }
            }
        }
        Err(_) => return CleanResult::Failed,
    }

    let files_failed = files
        .par_iter()
        .filter(|(f, size)| delete_file(f, *size, p) == CleanResult::Failed)
        .count();

    let subs_failed = subdirs
        .par_iter()
        .filter(|d| delete_tree(d, p) == CleanResult::Failed)
        .count();

    if p.cancelled() {
        return CleanResult::Skipped;
    }
    if std::fs::remove_dir(path).is_ok() && files_failed == 0 && subs_failed == 0 {
        CleanResult::Ok
    } else {
        CleanResult::Failed
    }
}

/// 删单个文件并记账。只读位挡路时清掉再试一次。
fn delete_file(path: &Path, size: u64, p: &CleanProgress) -> CleanResult {
    if p.cancelled() {
        return CleanResult::Skipped;
    }
    if !remove_file_forcing(path) {
        p.failed.fetch_add(1, Ordering::Relaxed);
        return CleanResult::Failed;
    }
    p.files.fetch_add(1, Ordering::Relaxed);
    p.bytes.fetch_add(size, Ordering::Relaxed);
    CleanResult::Ok
}

fn remove_file_forcing(path: &Path) -> bool {
    if std::fs::remove_file(path).is_ok() {
        return true;
    }
    if let Ok(md) = std::fs::symlink_metadata(path) {
        clear_readonly(path, &md);
    }
    std::fs::remove_file(path).is_ok()
}

/// 清理单个路径本身（连同其内容）。
pub fn clean_path(path: &Path, p: &CleanProgress) -> CleanResult {
    if std::fs::symlink_metadata(path).is_err() {
        return CleanResult::Skipped;
    }
    if is_protected(path) {
        return CleanResult::Failed;
    }
    delete_tree(path, p)
}

/// 清理目录**内容**但保留目录本身。
pub fn clean_dir_contents(dir: &Path, p: &CleanProgress) -> CleanReport {
    let mut report = CleanReport::default();

    let entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => {
            if dir.exists() {
                report.failed.push(dir.to_path_buf());
            } else {
                report.skipped += 1;
            }
            return report;
        }
    };

    let children: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    let merged = children
        .par_iter()
        .map(|c| {
            let mut r = CleanReport::default();
            if p.cancelled() {
                r.skipped += 1;
            } else {
                r.record(c, clean_path(c, p));
            }
            r
        })
        .reduce(CleanReport::default, |mut a, b| {
            a.merge(b);
            a
        });
    report.merge(merged);
    report
}

/// 清理多个扫描目标（每个目标清空其内容，保留目录本身）。
pub fn clean_targets(dirs: &[PathBuf], p: &CleanProgress) -> CleanReport {
    let mut report = CleanReport::default();
    let mut bin_done = false;
    for d in dirs {
        if p.cancelled() {
            break;
        }
        p.note(d);
        #[cfg(windows)]
        if crate::platform::windows::recycle::is_recycle_bin(d) {
            if !bin_done {
                report.merge(crate::platform::windows::recycle::empty_recycle_bin(p));
                bin_done = true;
            }
            continue;
        }
        #[cfg(target_os = "macos")]
        if d.to_string_lossy().contains(".Trash") {
            if !bin_done {
                report.merge(crate::platform::macos::trash::empty_trash(p));
                bin_done = true;
            }
            continue;
        }
        report.merge(clean_dir_contents(d, p));
    }
    report
}

/// 对用户在磁盘分析里手动选中的任意路径执行清理。
pub fn clean_arbitrary(paths: &[PathBuf], p: &CleanProgress) -> CleanReport {
    let mut report = CleanReport::default();
    for path in paths {
        if p.cancelled() {
            break;
        }
        p.note(path);
        if is_protected(path) {
            report.failed.push(path.clone());
            continue;
        }
        report.record(path, clean_path(path, p));
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    use crate::platform::windows::recycle::is_recycle_junk_entry;
    #[cfg(windows)]
    use crate::platform::windows::security::current_user_sid;

    fn make_tree(tag: &str, n_files: usize, size: usize) -> PathBuf {
        let base = std::env::temp_dir().join(format!("qc_prog_{tag}"));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("a").join("deep")).unwrap();
        std::fs::create_dir_all(base.join("b")).unwrap();
        let blob = vec![b'x'; size];
        for i in 0..n_files {
            let dir = match i % 3 {
                0 => base.join("a"),
                1 => base.join("a").join("deep"),
                _ => base.join("b"),
            };
            std::fs::write(dir.join(format!("f{i}.bin")), &blob).unwrap();
        }
        base
    }

    #[test]
    fn progress_counts_match_actual_tree() {
        let base = make_tree("counts", 30, 512);
        let p = CleanProgress::new(30, 30 * 512);

        let report = clean_dir_contents(&base, &p);
        let snap = p.snapshot();

        assert_eq!(snap.files, 30);
        assert_eq!(snap.bytes, 30 * 512);
        assert_eq!(snap.failed, 0);
        assert!(report.failed.is_empty());
        assert!((snap.ratio() - 1.0).abs() < 1e-6);

        assert!(base.exists());
        assert_eq!(std::fs::read_dir(&base).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn clean_path_removes_root_and_counts() {
        let base = make_tree("root", 12, 256);
        let p = CleanProgress::new(12, 12 * 256);

        assert_eq!(clean_path(&base, &p), CleanResult::Ok);
        let snap = p.snapshot();
        assert_eq!(snap.files, 12);
        assert_eq!(snap.bytes, 12 * 256);
        assert!(!base.exists());
    }

    #[test]
    fn cancel_stops_deletion_early() {
        let base = make_tree("cancel", 20, 128);
        let p = CleanProgress::new(20, 20 * 128);
        p.request_cancel();

        let _ = clean_dir_contents(&base, &p);
        let snap = p.snapshot();
        assert!(snap.cancelled);
        assert_eq!(snap.files, 0);
        assert!(base.exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(windows)]
    #[test]
    fn test_current_user_sid() {
        let sid = current_user_sid().expect("应该能拿到当前用户 SID");
        assert!(sid.starts_with("S-1-"));
        assert!(sid.len() > 8);
        assert!(sid.matches('-').count() >= 3);
    }

    #[cfg(windows)]
    #[test]
    fn recycle_sweep_keeps_only_desktop_ini() {
        assert!(is_recycle_junk_entry("$IABC123"));
        assert!(is_recycle_junk_entry("$RABC123"));
        assert!(is_recycle_junk_entry(".msys00010000000d0695f1e810a56094d18e"));
        assert!(is_recycle_junk_entry(".xxxx00010000000d0695f1e810a56094d18e"));
        assert!(!is_recycle_junk_entry("desktop.ini"));
        assert!(!is_recycle_junk_entry("Desktop.ini"));
        assert!(!is_recycle_junk_entry("DESKTOP.INI"));
    }

    #[test]
    fn ratio_is_bounded() {
        let s = CleanSnapshot { total_files: 10, files: 999, ..Default::default() };
        assert!((s.ratio() - 1.0).abs() < 1e-6);
        assert_eq!(CleanSnapshot::default().ratio(), 0.0);
    }

    #[cfg(windows)]
    #[test]
    fn locked_file_is_skipped_and_rest_continues() {
        use std::os::windows::fs::OpenOptionsExt;

        let base = make_tree("locked", 24, 128);
        let locked = base.join("a").join("f0.bin");
        assert!(locked.exists());
        let _guard = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&locked)
            .expect("应该能独占打开");

        let p = CleanProgress::new(24, 24 * 128);
        let report = clean_dir_contents(&base, &p);
        let snap = p.snapshot();

        assert!(locked.exists());
        assert_eq!(snap.files, 23);
        assert!(snap.failed >= 1);
        assert!(!snap.cancelled);
        assert!(!report.failed.is_empty());
        assert!(!base.join("b").exists());

        drop(_guard);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn deletes_readonly_tree() {
        let base = std::env::temp_dir().join("qc_readonly_test_9f3a");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("sub")).unwrap();
        let f = base.join("sub").join("ro.txt");
        std::fs::write(&f, b"x").unwrap();

        let mut perms = std::fs::metadata(&f).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&f, perms).unwrap();
        assert!(std::fs::metadata(&f).unwrap().permissions().readonly());

        assert_eq!(clean_path(&base, &CleanProgress::default()), CleanResult::Ok);
        assert!(!base.exists());
    }

}
