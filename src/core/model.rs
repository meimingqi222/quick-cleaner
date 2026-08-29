//! 核心通用数据模型与工具函数

use std::path::{Path, PathBuf};

/// APFS 本地快照的虚拟路径前缀。
///
/// 快照没有可枚举的文件系统路径，扫描管线又要求每个目标都有一个 `PathBuf`，
/// 所以用 `tmutil://snapshot/<name>` 这条虚拟路径代表它：
/// `categories::macos` 造，`scanner` 靠它跳过 `symlink_metadata` 与称重
/// （COW 快照的实际占用取不到，一律记 0），`cleaner` 靠它路由到
/// `tmutil deletelocalsnapshots`。
///
/// 三方各自写一遍前缀字面量必然漂移，构造与判定都只走这里的三个函数。
const SNAPSHOT_PREFIX: &str = "tmutil://snapshot/";

/// Docker 镜像的虚拟路径前缀，余文是 `docker image rm` 的引用参数
/// （`repo:tag` 或镜像 ID）。
///
/// 镜像在虚拟机磁盘文件里，和快照同理没有宿主路径：`categories::docker`
/// 造（体积走 `ScanTarget::size_hint`），`scanner` 跳过称重，`cleaner`
/// 取出引用参数路由到 `docker image rm`。余文**不做二次解析**——repo
/// 可含斜杠和冒口（`ghcr.io/o/img:1.0`）、镜像 ID 含 `sha256:` 前缀，
/// 靠前缀一刀切再整段取回最稳。
const DOCKER_PREFIX: &str = "docker://image/";

/// 由快照名造虚拟路径。
pub fn snapshot_path(name: &str) -> PathBuf {
    PathBuf::from(format!("{SNAPSHOT_PREFIX}{name}"))
}

/// 由 rmi 引用参数造 Docker 镜像虚拟路径。
pub fn docker_image_path(rmi_ref: &str) -> PathBuf {
    PathBuf::from(format!("{DOCKER_PREFIX}{rmi_ref}"))
}

/// 是否是虚拟路径——即不该拿去做任何文件系统调用的目标。
pub fn is_virtual_path(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.starts_with(SNAPSHOT_PREFIX) || s.starts_with(DOCKER_PREFIX) || is_brew_cleanup(path)
}

/// brew 清理的虚拟路径（`brew://cleanup`），见 `core::brew`。
///
/// 判等不判前缀：brew 清理只有一个目标，余文没有意义，不接受
/// `brew://anything` 被当成虚拟路径放行。
fn is_brew_cleanup(path: &Path) -> bool {
    path.to_string_lossy() == "brew://cleanup"
}

/// 取回虚拟路径里的快照名，不是快照路径则为 `None`。
pub fn snapshot_name(path: &Path) -> Option<String> {
    path.to_string_lossy()
        .strip_prefix(SNAPSHOT_PREFIX)
        .map(str::to_string)
}

/// 取回虚拟路径里的 rmi 引用参数，不是 Docker 镜像路径则为 `None`。
pub fn docker_rmi_ref(path: &Path) -> Option<String> {
    path.to_string_lossy()
        .strip_prefix(DOCKER_PREFIX)
        .map(str::to_string)
}

/// 复选框三态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    Off,
    On,
    Partial,
}

impl Check {
    /// 由「选中数 / 总数」推出父级复选框的三态。
    ///
    /// 空集合视为未选中，避免出现「零个子项却显示全选」。
    pub fn from_counts(selected: usize, total: usize) -> Self {
        if total == 0 || selected == 0 {
            Check::Off
        } else if selected >= total {
            Check::On
        } else {
            Check::Partial
        }
    }
}

