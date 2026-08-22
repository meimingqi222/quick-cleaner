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
    /// 平台不允许由本进程完成，必须用户自己动手。
    ///
    /// 和 `Failed` 的区别在于「重试没有意义」：SIP 开启时的系统扩展就算再点
    /// 一百次也不会消失，正确出路是去系统设置。混进 `Failed` 会让 UI 报
    /// 「权限不足」，把平台限制说成软件出错。
    ManualAction,
}

/// 一次清理里没能删掉的目标。
///
/// 多数是文件路径，但注册表键、计划任务、系统扩展这些**没有路径**。以前它们
/// 被硬塞进 `PathBuf`（`PathBuf::from("回收站")`、`PathBuf::from(bundle_id)`），
/// 类型上说了谎——任何拿这个列表去 `exists()` 或「在 Finder 中显示」的代码
/// 都会出错。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CleanFailure {
    Path(PathBuf),
    /// 非路径目标的标识串：注册表键、计划任务、系统扩展 Bundle ID……
    Id(String),
}

impl CleanFailure {
    /// 只有真正是路径的目标才返回 `Some`，供「在 Finder 中显示」这类操作使用。
    pub fn as_path(&self) -> Option<&Path> {
        match self {
            CleanFailure::Path(path) => Some(path),
            CleanFailure::Id(_) => None,
        }
    }

    pub fn label(&self) -> String {
        match self {
            CleanFailure::Path(path) => path.display().to_string(),
            CleanFailure::Id(id) => id.clone(),
        }
    }
}

impl From<&Path> for CleanFailure {
    fn from(path: &Path) -> Self {
        CleanFailure::Path(path.to_path_buf())
    }
}

/// 一次清理的汇总结果。没删掉的目标会被记录下来供 UI 展示。
#[derive(Clone, Debug, Default)]
pub struct CleanReport {
    pub ok: usize,
    pub skipped: usize,
    pub failed: Vec<CleanFailure>,
    /// 需要用户手动处理的目标。重试无意义，因此和 `failed` 分开计数，
    /// 否则 UI 会把平台限制报成「权限不足」。
    pub manual: Vec<CleanFailure>,
}

impl CleanReport {
    pub fn record(&mut self, path: &Path, r: CleanResult) {
        self.record_target(CleanFailure::from(path), r);
    }

    pub fn record_target(&mut self, target: CleanFailure, r: CleanResult) {
        match r {
            CleanResult::Ok => self.ok += 1,
            CleanResult::Skipped => self.skipped += 1,
            CleanResult::Failed => self.failed.push(target),
            CleanResult::ManualAction => self.manual.push(target),
        }
    }

    pub fn merge(&mut self, other: CleanReport) {
        self.ok += other.ok;
        self.skipped += other.skipped;
        self.failed.extend(other.failed);
        self.manual.extend(other.manual);
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
///
/// 每一层都重新过一遍 [`is_protected`]。以前只有入口 [`clean_path`] 查一次，
/// 递归下去全程不设防——那依赖于「保护表里的目录只能从更上层进入，而上层
/// 自己也受保护」这个巧合，而不是设计。这里是整个程序唯一真正动手删东西的
/// 地方，纵深防御应该做在这一层。
///
/// 代价是每个节点多一次路径归一化（一次 String 分配）。相对于紧随其后的
/// `DeleteFileW` 系统调用，这点开销可以忽略。
pub fn delete_tree(path: &Path, p: &CleanProgress) -> CleanResult {
    if p.cancelled() {
        return CleanResult::Skipped;
    }
    if is_protected(path) {
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
        return delete_file(path, allocated_file_size(&md), p);
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
                    let size = entry
                        .metadata()
                        .map(|metadata| allocated_file_size(&metadata))
                        .unwrap_or(0);
                    files.push((entry.path(), size));
                }
            }
        }
        Err(_) => {
            p.failed.fetch_add(1, Ordering::Relaxed);
            return CleanResult::Failed;
        }
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
        // 文件删不掉会记进 `p.failed`（见 delete_file），目录删不掉以前
        // 只体现在返回值上，进度条里的失败数因此偏少。
        p.failed.fetch_add(1, Ordering::Relaxed);
        CleanResult::Failed
    }
}

