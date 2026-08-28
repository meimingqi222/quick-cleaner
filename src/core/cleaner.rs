//! 核心清理引擎与安全防护

use crate::core::model::{snapshot_name, TargetIdentity};
use crate::core::safety::is_protected;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

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
    /// 策略拒绝（保护路径、白名单、取消）的具体目标，供 UI 和「还在磁盘
    /// 上」区分开：还在不等于失败。
    pub skipped_items: Vec<CleanFailure>,
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
            CleanResult::Skipped => {
                self.skipped += 1;
                self.skipped_items.push(target);
            }
            CleanResult::Failed => self.failed.push(target),
            CleanResult::ManualAction => self.manual.push(target),
        }
    }

    pub fn merge(&mut self, other: CleanReport) {
        self.ok += other.ok;
        self.skipped += other.skipped;
        self.skipped_items.extend(other.skipped_items);
        self.failed.extend(other.failed);
        self.manual.extend(other.manual);
    }

    /// 这条路径是不是被策略跳过（白名单 / 保护路径 / 用户取消），而不是删失败。
    pub fn was_skipped(&self, path: &Path) -> bool {
        self.skipped_items
            .iter()
            .any(|item| item.as_path() == Some(path))
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

    // 同一个 SQLite 家族（主库 + `-wal`/`-shm`/`-journal`）按 basename 分组，
    // 组间照常并行、组内强制串行且先删伴随文件再删主库。见
    // `group_sqlite_families` 和 `delete_sqlite_family` 的文档：主库和伴随
    // 文件谁先消失如果是并行赛跑决定的，就有几率留下「主库已经没了、
    // `-wal` 还在」这种脏状态——这正是 `safety::is_live_database` 文档里
    // Autodesk Fusion `Cache.db` 那次事故的诱因之一，即便这一组当下并没
    // 有被判定为「活库」也一样。
    let files_failed: usize = group_sqlite_families(files)
        .into_par_iter()
        .map(|group| delete_sqlite_family(group, p))
        .sum();

    let subs_failed = subdirs
        .par_iter()
        .filter(|d| delete_tree(d, p) == CleanResult::Failed)
        .count();

    if p.cancelled() {
        return CleanResult::Skipped;
    }
    let dir_removed = match std::fs::remove_dir(path) {
        Ok(()) => true,
        Err(err) => {
            note_delete_failure(path, &err);
            false
        }
    };
    if dir_removed && files_failed == 0 && subs_failed == 0 {
        CleanResult::Ok
    } else {
        // 文件删不掉会记进 `p.failed`（见 delete_file），目录删不掉以前
        // 只体现在返回值上，进度条里的失败数因此偏少。
        p.failed.fetch_add(1, Ordering::Relaxed);
        CleanResult::Failed
    }
}

/// 「连目录一起删」的目标在真正删除前的身份复验：扫描期快照与当前
/// `stat` 结果一致才放行。
///
/// **接线说明（给接手这段代码的人）**：这个函数是独立的纯判断，故意
/// 不放进 [`delete_tree`]——`delete_tree` 会递归调用自身处理子目录，
/// 而身份快照只对应用户勾选的那一个根，不对应递归下去遇到的每一层；
/// 塞进 `delete_tree` 内部会导致子目录也被拿根的身份去比对，结果是
/// 整棵树只要有一层不匹配（根本不会匹配，子目录的 dev/ino 跟根不一样）
/// 就全部拒删。正确的调用位置是 `clean_targets` 里 `t.remove_dir` 分支、
/// 调 `clean_path(d, p)` **之前**：
///
/// ```ignore
/// if t.remove_dir {
///     if !root_identity_holds(d, t.identity) {
///         note_delete_failure(d, &"identity-changed");
///         report.failed.push(CleanFailure::Path(d.clone()));
///         p.failed.fetch_add(1, Ordering::Relaxed);
///         continue;
///     }
///     report.record(d, clean_path(d, p));
/// } else {
///     report.merge(clean_dir_contents(d, p));
/// }
/// ```
///
/// 之所以是「调用前挡一道」而不是改 `clean_path` 本身：`clean_path` 还
/// 兼管 APFS 本地快照（`tmutil deletelocalsnapshots`）这类虚拟路径，
/// 虚拟路径在扫描阶段就没有身份（恒为 `None`），改在 `clean_path` 内部
/// 判断需要多一层虚拟路径分支，不如让调用方在合适的地方判一次干净。
///
/// `identity` 为 `None` 时拒绝。真实文件系统目标如果连扫描期身份都没能
/// 取得，就没有依据证明当前路径还是用户看到的那个对象；虚拟目标由调用方
/// 在进入这里之前分流，不依赖文件系统身份。
pub fn root_identity_holds(path: &Path, identity: Option<TargetIdentity>) -> bool {
    identity.is_some_and(|want| want.recheck(path))
}

/// 把一批文件按 SQLite 家族（`safety::sqlite_family_key`）分组：同一个主库
/// 和它的 `-wal`/`-shm`/`-journal` 伴随文件归到一组，不属于任何家族的普通
/// 文件各自单独成组（组内 1 个成员，等价于原来的独立并行删除，行为不变）。
///
/// 返回 `Vec<Vec<_>>` 而不是保留 `HashMap`：调用方要把每一组交给 rayon
/// 并行跑，`HashMap` 的 `Values` 不是 `IndexedParallelIterator`，先收集成
/// `Vec` 更直接。
fn group_sqlite_families(files: Vec<(PathBuf, u64)>) -> Vec<Vec<(PathBuf, u64)>> {
    use std::collections::HashMap;
    let mut groups: HashMap<String, Vec<(PathBuf, u64)>> = HashMap::new();
    for (path, size) in files {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        // 落单文件（不属于任何 SQLite 家族）拿自己的完整路径当 key，保证
        // 各自独立成组：只用小写文件名会在大小写敏感的文件系统上，把
        // "Notes.TXT" 和 "notes.txt" 这两个本不相干的文件错误地并成一组。
        let key = crate::core::safety::sqlite_family_key(name)
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        groups.entry(key).or_default().push((path, size));
    }
    groups.into_values().collect()
}