/// 格式化字节大小为可读字符串（KB, MB, GB, TB）
///
/// 进制按平台对齐系统显示标准：
///   - Windows 资源管理器用 1024 进制（KiB/MiB/GiB），自 Windows 95 起未变
///   - macOS Finder / 储存空间用 1000 进制（SI），自 10.6 Snow Leopard 起改用
///
/// 调用方无需关心平台差异，统一调 `fmt_size` 即可。
pub fn fmt_size(bytes: u64) -> String {
    // cfg!() 在编译时求值，整个 if 会在 const eval 阶段折叠为常量。
    const BASE: f64 = if cfg!(windows) { 1024.0 } else { 1000.0 };
    const KB: f64 = BASE;
    const MB: f64 = KB * BASE;
    const GB: f64 = MB * BASE;
    const TB: f64 = GB * BASE;

    let b = bytes as f64;
    if b >= TB {
        format!("{:.2} TB", b / TB)
    } else if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// 内存 / 交换区专用的容量格式化：**恒定 1024 进位**。
///
/// 不能复用 [`fmt_size`]：那个在 macOS / Linux 上按 1000 进位，因为磁盘容量
/// 从 macOS 10.6 起就是十进制口径（Finder 说 500 GB 就是 500×10⁹ 字节）。
/// 内存不是——内存条永远按 2 的幂出货，系统报的也是二进制口径：32 GiB 的机器
/// `sysinfo` 返回 34_359_738_368 字节，除以 1000³ 会显示成「34.36 GB」，而
/// 「关于本机」和活动监视器都写「32 GB」。同一台机器两个数字，用户只会认为
/// 程序算错了。
pub fn fmt_mem(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;

    let b = bytes as f64;
    if b >= TB {
        format!("{:.2} TB", b / TB)
    } else if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// 给大数字添加千分位逗号，提升可读性
pub fn commas(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// 字符串长度截断加省略号
pub fn truncate(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{t}…")
    } else {
        t
    }
}

/// 扫描期对清理目标的物理身份快照，删除前用来复验「这还是当时看到的
/// 那个东西吗」（TOCTOU 防护）。
///
/// 背景：`cleaner::clean_path` 删除前只做两件事——`symlink_metadata` 查
/// 存在性、`is_protected` 查保护表——这两项检查看的都是**路径字符串**
/// 和**是否存在**，字符串没变、文件也存在的话，中间被整体换成完全
/// 不同的内容照样通过。扫描（尤其是开发垃圾的发现式扫描）动辄几十秒，
/// 用户看完列表再点「清理」，窗口可以到几十秒甚至几分钟，足够让另一个
/// 进程（安装器、恶意软件，甚至用户自己的脚本）在原地放一个新东西。
///
/// 各平台字段选择都遵循同一个原则——从已经在做的 `Metadata` 查询里
/// 顺手取，不为了这个特性单独增加系统调用（例外见下面 Windows 的
/// 说明）：
///
/// - **Unix**：`dev`（设备号）+ `ino`（inode 号）是文件在文件系统里
///   唯一确定物理身份的二元组；再叠 `mtime` + `len` 是防极端情况下
///   inode 号被内核复用（旧文件删掉，新文件恰好分到同一个 inode），
///   同时也堵上「同一秒内原地覆盖写入、mtime 精度不够看不出差异」这条
///   缝——四者都在 `Metadata` / `MetadataExt` 里，零额外开销。
/// - **Windows**：稳定 API 的 `Metadata` 不含文件索引。`file_index` /
///   `volume_serial_number` 属于 nightly `windows_by_handle`，`DirEntry::
///   metadata` 也不会填这两个字段；为了拿它们再开一次句柄，会打破
///   「从已经在做的 Metadata 查询里顺手取」这条前提。因此退化为
///   `mtime + len` 弱校验：挡得住「内容被整体换掉」（长度或修改时间几乎
///   不可能保持一致），挡不住精确保持这两项的定向攻击。
///
/// 额外收益：复验用的是 `symlink_metadata`，只是不穿透目标**自身**的
/// 符号链接，路径中间各级目录该怎么解析还是怎么解析——这是操作系统
/// 路径解析的默认行为，不是这里特意加的逻辑。也就是说，如果扫描完成后
/// 目标的某个**祖先目录**被换成了指向别处的符号链接，字符串形式的路径
/// 完全没变，`is_protected` 照样放行，但重新 `stat` 出来的 dev/ino
/// 必然对不上快照——这个设计顺带堵上了这个口子，不需要专门再写一遍
/// 祖先链接检测。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetIdentity {
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
    /// 秒级修改时间。两个平台都存：Unix 上是 dev/ino 之外的补充判据，
    /// Windows 上和 `len` 一起构成弱校验的全部依据。
    mtime: i64,
    /// 文件长度。两个平台都存，原因见类型文档——在 Unix 上补上「同一秒
    /// 内原地覆盖写、mtime 精度不够」这个缝，在 Windows 上是弱校验的
    /// 另一半依据。
    len: u64,
}

impl TargetIdentity {
    /// 从一次已经拿到手的 `Metadata` 构造身份。调用方如果本来就要
    /// `symlink_metadata` / `entry.metadata()`（扫描、`read_dir` 本就要做
    /// 这一步），这里不产生任何新的系统调用。
    ///
    /// 拿不到 mtime（理论上只有极少数虚拟/网络文件系统会这样）就返回
    /// `None`。真实目标的扫描器和删除边界会据此 fail closed；只有明确的
    /// 虚拟目标允许没有文件系统身份。
    pub fn from_metadata(md: &std::fs::Metadata) -> Option<Self> {
        let mtime = mtime_secs(md)?;
        let len = md.len();
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Some(Self {
                dev: md.dev(),
                ino: md.ino(),
                mtime,
                len,
            })
        }
        #[cfg(windows)]
        {
            Some(Self { mtime, len })
        }
    }

    /// 对 `path` 重新取一次身份，和快照比对是否一致。
    ///
    /// 用 `symlink_metadata` 而不是 `metadata`：不穿透目标自身的符号
    /// 链接（否则复核会绕到链接指向的数据上，这是 `delete_tree` 一贯的
    /// 规则），但路径中间各级目录的符号链接照常解析——这正是「祖先目录
    /// 被换成符号链接也能被挡下」的原因，见类型文档。
    ///
    /// 路径读不出来（已经不存在、权限变了）一律算「对不上」：既然拿不到
    /// 现状就没法确认它还是原来那个东西，宁可保守拒绝。
    pub fn recheck(&self, path: &Path) -> bool {
        std::fs::symlink_metadata(path)
            .ok()
            .and_then(|md| Self::from_metadata(&md))
            .is_some_and(|now| now == *self)
    }
}