fn allocated_file_size(metadata: &std::fs::Metadata) -> u64 {
    #[cfg(windows)]
    {
        metadata.len()
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::MetadataExt;
        metadata.blocks().saturating_mul(512)
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
    // tmutil:// 虚拟路径：APFS 本地快照，用 tmutil deletelocalsnapshots 删除
    let path_str = path.to_string_lossy();
    if let Some(snapshot) = path_str.strip_prefix("tmutil://snapshot/") {
        p.note(path);
        let status = std::process::Command::new("tmutil")
            .arg("deletelocalsnapshots")
            .arg(snapshot)
            .status();
        return match status {
            Ok(s) if s.success() => CleanResult::Ok,
            Ok(_) => CleanResult::Failed,
            Err(_) => CleanResult::Failed,
        };
    }

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

    // read_dir 会跟随作为根目标的目录符号链接。固定扫描完成后路径仍可能
    // 被其它进程替换，因此删除层必须再次用 symlink_metadata 阻断，不能
    // 依赖扫描阶段的检查。
    let Ok(md) = std::fs::symlink_metadata(dir) else {
        report.skipped += 1;
        return report;
    };
    if md.file_type().is_symlink() || !md.is_dir() || is_protected(dir) {
        report.failed.push(CleanFailure::Path(dir.to_path_buf()));
        return report;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => {
            if dir.exists() {
                report.failed.push(CleanFailure::Path(dir.to_path_buf()));
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

/// 删完之后的结果行：成功/跳过多少，失败的具体是哪些。
///
/// 失败清单封顶 20 条。一次清理如果失败几万个（多半是整个目录被占用），
/// 前 20 条已经足够看出是哪一类问题了。
fn audit_result(report: &CleanReport, p: &CleanProgress) {
    const MAX_LISTED: usize = 20;
    let snap = p.snapshot();
    let shown: Vec<String> = report
        .failed
        .iter()
        .take(MAX_LISTED)
        .map(CleanFailure::label)
        .collect();
    let more = report.failed.len().saturating_sub(shown.len());

    crate::log!(
        "[删除] 完成：目标 ok={} skipped={} failed={}；文件 {} 个 / {}，失败 {} 个{}{}",
        report.ok,
        report.skipped,
        report.failed.len(),
        snap.files,
        crate::core::model::fmt_size(snap.bytes),
        snap.failed,
        if shown.is_empty() {
            String::new()
        } else {
            format!("；失败清单：{}", shown.join(" | "))
        },
        if more > 0 {
            format!("（另有 {more} 条未列出）")
        } else {
            String::new()
        }
    );
}

/// 一次删除动作的审计日志。
///
/// 本程序**永久删除**文件，没有回收站可退。用户回来说「它删了不该删的东西」
/// 的时候，如果日志里只有扫描耗时，那就等于什么都没有。这里记下每一批的
/// 目标清单，出事时至少有据可查。
///
/// 只记目标（用户勾选的那一层），不记递归展开出的每个文件——一次清理动辄
/// 几十万个文件，全记下来日志会先被自己撑爆，而定位问题靠的是顶层目标。
fn audit(action: &str, paths: impl Iterator<Item = PathBuf>) {
    let list: Vec<String> = paths.map(|p| p.display().to_string()).collect();
    crate::log!(
        "[删除] {action}，共 {} 个目标：{}",
        list.len(),
        list.join(" | ")
    );
}

/// 一个清理目标及其处置方式。
#[derive(Clone, Debug)]
pub struct CleanTarget {
    pub path: PathBuf,
    /// 连目录本身一起删，还是只清空内容。
    ///
    /// 见 `CategoryId::removes_directory`：系统缓存目录要保留（大量程序
    /// 假定它们存在），开发产物目录要删干净（空的 `.venv` / `node_modules`
    /// 比不存在更糟）。
    pub remove_dir: bool,
}

impl CleanTarget {
    /// 只清空内容，保留目录本身。
    pub fn empty(path: PathBuf) -> Self {
        Self {
            path,
            remove_dir: false,
        }
    }

    /// 连目录一起删。
    pub fn remove(path: PathBuf) -> Self {
        Self {
            path,
            remove_dir: true,
        }
    }
}

/// 清理多个扫描目标。
pub fn clean_targets(targets: &[CleanTarget], p: &CleanProgress) -> CleanReport {
    audit("分类清理", targets.iter().map(|t| t.path.clone()));
    let mut report = CleanReport::default();
    let mut bin_done = false;
    for t in targets {
        if p.cancelled() {
            break;
        }
        let d = &t.path;
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

        #[cfg(target_os = "macos")]
        if is_launch_agent_plist(d) {
            let result = match crate::platform::macos::trash::move_to_trash(d) {
                Ok(()) => {
                    p.files.fetch_add(1, Ordering::Relaxed);
                    CleanResult::Ok
                }
                Err(_) => {
                    p.failed.fetch_add(1, Ordering::Relaxed);
                    CleanResult::Failed
                }
            };
            report.record(d, result);
            continue;
        }

        if t.remove_dir {
            report.record(d, clean_path(d, p));
        } else {
            report.merge(clean_dir_contents(d, p));
        }
    }
    audit_result(&report, p);
    report
}

#[cfg(target_os = "macos")]
fn is_launch_agent_plist(path: &Path) -> bool {
    if path.extension().is_none_or(|ext| ext != "plist") {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    parent == Path::new("/Library/LaunchAgents")
        || dirs::home_dir().is_some_and(|home| parent == home.join("Library/LaunchAgents"))
}

/// 手选路径的处置方式。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Disposal {
    /// 直接抹掉，立刻释放空间。
    #[default]
    Permanent,
    /// 送进回收站，可以还原——但**不释放磁盘空间**。
    RecycleBin,
}

/// 对用户在磁盘分析里手动选中的任意路径执行清理。
///
/// `disposal` 只影响手选路径。分类清理走的是固定白名单表（缓存、临时文件、
/// 构建产物），把它们塞进回收站没有意义，只会让用户再清一次。
pub fn clean_arbitrary(paths: &[PathBuf], disposal: Disposal, p: &CleanProgress) -> CleanReport {
    audit(
        match disposal {
            Disposal::Permanent => "用户手选路径（永久删除）",
            Disposal::RecycleBin => "用户手选路径（送回收站）",
        },
        paths.iter().cloned(),
    );
    let mut report = CleanReport::default();
    for path in paths {
        if p.cancelled() {
            break;
        }
        p.note(path);
        if is_protected(path) {
            report.failed.push(CleanFailure::Path(path.clone()));
            continue;
        }
        report.record(path, dispose(path, disposal, p));
    }
    audit_result(&report, p);
    report
}

/// 按处置方式删掉一个路径。
///
/// 回收站失败时**不**回退到永久删除：用户开这个开关就是要「删错了能捞
/// 回来」，悄悄替他永久删掉是把安全网抽走。如实报失败，让他自己决定。
fn dispose(path: &Path, disposal: Disposal, p: &CleanProgress) -> CleanResult {
    match disposal {
        Disposal::Permanent => clean_path(path, p),
        Disposal::RecycleBin => recycle_path(path, p),
    }
}

#[cfg(windows)]
fn recycle_path(path: &Path, p: &CleanProgress) -> CleanResult {
    if p.cancelled() {
        return CleanResult::Skipped;
    }
    if std::fs::symlink_metadata(path).is_err() {
        return CleanResult::Skipped;
    }
    if is_protected(path) {
        return CleanResult::Failed;
    }

    if crate::platform::windows::move_to_recycle_bin(path) {
        // 回收站不释放空间，所以这里只记条目数，不往 bytes 上加——
        // 界面上「已释放 X」必须是真的释放了才算。
        p.files.fetch_add(1, Ordering::Relaxed);
        CleanResult::Ok
    } else {
        p.failed.fetch_add(1, Ordering::Relaxed);
        CleanResult::Failed
    }
}

/// 非 Windows 平台没有等价的回收站 API，一律按永久删除处理。
#[cfg(not(windows))]
fn recycle_path(path: &Path, p: &CleanProgress) -> CleanResult {
    clean_path(path, p)
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

    fn allocated_tree_size(path: &Path) -> u64 {
        walkdir::WalkDir::new(path)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .filter_map(|entry| entry.metadata().ok())
            .map(|metadata| allocated_file_size(&metadata))
            .sum()
    }

    /// `ManualAction` 不能混进 `failed`：SIP 下的系统扩展重试没有意义，
    /// 报成失败会让 UI 说「权限不足」，把平台限制说成软件出错。
    #[test]
    fn manual_action_is_counted_separately_from_failure() {
        let mut report = CleanReport::default();
        report.record_target(
            CleanFailure::Id("org.pqrs.Driver".into()),
            CleanResult::ManualAction,
        );
        report.record_target(
            CleanFailure::Path(PathBuf::from("/tmp/x")),
            CleanResult::Failed,
        );
        report.record(Path::new("/tmp/y"), CleanResult::Ok);

        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.manual.len(), 1);
        assert_eq!(report.ok, 1);

        let mut other = CleanReport::default();
        other.merge(report);
        assert_eq!(other.manual.len(), 1, "merge 不能丢掉手动处理项");
    }

    /// 非路径目标不再伪装成 `PathBuf`：拿去做「在 Finder 中显示」要拿不到路径。
    #[test]
    fn non_path_targets_expose_no_path() {
        assert_eq!(CleanFailure::Id("org.pqrs.Driver".into()).as_path(), None);
        assert_eq!(
            CleanFailure::Path(PathBuf::from("/tmp/x")).as_path(),
            Some(Path::new("/tmp/x"))
        );
        assert_eq!(CleanFailure::Id("回收站".into()).label(), "回收站");
    }

    #[test]
    fn progress_counts_match_actual_tree() {
        let base = make_tree("counts", 30, 512);
        let expected_bytes = allocated_tree_size(&base);
        let p = CleanProgress::new(30, expected_bytes);

        let report = clean_dir_contents(&base, &p);
        let snap = p.snapshot();

        assert_eq!(snap.files, 30);
        assert_eq!(snap.bytes, expected_bytes);
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
        let expected_bytes = allocated_tree_size(&base);
        let p = CleanProgress::new(12, expected_bytes);

        assert_eq!(clean_path(&base, &p), CleanResult::Ok);
        let snap = p.snapshot();
        assert_eq!(snap.files, 12);
        assert_eq!(snap.bytes, expected_bytes);
        assert!(!base.exists());
    }

    #[cfg(unix)]
    #[test]
    fn emptying_symlink_root_never_touches_target() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join("qc_symlink_root_safety");
        let target = root.join("target");
        let link = root.join("cache-link");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("keep.txt"), b"important").unwrap();
        symlink(&target, &link).unwrap();

        let report = clean_dir_contents(&link, &CleanProgress::default());

        assert_eq!(report.failed, vec![CleanFailure::Path(link)]);
        assert_eq!(
            std::fs::read(target.join("keep.txt")).unwrap(),
            b"important"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn only_launch_agent_plists_use_special_cleanup() {
        let home = dirs::home_dir().unwrap();
        assert!(is_launch_agent_plist(
            &home.join("Library/LaunchAgents/com.example.broken.plist")
        ));
        assert!(is_launch_agent_plist(Path::new(
            "/Library/LaunchAgents/com.example.broken.plist"
        )));
        assert!(!is_launch_agent_plist(
            &home.join("Library/LaunchAgents/notes.txt")
        ));
        assert!(!is_launch_agent_plist(Path::new(
            "/Library/LaunchDaemons/com.example.plist"
        )));
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
        assert!(is_recycle_junk_entry(
            ".msys00010000000d0695f1e810a56094d18e"
        ));
        assert!(is_recycle_junk_entry(
            ".xxxx00010000000d0695f1e810a56094d18e"
        ));
        assert!(!is_recycle_junk_entry("desktop.ini"));
        assert!(!is_recycle_junk_entry("Desktop.ini"));
        assert!(!is_recycle_junk_entry("DESKTOP.INI"));
    }

    #[test]
    fn ratio_is_bounded() {
        let s = CleanSnapshot {
            total_files: 10,
            files: 999,
            ..Default::default()
        };
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

        assert_eq!(
            clean_path(&base, &CleanProgress::default()),
            CleanResult::Ok
        );
        assert!(!base.exists());
    }
}