/// 删除一个 SQLite 家族分组：先删伴随文件（`-wal`/`-shm`/`-journal`），
/// 全部成功之后才删主库；只要有一个成员删不掉就整组放弃——已经排在后面
/// 还没轮到的成员（包括主库本身）原地不动，不会被继续尝试。
///
/// 这样保证组内**要么整组删、要么整组不删**：绝不会出现「主库已经被删、
/// 某个伴随文件还留着」的中间态。反过来——伴随文件删失败、主库留着——是
/// 安全的中间态：SQLite 下次打开时能拿主库配着剩下的伴随文件正常工作或
/// 走恢复流程；主库都不在了，伴随文件却还在，则完全是未定义行为。
///
/// 组内只有一个成员（普通文件，或落单的主库/伴随文件）时，这个函数退化成
/// 单文件删除，和分组之前的行为完全一致。
///
/// 返回值：本组里最终没能删除的文件数（真正失败的 + 因为前面失败而放弃
/// 尝试的），供上层判断「这一层目录是否已经清空」。取消场景下
/// `delete_file` 会直接返回 `Skipped` 而不计入这里——和分组之前的
/// `files_failed` 语义保持一致，取消不算失败。
fn delete_sqlite_family(members: Vec<(PathBuf, u64)>, p: &CleanProgress) -> usize {
    // 伴随文件之间互不依赖，各自独立尝试删除——某一个删不掉（比如被占用）
    // 不该连累另一个明明删得掉的伴随文件也不去删，那样只是白白少清理。
    // 真正要守住的不变量只有一条：**只要有任何一个伴随文件没删成功，主库
    // 就绝不能删**，所以主库放在最后单独判断，而不是跟着一起 par_iter。
    let (companions, main): (Vec<_>, Vec<_>) = members.into_iter().partition(|(path, _)| {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        crate::core::safety::is_sqlite_companion_name(name)
    });

    // 主库和伴随文件同时存在就是当前活库判据。macOS/Unix 允许 unlink
    // 正被打开的文件，因此不能指望 remove_file 失败替我们挡住：递归到
    // 嵌套目录时 clean_path 的顶层目录检查看不到这一组，必须在真正删除
    // 每个家族的位置再次 fail closed。
    if !companions.is_empty() && !main.is_empty() {
        for (path, _) in companions.iter().chain(main.iter()) {
            note_delete_failure(path, &LIVE_DATABASE_REFUSAL);
        }
        let blocked = companions.len() + main.len();
        p.failed.fetch_add(blocked as u64, Ordering::Relaxed);
        return blocked;
    }

    let mut undeleted = 0usize;
    let mut any_companion_failed = false;
    for (path, size) in companions {
        if delete_file(&path, size, p) == CleanResult::Failed {
            any_companion_failed = true;
            undeleted += 1;
        }
    }

    // 一个家族最多一个主库成员（`sqlite_family_key` 保证），但写成循环
    // 而不是假设恰好一个，任何意外都退化成「按元素处理」而不是 panic。
    for (path, size) in main {
        if any_companion_failed {
            // 伴随文件没删干净：主库必须原地保留，宁可这组只清了一半，
            // 也不能让主库先消失、伴随文件却还在——那正是
            // `safety::is_live_database` 文档里 Fusion `Cache.db` 事故的
            // 脏状态形状。
            undeleted += 1;
        } else if delete_file(&path, size, p) == CleanResult::Failed {
            undeleted += 1;
        }
    }
    undeleted
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
    match std::fs::remove_file(path) {
        Ok(()) => true,
        Err(err) => {
            note_delete_failure(path, &err);
            false
        }
    }
}

/// 删除失败的原因以前被整个丢掉：报告里只剩一个失败计数，用户和日志都
/// 看不出到底是被占用、没权限还是别的什么。这里补上原因。
///
/// 和 `audit_result` 的失败清单一样封顶 20 条：一次清理失败几万个是常态
/// （多半整个目录被占用），全记会先把日志撑爆，前 20 条足够看出是哪一类。
/// 计数在每批清理开始时（`audit`）归零。
fn note_delete_failure(path: &Path, err: &dyn std::fmt::Display) {
    const MAX_LOGGED: usize = 20;
    let n = DELETE_FAILURES_LOGGED.fetch_add(1, Ordering::Relaxed);
    if n < MAX_LOGGED {
        crate::log!("[删除] 失败：{}（{err}）", path.display());
    } else if n == MAX_LOGGED {
        crate::log!("[删除] 失败原因已记满 {MAX_LOGGED} 条，本批后续不再记录");
    }
}

static DELETE_FAILURES_LOGGED: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// 活数据库闸门（[`crate::core::safety::is_live_database`]）拒绝删除时的
/// 统一说明。必须写出「为什么拒」和「怎么办」：拒的是「主库与事务伴随
/// 文件同时存在」这个活连接证据；出路是先彻底退出对应应用。实测教训
/// （iStat Menus 残留）：只说「拒绝删除」，用户会把它当权限问题盲目
/// 重试五轮——真正卡住的是 launchd KeepAlive 代理把进程秒拉起。
pub(crate) const LIVE_DATABASE_REFUSAL: &str = "命中活数据库保护：目标里有正在使用的数据库文件（主库与事务伴随文件同时存在），对应应用或其后台代理可能仍在运行——先彻底退出该应用（含菜单栏常驻与后台代理）再清理";

/// 清理单个路径本身（连同其内容）。
///
/// 删除前依次过闸门，任何一道拦住都不会碰文件系统：
/// 1. [`is_protected`]——系统骨架、用户主目录、用户白名单。命中是策略拒绝，
///    记 [`CleanResult::Skipped`]，不进失败清单。
/// 2. [`safety::is_live_database`]——活数据库家族保护。事故背景见该函数
///    文档：参考项目 Mole 记录的 issue #1390，Autodesk Fusion 的
///    `AcCoreConsole` 在它的 `Cache.db` 被删除后进入无界写循环、写满整个
///    卷。以前这个判据（`categories::helpers::holds_live_database`）只
///    参与「要不要默认勾选」的展示层决策，用户手动勾上就直奔这里，没有
///    任何拦截——现在提到删除入口本身，不依赖调用方记得先查一遍。命中
///    记 [`CleanResult::Failed`]：这是安全机制拦住的删除，不是用户排除。
pub fn clean_path(path: &Path, p: &CleanProgress) -> CleanResult {
    // 虚拟路径：APFS 本地快照，用 tmutil deletelocalsnapshots 删除
    if let Some(snapshot) = snapshot_name(path) {
        p.note(path);
        let status = crate::core::proc::run_with_timeout(
            "tmutil",
            &["deletelocalsnapshots", snapshot.as_str()],
            Duration::from_secs(60),
        );
        return match status {
            Some(run) if run.ok => CleanResult::Ok,
            _ => CleanResult::Failed,
        };
    }

    if std::fs::symlink_metadata(path).is_err() {
        return CleanResult::Skipped;
    }
    if is_protected(path) {
        return CleanResult::Skipped;
    }
    if crate::core::safety::is_live_database(path) {
        note_delete_failure(path, &LIVE_DATABASE_REFUSAL);
        return CleanResult::Failed;
    }
    delete_tree(path, p)
}