fn mtime_secs(md: &std::fs::Metadata) -> Option<i64> {
    let t = md.modified().ok()?;
    Some(match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        // 1970 年之前的时间戳：取负值而不是当成拿不到处理——这类文件
        // 不常见（老磁带镜像、部分压缩包）但确实存在，没必要连累它们
        // 整个失去身份防护。
        Err(e) => -(e.duration().as_secs() as i64),
    })
}

/// 便捷封装：直接对路径取一次身份快照，内部就是一次 `symlink_metadata`
/// 加上 [`TargetIdentity::from_metadata`]。给扫描器里没有现成 `Metadata`
/// 可复用的地方用（比如发现式扫描的几条通道，称重用的是内存里的树/
/// 遍历累加值，不会顺手留一份 `Metadata`）——多付一次系统调用的代价，
/// 换这个目标也能享受身份防护。一个目标一次 stat，相对于称重要遍历的
/// 成千上万个文件可以忽略不计。
pub fn capture_identity(path: &Path) -> Option<TargetIdentity> {
    std::fs::symlink_metadata(path)
        .ok()
        .and_then(|md| TargetIdentity::from_metadata(&md))
}

#[cfg(test)]
mod tests {
    /// 32 GiB 的机器上「关于本机」写 32 GB，十进制口径会显示成 34.36 GB。
    /// 这是当初内存卡片显示错的原点，钉死它。
    #[test]
    fn memory_uses_binary_units_like_the_system_does() {
        assert_eq!(fmt_mem(32 * 1024 * 1024 * 1024), "32.00 GB");
        assert_eq!(fmt_mem(16 * 1024 * 1024 * 1024), "16.00 GB");
        // 同一个字节数走磁盘口径（macOS 十进制）本来就该是另一个数，
        // 两个函数各司其职，不是同一个 bug 的两种写法。
        #[cfg(not(windows))]
        assert_eq!(fmt_size(32 * 1024 * 1024 * 1024), "34.36 GB");
    }