/// 清理目录**内容**但保留目录本身。
///
/// TOCTOU 防护的关键覆盖点：`CleanTarget::identity` 只对 `remove_dir:
/// true` 有效，对这条通道（`remove_dir: false`）完全帮不上忙——这类
/// 目标的根是 `~/Library/Caches` 之类长期存在的目录，扫描前后 `dev`/
/// `ino` 常年不变，子项被整体换掉根的身份纹丝不动，验根等于没验。而
/// 恰恰是这条通道覆盖了体积占比最大、用户几乎每次都勾选的类别（系统
/// 缓存、用户缓存、日志）。
///
/// 真正的防线是「叶子—父目录绑定」：`read_dir` 拿到每个子项时顺带存一份
/// 它自己和父目录 `dir` 此刻的身份（`entry.metadata()` 本来就要调，不算
/// 额外系统调用），删除前对每一跳重新 `stat` 一次做最后复核——只要父
/// 目录被整体换掉（比如换成指向别处的符号链接），或者这一个具体子项在
/// `read_dir` 到真正删除之间被换了内容，就在这一跳被挡下；没被换的
/// 兄弟节点不受影响，照常清理。
pub fn clean_dir_contents(dir: &Path, p: &CleanProgress) -> CleanReport {
    let mut report = CleanReport::default();

    // read_dir 会跟随作为根目标的目录符号链接。固定扫描完成后路径仍可能
    // 被其它进程替换，因此删除层必须再次用 symlink_metadata 阻断，不能
    // 依赖扫描阶段的检查。
    let Ok(md) = std::fs::symlink_metadata(dir) else {
        report.record(dir, CleanResult::Skipped);
        return report;
    };
    if md.file_type().is_symlink() || !md.is_dir() {
        report.failed.push(CleanFailure::Path(dir.to_path_buf()));
        return report;
    }
    if is_protected(dir) {
        report.record(dir, CleanResult::Skipped);
        return report;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => {
            if dir.exists() {
                report.failed.push(CleanFailure::Path(dir.to_path_buf()));
            } else {
                report.record(dir, CleanResult::Skipped);
            }
            return report;
        }
    };

    // 父目录身份在 read_dir **之后**取：TOCTOU 防护的窗口是「看到子项」到
    // 「删子项」之间，read_dir 本身不是威胁。Windows 上 read_dir 可能改变
    // 目录的元数据（如分配大小），在 read_dir 之前取的快照会因此误判。
    let parent_identity = std::fs::symlink_metadata(dir)
        .ok()
        .and_then(|md| TargetIdentity::from_metadata(&md));

    // 连同每个子项自己的身份一起收集。用 `symlink_metadata` 而不是
    // `DirEntry::metadata()`：后者在 Windows 上取的是 `FindNextFile` 的数据，
    // 和 `recheck` 用的 `GetFileAttributesEx`（`symlink_metadata`）对目录
    // 可能给出不同的 `len`，导致身份永远对不上。多一次系统调用，但保证
    // 捕获和复核用同一个 API。
    let children: Vec<(PathBuf, Option<TargetIdentity>)> = entries
        .flatten()
        .map(|e| {
            let path = e.path();
            let identity = std::fs::symlink_metadata(&path)
                .ok()
                .and_then(|md| TargetIdentity::from_metadata(&md));
            (path, identity)
        })
        .collect();

    // 先串行做完所有叶子绑定复核，再并行删除：delete_tree 删完子目录内容
    // 后会 remove_dir 子目录本身，改变父目录的 mtime。如果复核和删除交错在
    // 并行线程里，一个线程删掉子目录 → 父目录 mtime 变化 → 另一个线程的
    // 父目录 recheck 误判为「身份变了」而拒绝删除整批子项。
    let checked: Vec<(&PathBuf, &Option<TargetIdentity>, bool)> = children
        .iter()
        .map(|(c, child_identity)| {
            let ok = !p.cancelled()
                && leaf_binding_holds(dir, parent_identity, c, *child_identity);
            (c, child_identity, ok)
        })
        .collect();

    let merged = checked
        .par_iter()
        .map(|(c, _child_identity, binding_ok)| {
            let mut r = CleanReport::default();
            if !binding_ok {
                note_delete_failure(c, &"identity-changed");
                r.failed.push(CleanFailure::Path((*c).clone()));
                p.failed.fetch_add(1, Ordering::Relaxed);
                return r;
            }
            if p.cancelled() {
                r.record(c, CleanResult::Skipped);
                return r;
            }
            r.record(c, clean_path(c, p));
            r
        })
        .reduce(CleanReport::default, |mut a, b| {
            a.merge(b);
            a
        });
    report.merge(merged);
    report
}

/// 「叶子—父目录绑定」复核：删除某个子项之前，最后一跳确认父目录和这个
/// 子项本身都还是 `read_dir` 那一刻看到的那个东西。
///
/// 两个身份分别检查、任意一个「有快照但对不上」就拒绝：
/// - 父目录对不上，说明 `dir` 这个路径在 `read_dir` 之后被整体换掉了
///   （比如换成了指向别处的符号链接），这一整批子项全都不可信；
/// - 子项对不上，说明只有这一个文件在 `read_dir` 到现在这段时间里被
///   换了内容，其它兄弟节点不受影响。
///
/// 任意身份缺失都拒绝：探测失败不是删除授权。
fn leaf_binding_holds(
    parent: &Path,
    parent_identity: Option<TargetIdentity>,
    child: &Path,
    child_identity: Option<TargetIdentity>,
) -> bool {
    parent_identity.is_some_and(|want| want.recheck(parent))
        && child_identity.is_some_and(|want| want.recheck(child))
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
    DELETE_FAILURES_LOGGED.store(0, Ordering::Relaxed);
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
    /// 虚拟路径目标（Docker 镜像）的真实体积。真实路径的体积在删除时
    /// 逐文件累计，用不到这个字段。
    pub size_hint: Option<u64>,
    /// 扫描期拍下的目标身份快照（TOCTOU 防护，见
    /// `core::model::TargetIdentity`），从 `ScanItem::identity` 原样搬运
    /// 而来（搬运点：`ui::state::JunkState::selected_targets`），不会在
    /// 这里或更晚的任何地方重新拍。
    ///
    /// 这个字段只对 `remove_dir: true` 的目标有意义：删除前拿它跟当前
    /// stat 复验一次（见 `root_identity_holds`），根身份变了就说明整个
    /// 目标在扫描之后被换掉了。
    ///
    /// 对 `remove_dir: false` 的目标（系统缓存、用户缓存这类只清内容、
    /// 保留目录本身的类别——恰好是体积占比最大、用户几乎每次都勾选的
    /// 通道）验根**没有意义**：它们的根是 `~/Library/Caches` 这种长期
    /// 存在的目录，扫描前后 `dev`/`ino` 常年不变，子项被整体换掉根的
    /// 身份纹丝不动，验根等于没验。这类目标真正的防线在
    /// `clean_dir_contents` 内部对每个叶子做的「叶子—父目录绑定」复核，
    /// 这个字段对它们来说用不上（但仍然会被填充，只是没有消费方）。
    pub identity: Option<crate::core::model::TargetIdentity>,
    /// 永久删除还是送废纸篓/回收站，由类目决定（见
    /// `categories::CategoryId::disposal`）。只对 `remove_dir: true` 的
    /// 目标有意义——`remove_dir: false` 是「清空内容、保留目录」，把内容
    /// 逐个挪进废纸篓既不释放空间也不成其为一次清理。
    pub disposal: Disposal,
}

impl CleanTarget {
    /// 只清空内容，保留目录本身。
    pub fn empty(path: PathBuf) -> Self {
        Self {
            path,
            remove_dir: false,
            size_hint: None,
            identity: None,
            disposal: Disposal::Permanent,
        }
    }

    /// 连目录一起删。
    pub fn remove(path: PathBuf) -> Self {
        Self {
            path,
            remove_dir: true,
            size_hint: None,
            identity: None,
            disposal: Disposal::Permanent,
        }
    }
}