    use super::*;

    /// 卷标签靠它截断。按字节切会在中文字符中间 panic。
    #[test]
    fn truncate_is_char_safe() {
        assert_eq!(truncate("短名", 22), "短名");
        // 24 字节 / 8 字符，不足 22 字符，应原样返回
        assert_eq!(truncate("我的外置移动硬盘", 22), "我的外置移动硬盘");
        assert_eq!(truncate("一二三四", 2), "一二…");
        assert_eq!(truncate("🍎🍎🍎", 1), "🍎…");
    }

    /// 构造与解析必须是一对：cleaner 拿 `snapshot_name` 的结果直接喂给
    /// `tmutil deletelocalsnapshots`，多切或少切一段前缀就是删不掉。
    #[test]
    fn snapshot_path_round_trips() {
        let p = snapshot_path("com.apple.TimeMachine.2024-01-15-123456");
        assert!(is_virtual_path(&p));
        assert_eq!(
            snapshot_name(&p).as_deref(),
            Some("com.apple.TimeMachine.2024-01-15-123456")
        );
    }

    /// 构造与解析必须是一对：cleaner 拿 `docker_rmi_ref` 的结果直接喂给
    /// `docker image rm`。引用参数里的斜杠、冒口（注册表仓库）、
    /// `sha256:` 前缀都不能被切掉。
    #[test]
    fn docker_path_round_trips() {
        for rmi_ref in ["nginx:1.25", "ghcr.io/owner/img:1.0", "a1b2c3d4e5f6"] {
            let p = docker_image_path(rmi_ref);
            assert!(is_virtual_path(&p));
            assert_eq!(docker_rmi_ref(&p).as_deref(), Some(rmi_ref));
        }
    }

    #[test]
    fn real_paths_are_not_virtual() {
        let p = Path::new("/Users/me/Library/Caches");
        assert!(!is_virtual_path(p));
        assert_eq!(snapshot_name(p), None);
        assert_eq!(docker_rmi_ref(p), None);
    }

    #[test]
    fn test_fmt_size() {
        assert_eq!(fmt_size(500), "500 B");
        #[cfg(windows)]
        {
            assert_eq!(fmt_size(2048), "2 KB");
            assert_eq!(fmt_size(15 * 1024 * 1024), "15.0 MB");
            assert_eq!(fmt_size(2 * 1024 * 1024 * 1024), "2.00 GB");
        }
        #[cfg(not(windows))]
        {
            assert_eq!(fmt_size(2000), "2 KB");
            assert_eq!(fmt_size(15 * 1000 * 1000), "15.0 MB");
            assert_eq!(fmt_size(2 * 1000 * 1000 * 1000), "2.00 GB");
        }
    }

    #[test]
    fn test_check_from_counts() {
        assert_eq!(Check::from_counts(0, 0), Check::Off);
        assert_eq!(Check::from_counts(0, 5), Check::Off);
        assert_eq!(Check::from_counts(2, 5), Check::Partial);
        assert_eq!(Check::from_counts(5, 5), Check::On);
    }

    #[test]
    fn test_commas() {
        assert_eq!(commas(100), "100");
        assert_eq!(commas(1000), "1,000");
        assert_eq!(commas(1000000), "1,000,000");
    }

    // ---- TargetIdentity ----

    /// 没变的文件复核必须通过，否则这个特性会让老路径大面积拒删。
    #[test]
    fn identity_recheck_passes_for_unchanged_file() {
        let path = crate::core::testing::file_path("qc_identity_unchanged");
        std::fs::write(&path, b"payload").unwrap();

        let id = capture_identity(&path).expect("应该能拿到身份");
        assert!(id.recheck(&path));

        let _ = std::fs::remove_file(&path);
    }