/// 清理多个扫描目标。
pub fn clean_targets(targets: &[CleanTarget], p: &CleanProgress) -> CleanReport {
    audit("分类清理", targets.iter().map(|t| t.path.clone()));

    // 扫描阶段的占用检测是十几秒到几分钟前的快照（详见 core::inuse 顶部
    // 说明），从检测完成到用户点下「清理」，中间应用完全可能刚刚启动。
    // macOS 允许删除正被打开的文件——不拦的话就是"删成功了，应用却在写
    // 一个已消失的路径"。这里只对已勾选的这一批目标做定点复检（不是全表
    // 扫描），毫秒级，用户感知不到额外等待；新发现的占用、或者复检本身
    // 测不出，都直接拒删，不是降级成"不推荐"。
    let spot_paths: Vec<PathBuf> = targets.iter().map(|t| t.path.clone()).collect();
    let spot = crate::core::inuse::spot_check(&spot_paths);

    let mut report = CleanReport::default();
    let mut bin_done = false;
    for t in targets {
        if p.cancelled() {
            break;
        }
        let d = &t.path;
        p.note(d);

        match spot.get(d) {
            Some(crate::core::inuse::SpotCheck::Busy) => {
                note_delete_failure(d, &"became-busy");
                report.record(d, CleanResult::Failed);
                continue;
            }
            Some(crate::core::inuse::SpotCheck::Unknown) => {
                note_delete_failure(d, &"spot-check-unknown");
                report.record(d, CleanResult::Failed);
                continue;
            }
            _ => {}
        }

        // 系统回收站/废纸篓：整目录一次清空，不逐条删。平台差异（Windows 的
        // `$Recycle.Bin` 与 macOS 的 `~/.Trash`）由门面契约的 `is_system_trash`
        // / `empty_trash` 吃掉，这里不再有 `#[cfg]` 分支。
        if crate::platform::is_system_trash(d) {
            if !bin_done {
                report.merge(crate::platform::empty_trash(p));
                bin_done = true;
            }
            continue;
        }

        #[cfg(target_os = "macos")]
        if is_launch_agent_plist(d) {
            if !root_identity_holds(d, t.identity) {
                note_delete_failure(d, &"identity-changed");
                report.record(d, CleanResult::Failed);
                continue;
            }
            let result = match crate::platform::move_to_trash(d) {
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

        // Docker 镜像：虚拟路径余文就是 rmi 引用参数，路由到
        // `docker image rm`（机制与不用 --force 的理由见 `core::docker`）。
        if let Some(rmi_ref) = crate::core::model::docker_rmi_ref(d) {
            report.record_target(
                CleanFailure::Path(d.clone()),
                clean_docker_image(&rmi_ref, t.size_hint, p),
            );
            continue;
        }

        // brew 清理：与 Docker 同构的虚拟目标，路由到 `brew cleanup`
        // （owner command——命令自己知道怎么安全收缩，见 `core::brew`）。
        // 体积记账用扫描期的 size_hint：真实清掉的量由 brew 决定，与
        // dry-run 的估算不完全一致，但这只影响「已释放 X」的显示精度。
        if crate::core::brew::is_brew_virtual(d) {
            let result = if crate::core::brew::run_cleanup() {
                p.files.fetch_add(1, Ordering::Relaxed);
                p.bytes
                    .fetch_add(t.size_hint.unwrap_or(0), Ordering::Relaxed);
                CleanResult::Ok
            } else {
                p.failed.fetch_add(1, Ordering::Relaxed);
                CleanResult::Failed
            };
            report.record(d, result);
            continue;
        }

        // Go module cache / pnpm store：owner command 路由（`core::owner`）。
        // 这两个缓存的内部状态（Go 的只读位与索引、pnpm 的 store 元数据）
        // 让裸删有留不一致的风险，优先让生态自己的命令收缩；探测不满足
        // （工具链缺席、路径与命令作用域不一致）回退现有裸删。命令失败
        // **不**回退：跑到一半的 store 状态未知，不能再动刀。
        // 体积按扫描期的 size_hint 估算记账（命令收缩量与称重口径不完全
        // 一致，只影响显示精度）。
        let go_modcache = crate::core::owner::is_go_modcache(d);
        let pnpm_store = crate::core::owner::is_pnpm_store(d);
        if (go_modcache || pnpm_store) && !root_identity_holds(d, t.identity) {
            note_delete_failure(d, &"identity-changed");
            report.record(d, CleanResult::Failed);
            continue;
        }
        let owner_result: Option<Option<bool>> = if go_modcache {
            crate::core::owner::go_clean_modcache(d).map(Some)
        } else if pnpm_store {
            crate::core::owner::pnpm_store_prune(d).map(Some)
        } else {
            None
        };
        match owner_result {
            // 命令明确失败：报 Failed，不回退裸删
            Some(Some(false)) => {
                note_delete_failure(d, &"owner-command-failed");
                p.failed.fetch_add(1, Ordering::Relaxed);
                report.record(d, CleanResult::Failed);
                continue;
            }
            // 命令成功：按估算体积记账，跳过裸删
            Some(Some(true)) => {
                p.files.fetch_add(1, Ordering::Relaxed);
                p.bytes
                    .fetch_add(t.size_hint.unwrap_or(0), Ordering::Relaxed);
                report.record(d, CleanResult::Ok);
                continue;
            }
            // 探测不满足（None 外层）：回退现有裸删路径
            _ => {}
        }

        if t.remove_dir {
            // 扫描期身份快照与当前 stat 不一致 → 目标已被换过，拒删。
            // 挡在 clean_path 之前而不是塞进它内部：clean_path 还兼管
            // APFS 快照这类虚拟路径（身份恒为 None），调用方在这里明确
            // 分流；真实目标没有快照一律拒绝。
            if !crate::core::model::is_virtual_path(d) && !root_identity_holds(d, t.identity) {
                note_delete_failure(d, &"identity-changed");
                report.record(d, CleanResult::Failed);
                continue;
            }
            report.record(d, dispose(d, t.disposal, p));
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

/// 删除一个 Docker 镜像引用。
///
/// 字节记账：`docker image rm` 对多标签镜像可能只摘标签（`Untagged:`）
/// 而不删层（`Deleted:`），只有层真删了磁盘空间才释放，此时才往
/// bytes 上加——「已释放 X」必须是真的释放了才算。
fn clean_docker_image(rmi_ref: &str, size_hint: Option<u64>, p: &CleanProgress) -> CleanResult {
    if p.cancelled() {
        return CleanResult::Skipped;
    }
    match crate::core::docker::remove_image(rmi_ref) {
        Ok(layers_deleted) => {
            p.files.fetch_add(1, Ordering::Relaxed);
            if layers_deleted {
                if let Some(size) = size_hint {
                    p.bytes.fetch_add(size, Ordering::Relaxed);
                }
            }
            CleanResult::Ok
        }
        Err(err) => {
            note_delete_failure(Path::new(rmi_ref), &err);
            p.failed.fetch_add(1, Ordering::Relaxed);
            CleanResult::Failed
        }
    }
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

/// 手选路径（磁盘透镜 / 整理页）的一条目标：路径 + 确认时拍下的身份。
///
/// 磁盘树本身不存身份（紧凑 mmap 索引没有这个字段），所以快照打在用户
/// 点确认那一刻，删除前再 `recheck`。盖不住「树扫完到点确认」那一段，
/// 但盖得住确认之后、后台真正动手之前的窗口——手选路径没有分类清理
/// 那条 `ScanItem::identity` 搬运链，不在这里补就完全裸奔。
#[derive(Clone, Debug)]
pub struct ArbitraryTarget {
    pub path: PathBuf,
    pub identity: Option<TargetIdentity>,
}

impl ArbitraryTarget {
    /// 当场拍一份身份。调用方应在用户确认删除时调用，不要拖到后台线程
    /// 已经开始删了再拍。
    pub fn capture(path: PathBuf) -> Self {
        let identity = crate::core::model::capture_identity(&path);
        Self { path, identity }
    }
}

/// 对用户在磁盘分析里手动选中的任意路径执行清理。
///
/// 没有身份快照时（测试、内部调用）在入口当场拍一份，再走
/// [`clean_arbitrary_items`]。生产路径请用 [`ArbitraryTarget::capture`]
/// 在确认时拍好再传入。
///
/// `disposal` 只影响手选路径。分类清理走的是固定白名单表（缓存、临时文件、
/// 构建产物），把它们塞进回收站没有意义，只会让用户再清一次。
pub fn clean_arbitrary(paths: &[PathBuf], disposal: Disposal, p: &CleanProgress) -> CleanReport {
    let items: Vec<ArbitraryTarget> = paths.iter().cloned().map(ArbitraryTarget::capture).collect();
    clean_arbitrary_items(&items, disposal, p)
}

/// 带确认期身份快照的手选路径清理。
pub fn clean_arbitrary_items(
    items: &[ArbitraryTarget],
    disposal: Disposal,
    p: &CleanProgress,
) -> CleanReport {
    audit(
        match disposal {
            Disposal::Permanent => "用户手选路径（永久删除）",
            Disposal::RecycleBin => "用户手选路径（送回收站）",
        },
        items.iter().map(|t| t.path.clone()),
    );

    let spot_paths: Vec<PathBuf> = items.iter().map(|t| t.path.clone()).collect();
    let spot = crate::core::inuse::spot_check(&spot_paths);

    let mut report = CleanReport::default();
    for item in items {
        if p.cancelled() {
            break;
        }
        let path = &item.path;
        p.note(path);

        match spot.get(path) {
            Some(crate::core::inuse::SpotCheck::Busy) => {
                note_delete_failure(path, &"became-busy");
                report.record(path, CleanResult::Failed);
                continue;
            }
            Some(crate::core::inuse::SpotCheck::Unknown) => {
                note_delete_failure(path, &"spot-check-unknown");
                report.record(path, CleanResult::Failed);
                continue;
            }
            _ => {}
        }

        if is_protected(path) {
            report.record(path, CleanResult::Skipped);
            continue;
        }
        // 确认时没拍到身份（网络盘、mtime 读不到）不能把整条手选路径
        // 卡死：没有快照就跳过复验。有快照但对不上，才是 TOCTOU。
        if let Some(identity) = item.identity {
            if !identity.recheck(path) {
                note_delete_failure(path, &"identity-changed");
                report.record(path, CleanResult::Failed);
                continue;
            }
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
pub(crate) fn dispose(path: &Path, disposal: Disposal, p: &CleanProgress) -> CleanResult {
    match disposal {
        Disposal::Permanent => clean_path(path, p),
        Disposal::RecycleBin => recycle_path(path, p),
    }
}

/// 把路径送进回收站/废纸篓。
///
/// 以前这里按平台分成两份：Windows 走 `SHFileOperationW`，非 Windows 直接
/// `clean_path` 永久删除，理由写的是「非 Windows 平台没有等价的回收站 API」。
/// 那句话在本仓库里是假的——`platform::macos::trash::move_to_trash` 一直都在
/// （走 `NSFileManager.trashItemAtURL:`）。结果是 macOS 上用户勾了「删除到
/// 回收站」，`Disposal::RecycleBin` 的文档、审计日志和界面文案三处都承诺
/// 「可以还原」，实际却是不可撤销的永久删除。现在统一走门面契约的
/// `move_to_trash`，平台差异由 `platform` 层负责。
///
/// 失败时**不**回退到永久删除（见 [`dispose`]），如实报失败。
fn recycle_path(path: &Path, p: &CleanProgress) -> CleanResult {
    if p.cancelled() {
        return CleanResult::Skipped;
    }
    if std::fs::symlink_metadata(path).is_err() {
        return CleanResult::Skipped;
    }
    if is_protected(path) {
        return CleanResult::Skipped;
    }
    if crate::core::safety::is_live_database(path) {
        note_delete_failure(path, &LIVE_DATABASE_REFUSAL);
        return CleanResult::Failed;
    }

    match crate::platform::move_to_trash(path) {
        Ok(()) => {
            // 回收站不释放空间，所以这里只记条目数，不往 bytes 上加——
            // 界面上「已释放 X」必须是真的释放了才算。
            p.files.fetch_add(1, Ordering::Relaxed);
            CleanResult::Ok
        }
        Err(err) => {
            note_delete_failure(path, &err);
            p.failed.fetch_add(1, Ordering::Relaxed);
            CleanResult::Failed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用户白名单是删除层的硬拦截：哪怕用户把白名单目标手动勾选
    /// （勾上 = 绕过所有 recommended 降级、强过确认弹窗），`clean_path`
    /// 也必须拒绝。「永久排除」的语义就是任何通道都碰不到它——这里验证
    /// 的是勾选手动目标这条最直接的通道；`delete_tree` 递归层与
    /// `clean_dir_contents` 子项通道共用同一个 `is_protected` 检查点，
    /// 由它统一覆盖。
    #[test]
    fn clean_path_rejects_whitelisted_target() {
        let _guard = crate::core::whitelist::TEST_LOCK.lock().unwrap();
        crate::core::whitelist::clear();
        let base = std::env::temp_dir().join("qc_whitelist_hard_reject");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let victim = base.join("precious.sqlite");
        std::fs::write(&victim, b"do not touch").unwrap();

        crate::core::whitelist::reload(std::slice::from_ref(
            &victim.to_string_lossy().into_owned(),
        ));

        let p = CleanProgress::default();
        assert_eq!(clean_path(&victim, &p), CleanResult::Skipped);
        assert!(victim.exists(), "白名单目标必须原地保留");

        // 子树同样拦得住：白名单的是目录，里面的文件递归进来也要被挡
        std::fs::remove_file(&victim).unwrap();
        std::fs::create_dir_all(&victim).unwrap();
        let inner = victim.join("child.db");
        std::fs::write(&inner, b"inside protected tree").unwrap();
        let p2 = CleanProgress::default();
        assert_eq!(delete_tree(&victim, &p2), CleanResult::Skipped);
        assert!(inner.exists(), "白名单目录的子树必须原地保留");

        crate::core::whitelist::clear();
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 手选路径必须复验确认时拍下的身份：替换之后不能按原路径删下去。
    #[test]
    fn clean_arbitrary_rejects_swapped_target() {
        let base = std::env::temp_dir().join("qc_arbitrary_identity_swap");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let path = base.join("leaf.bin");
        std::fs::write(&path, b"original").unwrap();
        let identity = crate::core::model::capture_identity(&path);
        assert!(identity.is_some());

        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"replaced payload, different length").unwrap();

        let item = ArbitraryTarget {
            path: path.clone(),
            identity,
        };
        let p = CleanProgress::default();
        let report = clean_arbitrary_items(std::slice::from_ref(&item), Disposal::Permanent, &p);

        assert_eq!(report.failed.len(), 1);
        assert!(path.exists(), "身份对不上时必须原地保留");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 受保护路径是策略拒绝，记 skipped 而不是 failed。
    #[test]
    fn clean_arbitrary_skips_protected_path() {
        let _guard = crate::core::whitelist::TEST_LOCK.lock().unwrap();
        crate::core::whitelist::clear();
        let base = std::env::temp_dir().join("qc_arbitrary_protected_skip");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let path = base.join("keep.bin");
        std::fs::write(&path, b"x").unwrap();
        crate::core::whitelist::reload(std::slice::from_ref(
            &path.to_string_lossy().into_owned(),
        ));

        let p = CleanProgress::default();
        let report = clean_arbitrary(std::slice::from_ref(&path), Disposal::Permanent, &p);
        assert_eq!(report.skipped, 1);
        assert!(report.was_skipped(&path));
        assert!(report.failed.is_empty());
        assert!(path.exists());

        crate::core::whitelist::clear();
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 确认时拍不到身份（网络盘、mtime 缺失）不能把磁盘透镜整条卡死。
    #[test]
    fn clean_arbitrary_without_identity_still_deletes() {
        let base = std::env::temp_dir().join("qc_arbitrary_no_identity");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let path = base.join("ok.bin");
        std::fs::write(&path, b"x").unwrap();

        let item = ArbitraryTarget {
            path: path.clone(),
            identity: None,
        };
        let p = CleanProgress::default();
        let report = clean_arbitrary_items(std::slice::from_ref(&item), Disposal::Permanent, &p);
        assert_eq!(report.ok, 1);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 「送回收站」的核心不变量：文件要么真的进了废纸篓，要么原地还在，
    /// **绝不能静默消失**。
    ///
    /// macOS 上这条以前是不成立的——`recycle_path` 的非 Windows 分支直接
    /// 转发给 `clean_path` 永久删除，而界面、枚举文档和审计日志三处都写着
    /// 「可以还原」。这个断言故意写成「二选一」而不是「必须在废纸篓里」：
    /// 沙盒、无 Finder、只读卷等环境下移动会失败，那时正确行为是保留原文件
    /// 并报失败，同样满足不变量。
    #[test]
    fn recycle_never_silently_destroys() {
        let base = std::env::temp_dir().join("qc_recycle_invariant");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let file = base.join("待回收.txt");
        std::fs::write(&file, b"payload").unwrap();

        let p = CleanProgress::new(1, 7);
        let report = clean_arbitrary(std::slice::from_ref(&file), Disposal::RecycleBin, &p);

        let snap = p.snapshot();
        if report.ok == 1 {
            assert!(!file.exists(), "报告成功就不该还留在原处");
            // 回收站不释放空间：只记条目数，bytes 必须是 0
            assert_eq!(snap.files, 1);
            assert_eq!(snap.bytes, 0, "移入废纸篓不等于释放空间");

            // 从废纸篓里把它清掉，别给用户留垃圾
            if let Some(home) = dirs::home_dir() {
                let _ = std::fs::remove_file(home.join(".Trash").join("待回收.txt"));
            }
        } else {
            assert!(
                file.exists(),
                "移入废纸篓失败时必须保留原文件，绝不能退化成永久删除"
            );
        }

        let _ = std::fs::remove_dir_all(&base);
    }

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

    // ---- TargetIdentity / TOCTOU 防护 ----

    /// 核心场景：扫描期拍完快照之后，目标被整个删掉重建成同名的新文件
    /// （典型的 rename/替换手法）。`root_identity_holds` 必须识破——
    /// 复核失败之后，如果调用方遵循「失败就不进 `delete_tree`」的约定，
    /// 新文件应当原封不动地留在原地。
    #[test]
    fn root_identity_guard_blocks_renamed_swap_target() {
        let base = std::env::temp_dir().join("qc_identity_swap_root");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let target = base.join("victim.txt");
        std::fs::write(&target, b"original build artifact").unwrap();

        let identity = crate::core::model::capture_identity(&target);
        assert!(identity.is_some(), "应该能拍到身份");

        // 攻击：扫描之后，目标被整个替换成新内容
        std::fs::remove_file(&target).unwrap();
        std::fs::write(&target, b"attacker payload").unwrap();

        assert!(
            !root_identity_holds(&target, identity),
            "身份对不上时必须拒绝"
        );
        // 复核在 delete_tree 之前拦截，新文件应当完好无损
        assert_eq!(std::fs::read(&target).unwrap(), b"attacker payload");

        let _ = std::fs::remove_dir_all(&base);
    }

    /// 正向对照：没被替换的目标复核应当通过，并且真的能被 `delete_tree`
    /// 删掉——不能因为加了这层防护，让原本能清的东西反而清不掉。
    #[test]
    fn root_identity_guard_allows_unchanged_target_through_to_delete() {
        let base = make_tree("identity_unchanged", 6, 64);
        let identity = crate::core::model::capture_identity(&base);
        assert!(identity.is_some());
        assert!(root_identity_holds(&base, identity));

        let p = CleanProgress::default();
        assert_eq!(delete_tree(&base, &p), CleanResult::Ok);
        assert!(!base.exists());
    }

    /// 身份为 `None` 表示扫描时没能确认对象是谁，必须 fail closed。
    #[test]
    fn root_identity_guard_rejects_when_snapshot_missing() {
        assert!(!root_identity_holds(
            Path::new("/definitely/not/a/real/path/qc"),
            None
        ));
    }

    /// 额外收益：祖先目录被换成指向别处的符号链接，路径字符串一个字都
    /// 没变，`is_protected` 这类基于字符串匹配的检查完全看不出来，但
    /// `root_identity_holds` 用 `symlink_metadata` 复核时会穿过这层
    /// 替换掉的祖先，落到完全不同的 inode 上，从而被挡下。
    #[cfg(unix)]
    #[test]
    fn root_identity_guard_blocks_when_ancestor_becomes_symlink() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join("qc_identity_ancestor_swap_cleaner");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let real_ancestor = base.join("real_ancestor");
        std::fs::create_dir_all(&real_ancestor).unwrap();
        let scanned_path = real_ancestor.join("leaf.bin");
        std::fs::write(&scanned_path, b"safe payload").unwrap();

        let identity = crate::core::model::capture_identity(&scanned_path);
        assert!(identity.is_some());

        // 攻击：把祖先目录整个换成指向别处的符号链接，链接目标里放一个
        // 同名但内容不同的文件；用户勾选时记下的路径字符串完全没变。
        let decoy_dir = base.join("decoy");
        std::fs::create_dir_all(&decoy_dir).unwrap();
        std::fs::write(
            decoy_dir.join("leaf.bin"),
            b"decoy payload, deliberately different length",
        )
        .unwrap();
        std::fs::remove_dir_all(&real_ancestor).unwrap();
        symlink(&decoy_dir, &real_ancestor).unwrap();

        assert!(
            !root_identity_holds(&scanned_path, identity),
            "祖先目录被换成符号链接后复核应当失败"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    /// `remove_dir: false` 通道的核心不变量：根身份不变（长期存在的缓存
    /// 目录本来就是这样）也要能挡住「某个子项被替换」——这正是「只验根
    /// 对这条通道无效」的原因，叶子—父目录绑定必须独立起作用。
    #[test]
    fn leaf_binding_rejects_a_swapped_child_but_not_its_sibling() {
        let base = std::env::temp_dir().join("qc_leaf_binding_swap");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let sibling = base.join("sibling.txt");
        std::fs::write(&sibling, b"ordinary cache entry").unwrap();
        let victim = base.join("victim.txt");
        std::fs::write(&victim, b"original cache entry").unwrap();

        // 模拟 clean_dir_contents 在 read_dir 那一刻拍下的身份
        let parent_identity = crate::core::model::capture_identity(&base);
        let sibling_identity = crate::core::model::capture_identity(&sibling);
        let victim_identity = crate::core::model::capture_identity(&victim);
        assert!(parent_identity.is_some() && sibling_identity.is_some());

        // 攻击：read_dir 之后、真正删除之前，victim 被换成了别的内容；
        // sibling 保持原样，根 `base` 自己也没变——「验根没用」的场景。
        std::fs::remove_file(&victim).unwrap();
        std::fs::write(
            &victim,
            b"attacker payload with a deliberately different length",
        )
        .unwrap();

        assert!(
            leaf_binding_holds(&base, parent_identity, &sibling, sibling_identity),
            "没被动过的兄弟节点不该被牵连"
        );
        assert!(
            !leaf_binding_holds(&base, parent_identity, &victim, victim_identity),
            "被替换的子项必须被挡下"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    /// 任意身份缺失都不能授权删除。
    #[test]
    fn leaf_binding_rejects_when_snapshots_are_missing() {
        assert!(!leaf_binding_holds(
            Path::new("/no/such/parent"),
            None,
            Path::new("/no/such/child"),
            None,
        ));
    }

    /// 集成级验证：`clean_dir_contents` 在没有任何替换发生时，行为必须
    /// 和加防护之前完全一致——叶子绑定检查不能误伤正常清理。
    #[test]
    fn clean_dir_contents_still_cleans_normally_with_identity_binding() {
        let base = make_tree("identity_normal_sweep", 15, 96);
        let p = CleanProgress::new(15, 15 * 96);

        let report = clean_dir_contents(&base, &p);

        assert!(report.failed.is_empty());
        assert_eq!(p.snapshot().files, 15);
        assert!(base.exists());
        assert_eq!(std::fs::read_dir(&base).unwrap().count(), 0);

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

    // ---- 任务 1：活数据库删除级闸门 + SQLite 家族删除顺序 ----

    /// 目录顶层带活库标记：整个目录在 `clean_path` 这一关就被拒绝，不会
    /// 走到 `delete_tree`。
    #[test]
    fn clean_path_rejects_live_database_directory() {
        let base = std::env::temp_dir().join("qc_clean_path_live_db_dir");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("cache.otc"), b"x").unwrap();
        std::fs::write(base.join("cache.otc-wal"), b"x").unwrap();

        let p = CleanProgress::default();
        assert_eq!(clean_path(&base, &p), CleanResult::Failed);
        assert!(base.exists(), "活数据库目录必须原地保留");

        let _ = std::fs::remove_dir_all(&base);
    }

    /// 主库 + 伴随文件同时存在：单独把主库文件本身作为清理目标也要被拒绝，
    /// 不依赖调用方先判断它所在的目录。
    #[test]
    fn clean_path_rejects_live_database_file() {
        let base = std::env::temp_dir().join("qc_clean_path_live_db_file");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let db = base.join("Cache.db");
        let wal = base.join("Cache.db-WAL");
        std::fs::write(&db, b"x").unwrap();
        std::fs::write(&wal, b"x").unwrap();

        let p = CleanProgress::default();
        assert_eq!(clean_path(&db, &p), CleanResult::Failed);
        assert!(db.exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    /// 没有伴随文件的孤立 `.db`（缩略图缓存、iOS 备份的 `Manifest.db` 这类
    /// 正常清理目标的形状）不能被这道闸门误伤，必须能正常删除。
    #[test]
    fn clean_path_allows_lone_db_file() {
        let base = std::env::temp_dir().join("qc_clean_path_lone_db");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let db = base.join("Manifest.db");
        std::fs::write(&db, b"x").unwrap();

        let p = CleanProgress::default();
        assert_eq!(clean_path(&db, &p), CleanResult::Ok);
        assert!(!db.exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    /// `delete_sqlite_family` 的排序：伴随文件排在主库前面，方便调用方按
    /// 这个顺序串行删除——先删 `-wal`/`-shm`/`-journal`，最后才删主库。
    #[test]
    fn sqlite_family_orders_companions_before_main() {
        let mut members = [
            (PathBuf::from("/tmp/x/cache.db"), 10),
            (PathBuf::from("/tmp/x/cache.db-wal"), 1),
            (PathBuf::from("/tmp/x/cache.db-shm"), 1),
        ];
        members.sort_by_key(|(path, _)| {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            !crate::core::safety::is_sqlite_companion_name(name)
        });
        let names: Vec<&str> = members
            .iter()
            .map(|(p, _)| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names.last(), Some(&"cache.db"), "主库必须排在最后");
        assert!(names[..2].contains(&"cache.db-wal"));
        assert!(names[..2].contains(&"cache.db-shm"));
    }

    /// 家族同时含主库和伴随文件时必须整组拒绝。不能依赖 remove_file 在
    /// 文件被打开时失败——Unix 允许把正在使用的文件 unlink 掉。
    #[test]
    fn sqlite_family_with_companion_is_rejected_as_live() {
        let base = std::env::temp_dir().join("qc_sqlite_family_atomic");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let db = base.join("cache.db");
        std::fs::write(&db, b"payload").unwrap();
        let shm = base.join("cache.db-shm");
        std::fs::write(&shm, b"x").unwrap();
        let wal = base.join("cache.db-wal");
        std::fs::write(&wal, b"x").unwrap();

        let members = vec![(db.clone(), 7), (wal.clone(), 0), (shm.clone(), 1)];
        let p = CleanProgress::default();
        let undeleted = delete_sqlite_family(members, &p);

        assert!(db.exists(), "伴随文件删除失败时，主库不能被删除");
        assert!(shm.exists(), "活库家族必须整组原地保留");
        assert!(wal.exists(), "活库家族必须整组原地保留");
        assert_eq!(undeleted, 3);

        let _ = std::fs::remove_dir_all(&base);
    }

    /// 根目标顶层没有数据库，但嵌套目录里有活跃 SQLite 家族时，普通文件
    /// 可以清理，数据库家族及其祖先目录必须保留。
    #[test]
    fn delete_tree_rejects_nested_live_sqlite_family() {
        let base = std::env::temp_dir().join("qc_delete_tree_sqlite_family");
        let _ = std::fs::remove_dir_all(&base);
        // 家族文件放进子目录 `nested`，不直接放在 `base` 顶层：`base` 本身
        // 作为 `clean_path` 的整删目标要先过 `is_live_database` 这道目录级
        // 闸门，而那道闸门只看目标**自己**的顶层——把家族放在顶层会让
        // `base` 自己被判成活库目录拒删，测的就不是这里想验证的「分组按
        // 序删除」这件事了。放进子目录，闸门只查一次 `base`（查不出东
        // 西），`delete_tree` 递归到 `nested` 时才会走到分组删除的代码。
        let nested = base.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(base.join("plain.txt"), b"x").unwrap();
        std::fs::write(nested.join("app.db"), b"x").unwrap();
        std::fs::write(nested.join("app.db-wal"), b"x").unwrap();
        std::fs::write(nested.join("app.db-shm"), b"x").unwrap();

        let p = CleanProgress::new(4, 4);
        assert_eq!(clean_path(&base, &p), CleanResult::Failed);
        assert!(!base.join("plain.txt").exists());
        assert!(nested.join("app.db").exists());
        assert!(nested.join("app.db-wal").exists());
        assert!(nested.join("app.db-shm").exists());
    }
}