    /// 核心场景：扫描后目标被删掉重建成同名的新内容（典型的 rename 替换
    /// 手法），复核必须能识破——旧身份对应的 inode/mtime（Windows 上是
    /// len/mtime）已经不是这个路径当下的了。
    #[test]
    fn identity_recheck_fails_after_delete_and_recreate() {
        let path = crate::core::testing::file_path("qc_identity_swapped");
        std::fs::write(&path, b"original").unwrap();
        let id = capture_identity(&path).expect("应该能拿到身份");

        std::fs::remove_file(&path).unwrap();
        // 睡一拍不现实也没必要：inode 号变了（几乎总是变，除非恰好复用），
        // 或者长度/mtime 变了，二者只要有一个不一致复核就会失败。
        std::fs::write(&path, b"attacker payload, different length").unwrap();

        assert!(!id.recheck(&path), "内容被整体替换后复核应当失败");

        let _ = std::fs::remove_file(&path);
    }

    /// 路径消失也算「对不上」：既然连现状都读不出来，没法确认它还是
    /// 原来那个东西，必须保守拒绝而不是默认放行。
    #[test]
    fn identity_recheck_fails_when_path_vanishes() {
        let path = crate::core::testing::file_path("qc_identity_gone_completely");
        std::fs::write(&path, b"x").unwrap();
        let id = capture_identity(&path).expect("应该能拿到身份");
        std::fs::remove_file(&path).unwrap();

        assert!(!id.recheck(&path));
    }

    /// 额外收益：祖先目录被换成指向别处的符号链接时，即便叶子文件名
    /// 完全一样，`symlink_metadata` 穿过祖先链接解析出来的身份也会
    /// 和快照对不上——不需要专门写一遍「祖先是不是链接」的检测。
    #[cfg(unix)]
    #[test]
    fn identity_recheck_fails_when_ancestor_becomes_symlink() {
        use std::os::unix::fs::symlink;

        let base = crate::core::testing::fixture("qc_identity_ancestor_swap");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let real_ancestor = base.join("real_ancestor");
        std::fs::create_dir_all(&real_ancestor).unwrap();
        let scanned_path = real_ancestor.join("leaf.bin");
        std::fs::write(&scanned_path, b"safe payload").unwrap();

        let id = capture_identity(&scanned_path).expect("应该能拿到身份");

        // 把祖先目录整个换成指向别处的符号链接，链接目标里放一个同名
        // 但内容不同的文件——路径字符串（base/real_ancestor/leaf.bin）
        // 一个字都没变。
        let decoy_dir = base.join("decoy");
        std::fs::create_dir_all(&decoy_dir).unwrap();
        std::fs::write(
            decoy_dir.join("leaf.bin"),
            b"decoy payload, longer than original",
        )
        .unwrap();
        std::fs::remove_dir_all(&real_ancestor).unwrap();
        symlink(&decoy_dir, &real_ancestor).unwrap();

        assert!(
            !id.recheck(&scanned_path),
            "祖先目录被换成符号链接后复核应当失败"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    /// `len` 在两个平台上都参与比对，必须能挡住「体积明显不同」的
    /// 替换——这是弱身份（Windows：mtime + len）与强身份（Unix：dev +
    /// ino + mtime + len）共有的最低保证。
    #[test]
    fn identity_recheck_fails_on_size_change_in_place() {
        let path = crate::core::testing::file_path("qc_identity_size_change");
        std::fs::write(&path, b"short").unwrap();
        let id = capture_identity(&path).expect("应该能拿到身份");

        // 不删除、直接原地覆盖写入更长的内容：在支持原地覆盖的文件系统上
        // inode 号可能不变（Unix），但 mtime 必然更新、体积也变了。
        std::fs::write(&path, b"this payload is a lot longer than before").unwrap();

        assert!(!id.recheck(&path));

        let _ = std::fs::remove_file(&path);
    }
}
