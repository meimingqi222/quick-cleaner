//! 磁盘空间分析通用领域模型

use std::path::{Path, PathBuf};

/// 跨平台卷标识：Windows 是盘符（`C:`），Unix 是挂载点（`/`、`/Volumes/外接盘`）。
///
/// 旧契约用 `char` 表达卷，macOS 上只能返回 `'/'` 占位，`/Volumes` 下的外接盘
/// 表达不了。`VolumeId` 持有挂载点路径和一个展示用的标签，两边都能用。
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct VolumeId {
    /// 挂载点路径：Windows 上是 `C:\`，macOS 上是 `/` 或 `/Volumes/...`
    mount: PathBuf,
    /// 展示标签：Windows 上是 `C:`，macOS 上是挂载点路径的字符串
    label: String,
}

impl VolumeId {
    /// Windows 上从盘符构造。
    #[cfg(windows)]
    pub fn from_drive_letter(letter: char) -> Self {
        let letter = letter.to_ascii_uppercase();
        Self {
            mount: PathBuf::from(format!("{}:\\", letter)),
            label: format!("{}:", letter),
        }
    }

    /// Unix 上从挂载点路径构造。
    #[cfg(not(windows))]
    pub fn from_mount_point(mount: PathBuf) -> Self {
        let label = mount.display().to_string();
        Self { mount, label }
    }

    /// Unix 上从挂载点路径和自定义标签构造。
    ///
    /// 外接盘的挂载点路径可能很长（`/Volumes/外接盘`），
    /// 但用户看到的卷名可能只是「外接盘」。这个方法允许分离两者。
    #[cfg(not(windows))]
    pub fn from_mount_point_with_label(mount: PathBuf, label: String) -> Self {
        Self { mount, label }
    }

    /// 用户可见的卷标签：`"C:"` / `"/"` / `"/Volumes/外接盘"`。
    pub fn display(&self) -> &str {
        &self.label
    }

    /// 挂载点路径，用于 `statfs` / `GetDiskFreeSpaceEx` 等系统调用。
    pub fn mount_point(&self) -> &Path {
        &self.mount
    }

    /// Windows 上的盘符（大写）。非 Windows 返回 `None`。
    #[cfg(windows)]
    pub fn drive_letter(&self) -> Option<char> {
        self.label.chars().next().map(|c| c.to_ascii_uppercase())
    }
}

impl std::fmt::Display for VolumeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

/// 扫描用户内容时统一跳过的目录名（跨平台共享的**基表**）。
///
/// 隐藏目录、构建产物、依赖缓存、系统骨架等不含用户自己的文件。
/// 以前这张表在 `core::disk` / `core::fs_query` / `core::declutter::photos`
/// 各有一份，已经漂移出三种口径（回收站只有其中一份挡掉）。三个入口现在
/// 都从这里取基表，各自只加自己那几条增量。
///
/// `name` 传目录名本身，不是路径。
pub fn is_ignored_dir_name(name: &str) -> bool {
    // 隐藏目录一律跳过（.cache、.npm、.cargo、.git、.vscode …）
    if name.starts_with('.') {
        return true;
    }
    let s = name.to_lowercase();
    matches!(
        s.as_str(),
        "node_modules"
            | "library"
            | "appdata"
            | "application data"
            | "application support"
            | "local settings"
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
            | "sdk"
            | "javadoc"
    ) || is_system_meta_dir_name(&s)
}

/// 系统自己的元数据目录。回收站尤其重要：里面全是已删除文件，
/// 不该在「大文件 / 重复文件」里当成用户内容列出来给人再删一次。
///
/// `$Recycle.Bin` 不以 `.` 开头，隐藏目录规则挡不住它。
fn is_system_meta_dir_name(lower: &str) -> bool {
    matches!(lower, "$recycle.bin" | "system volume information")
}

/// 冗余整理（大文件候选枚举）时跳过的目录名。
pub fn is_declutter_ignored_dir_name(name: &str) -> bool {
    is_ignored_dir_name(name)
}

/// 图片整理额外跳过的目录名：素材/文档目录里的图多是软件自带资源，
/// 不是用户相册内容。
pub fn is_photo_ignored_dir_name(name: &str) -> bool {
    if is_ignored_dir_name(name) {
        return true;
    }
    matches!(name.to_lowercase().as_str(), "site" | "help" | "manuals")
}

/// 文件搜索结果条目。跨平台共用。
#[derive(Clone, Debug)]
pub struct SearchHit {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: u64,
}

/// 文件名匹配模式（搜索框输入解析后得到）。
///
/// 三种模式按用户输入自动选择，无需 UI 切换：
/// - `Substring`：不含通配符 → 大小写不敏感子串匹配（向后兼容）
/// - `Wildcard`：含 `*` 或 `?` → 通配符匹配
/// - `Empty`：空查询 → 由调用方走 top-N 路径
///
/// `Wildcard` 内部预提取所有「字面子串片段」（连续的非通配符字符），
/// 匹配时先用 `contains` 快速过滤——`contains` 走 SIMD 优化，比 DP
/// 快一两个量级。只有通过全部字面片段过滤的文件名才走 DP 精确验证。
/// 这样 `*.mp4` 实际只走一次 `contains(".mp4")`，DP 几乎不会被触发。
#[derive(Clone, Debug)]
pub enum NamePattern {
    Substring(String),
    Wildcard {
        chars: Vec<char>,
        /// 字面子串片段（按出现顺序）。空片段被丢弃。
        /// 例如 `*report*.pdf` → ["report", ".pdf"]
        literals: Vec<String>,
    },
    Empty,
}

impl NamePattern {
    /// 解析搜索框输入。调用方已 `trim`，这里只做大小写归一化。
    pub fn parse(query: &str) -> Self {
        if query.is_empty() {
            return NamePattern::Empty;
        }
        if query.contains('*') || query.contains('?') {
            let lower: Vec<char> = query.to_ascii_lowercase().chars().collect();
            // 提取所有连续的非通配符字符作为字面片段，用于快路径过滤
            let literals = extract_literals(&lower);
            NamePattern::Wildcard {
                chars: lower,
                literals,
            }
        } else {
            NamePattern::Substring(query.to_ascii_lowercase())
        }
    }

    /// 判断原始文件名是否匹配。不分配小写副本；子串走字节窗口的
    /// `eq_ignore_ascii_case`。通配符在字面片段都命中后才对候选做一次小写。
    pub fn matches_raw(&self, name: &str) -> bool {
        match self {
            NamePattern::Empty => false,
            NamePattern::Substring(s) => contains_ignore_ascii_case(name, s),
            NamePattern::Wildcard { chars, literals } => {
                for lit in literals {
                    if !contains_ignore_ascii_case(name, lit) {
                        return false;
                    }
                }
                let lower = name.to_ascii_lowercase();
                wildcard_match(&lower, chars)
            }
        }
    }

    /// 判断文件名（**已小写化**）是否匹配当前模式。
    /// 调用方负责把文件名转成小写，避免每个文件重复 to_lowercase。
    pub fn matches(&self, name_lower: &str) -> bool {
        match self {
            NamePattern::Empty => false,
            NamePattern::Substring(s) => name_lower.contains(s),
            NamePattern::Wildcard { chars, literals } => {
                // 快路径：所有字面片段都必须作为子串出现。
                // contains 走 SIMD 优化，比 DP 快一两个量级。
                // 大部分不匹配的文件名会在这里被快速淘汰。
                for lit in literals {
                    if !name_lower.contains(lit.as_str()) {
                        return false;
                    }
                }
                // 慢路径：通过字面过滤的候选者走 DP 精确验证
                wildcard_match(name_lower, chars)
            }
        }
    }
}

fn contains_ignore_ascii_case(hay: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    let n = needle_lower.len();
    let h = hay.as_bytes();
    if h.len() < n {
        return false;
    }
    h.windows(n)
        .any(|w| w.eq_ignore_ascii_case(needle_lower.as_bytes()))
}

/// 从通配符 pattern 中提取所有连续的非通配符字符片段。
///
/// `*` 和 `?` 是分隔符。空片段被丢弃。例如：
/// - `*.mp4` → ["mp4"]... 实际是 [".mp4"]（`.` 不是通配符）
/// - `*report*.pdf` → ["report", ".pdf"]
/// - `a?c` → ["a", "c"]
fn extract_literals(pattern: &[char]) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for &c in pattern {
        if c == '*' || c == '?' {
            if !buf.is_empty() {
                out.push(std::mem::take(&mut buf));
            }
        } else {
            buf.push(c);
        }
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

/// 通配符匹配：`*` 匹配任意数量字符，`?` 匹配单个字符。
///
/// 输入的 `text` 和 `pattern` 都应是**已小写化**的字符序列。
/// 用滚动数组 DP，O(n*m) 时间、O(m) 空间。文件名和 pattern 通常都很短
/// （各几十字符以内），单次匹配开销可忽略。
fn wildcard_match(text: &str, pattern: &[char]) -> bool {
    let text: Vec<char> = text.chars().collect();
    let n = text.len();
    let m = pattern.len();
    // dp[j] = 当前 text 行的 pat[..j] 匹配结果
    let mut prev = vec![false; m + 1];
    prev[0] = true;
    // 前导连续 '*' 可以匹配空串
    for j in 1..=m {
        if pattern[j - 1] == '*' {
            prev[j] = prev[j - 1];
        } else {
            break;
        }
    }
    for i in 1..=n {
        let mut curr = vec![false; m + 1];
        // curr[0] 永远是 false（非空 text 匹配空 pattern）
        for j in 1..=m {
            let p = pattern[j - 1];
            if p == '*' {
                // '*' 匹配空 (prev[j]) 或匹配至少一个字符 (curr[j-1])
                curr[j] = prev[j] || curr[j - 1];
            } else if p == '?' || p == text[i - 1] {
                curr[j] = prev[j - 1];
            }
            // 其他情况 curr[j] 保持 false
        }
        // 提前剪枝：如果整行全 false 且没有 '*' 能恢复，可以提前结束
        // 但 '*' 的传播依赖 curr[j-1]，简单起见不做剪枝
        prev = curr;
    }
    prev[m]
}

#[cfg(windows)]
pub use crate::platform::windows::mft::{
    DirUsage, Node, ScanError, ScanResult, SizeTree, ROOT_RECORD as ROOT_NODE,
};

#[cfg(not(windows))]
pub use crate::platform::macos::disk_tree::{
    DirUsage, Node, ScanError, ScanResult, SizeTree, TreeEntry, TreeIndexEntry, TreeSnapshotEntry,
    ROOT_NODE,
};

pub use super::disk_selection::DiskSelectionState;

#[cfg(test)]
mod tests {
    use super::*;

    /// 回收站里全是已删除文件，不该在「大文件 / 重复文件」里当用户内容
    /// 再列一遍。它不以 `.` 开头，隐藏目录规则挡不住，必须显式列入。
    #[test]
    fn recycle_bin_is_ignored_everywhere() {
        for name in ["$RECYCLE.BIN", "$Recycle.Bin", "System Volume Information"] {
            assert!(is_ignored_dir_name(name), "{name} 应被跳过");
            assert!(is_declutter_ignored_dir_name(name), "{name} 应被跳过");
            assert!(is_photo_ignored_dir_name(name), "{name} 应被跳过");
        }
    }

    #[test]
    fn shared_base_and_per_use_extras() {
        // 基表：三个入口口径一致
        for name in [
            "node_modules",
            ".git",
            "Library",
            "__pycache__",
            "DerivedData",
        ] {
            assert!(is_declutter_ignored_dir_name(name));
            assert!(is_photo_ignored_dir_name(name));
        }
        // 图片整理的增量：素材/文档目录只在它这里跳过
        for name in ["site", "help", "manuals"] {
            assert!(is_photo_ignored_dir_name(name), "{name} 图片整理应跳过");
            assert!(!is_declutter_ignored_dir_name(name), "{name} 不属于基表");
        }
        // 普通用户目录不受影响
        for name in ["Pictures", "我的照片", "Projects"] {
            assert!(!is_ignored_dir_name(name));
            assert!(!is_photo_ignored_dir_name(name));
        }
    }

    #[test]
    fn wildcard_match_basic() {
        // 基础通配符语义
        assert!(wildcard_match("abc", &['a', '*', 'c']));
        assert!(wildcard_match("ac", &['a', '*', 'c']));
        assert!(wildcard_match("abxyzc", &['a', '*', 'c']));
        assert!(!wildcard_match("ab", &['a', '*', 'c']));
        assert!(!wildcard_match("bcd", &['a', '*', 'c']));

        // ? 单字符
        assert!(wildcard_match("abc", &['a', '?', 'c']));
        assert!(!wildcard_match("ac", &['a', '?', 'c']));
        assert!(!wildcard_match("abbc", &['a', '?', 'c']));

        // 全 * 匹配任意
        assert!(wildcard_match("anything", &['*']));
        assert!(wildcard_match("", &['*']));

        // 纯字面（无通配符）等价于完全相等
        assert!(wildcard_match("abc", &['a', 'b', 'c']));
        assert!(!wildcard_match("abcd", &['a', 'b', 'c']));
    }

    #[test]
    fn wildcard_match_edge_cases() {
        // 前导连续 *
        assert!(wildcard_match("xyzabc", &['*', '*', 'a', 'b', 'c']));
        // 末尾连续 *
        assert!(wildcard_match("abcxyz", &['a', 'b', 'c', '*', '*']));
        // 中间多个 *
        assert!(wildcard_match(
            "aXXbYYc",
            &['a', '*', '*', 'b', '*', '*', 'c']
        ));
        // 空 pattern 只匹配空串
        assert!(wildcard_match("", &[]));
        assert!(!wildcard_match("a", &[]));
        // 空 text 匹配纯 * pattern
        assert!(wildcard_match("", &['*', '*']));
        assert!(!wildcard_match("", &['*', 'a']));
    }

    #[test]
    fn name_pattern_dispatches_correctly() {
        // 空查询
        assert!(matches!(NamePattern::parse(""), NamePattern::Empty));
        assert!(!NamePattern::parse("").matches("anything"));

        // 子串模式（无通配符）。注意 matches 期望已小写化的文件名——
        // 真实调用方（search 函数）会先 to_ascii_lowercase。
        let p = NamePattern::parse("report");
        assert!(matches!(p, NamePattern::Substring(_)));
        assert!(p.matches("annual_report_final.pdf"));
        assert!(p.matches("report")); // 大小写不敏感：pattern 已小写化
        assert!(!p.matches("summary.docx"));

        // 通配符模式
        let p = NamePattern::parse("*.txt");
        assert!(matches!(p, NamePattern::Wildcard { .. }));
        assert!(p.matches("notes.txt"));
        assert!(p.matches("readme.txt"));
        assert!(!p.matches("notes.txt.bak"));

        // ? 通配符
        let p = NamePattern::parse("a?c");
        assert!(p.matches("abc"));
        assert!(!p.matches("ac"));
        assert!(!p.matches("abbc"));

        // *report* 等价于子串 report
        let p = NamePattern::parse("*report*");
        assert!(p.matches("annual_report_final.pdf"));
        assert!(p.matches("report.log"));
        assert!(!p.matches("summary.docx"));

        // 大小写不敏感：pattern 大写输入也会被小写化
        let p = NamePattern::parse("*.TXT");
        assert!(matches!(p, NamePattern::Wildcard { .. }));
        assert!(p.matches("notes.txt"));

        // matches_raw 直接吃原始文件名，搜索热路径不再为每个节点分配小写副本
        let p = NamePattern::parse("Report");
        assert!(p.matches_raw("annual_REPORT_final.pdf"));
        assert!(p.matches_raw("report"));
        assert!(!p.matches_raw("summary.docx"));
        let p = NamePattern::parse("*.TXT");
        assert!(p.matches_raw("notes.txt"));
        assert!(p.matches_raw("README.TXT"));
        assert!(!p.matches_raw("notes.txt.bak"));
    }

    #[test]
    fn wildcard_literal_fast_path_filters_correctly() {
        // *.mp4 的字面片段是 [".mp4"]，不含 .mp4 的文件名应被快速淘汰
        let p = NamePattern::parse("*.mp4");
        assert!(p.matches("video.mp4"));
        assert!(p.matches("movie.MP4".to_ascii_lowercase().as_str())); // "movie.mp4"
        assert!(!p.matches("video.mkv"));
        assert!(!p.matches("mp4_readme.txt")); // "mp4" 不是 ".mp4"
        assert!(!p.matches("trailer.avi"));

        // *report*.pdf 的字面片段是 ["report", ".pdf"]
        let p = NamePattern::parse("*report*.pdf");
        assert!(p.matches("annual_report_final.pdf"));
        assert!(p.matches("report.pdf"));
        assert!(!p.matches("annual_summary.pdf")); // 缺 "report"
        assert!(!p.matches("annual_report_final.docx")); // 缺 ".pdf"

        // 纯 ? 通配符没有字面片段，走纯 DP
        let p = NamePattern::parse("???");
        assert!(p.matches("abc"));
        assert!(!p.matches("ab"));
        assert!(!p.matches("abcd"));
    }

    #[test]
    fn extract_literals_splits_on_wildcards() {
        assert_eq!(
            extract_literals(&['*', '.', 'm', 'p', '4']),
            vec![".mp4".to_string()]
        );
        assert_eq!(
            extract_literals(&['*', 'r', 'e', 'p', '*', '.', 'p', 'd', 'f']),
            vec!["rep".to_string(), ".pdf".to_string()]
        );
        assert_eq!(
            extract_literals(&['a', '?', 'c']),
            vec!["a".to_string(), "c".to_string()]
        );
        // 纯通配符 → 空字面列表
        assert!(extract_literals(&['*', '?', '*']).is_empty());
        // 无通配符 → 整体作为一个字面
        assert_eq!(extract_literals(&['a', 'b', 'c']), vec!["abc".to_string()]);
    }

    /// 构造一个平台合适的绝对路径，`rel` 用 `/` 分段。
    ///
    /// 这些用例原本写死 `C:\\a\\b` 这类 Windows 字面量，但 `Path` 的父子判定是
    /// 平台相关的：Unix 上反斜杠不是分隔符，`C:\\a\\b` 是**单个**组件，
    /// `starts_with("C:\\a")` 恒为 false——父子收编逻辑在 macOS / Linux 上压根
    /// 没被测到，其中 3 个用例还会直接失败。路径按平台构造才能真正覆盖。
    fn p(rel: &str) -> PathBuf {
        let mut base = if cfg!(windows) {
            PathBuf::from("C:\\")
        } else {
            PathBuf::from("/")
        };
        for seg in rel.split('/') {
            base.push(seg);
        }
        base
    }

    #[test]
    fn test_disk_selection_inheritance_and_deselection() {
        let mut state = DiskSelectionState::new();
        let parent = p("Users/test/AppData/Roaming/TRAE SOLO CN");
        let child1 = parent.join("resources");
        let child2 = parent.join("extensions");
        let grandchild = child1.join("app.asar");

        // 1. 选中父文件夹
        state.toggle(&parent, 5_000_000_000);
        assert!(state.is_selected(&parent));
        assert_eq!(state.total_size(), 5_000_000_000);

        // 2. 进入子目录，子项目和孙项目均自动继承选中
        assert!(state.is_selected(&child1));
        assert!(state.is_selected(&child2));
        assert!(state.is_selected(&grandchild));

        // 3. 反选/取消勾选 child1
        state.toggle(&child1, 1_000_000);
        assert!(!state.is_selected(&child1));
        assert!(!state.is_selected(&grandchild)); // 孙项目也随之被排除
        assert!(state.is_selected(&child2)); // child2 仍然保持继承选中
        assert!(state.is_selected(&parent)); // 父级依然保留

        // 4. 重新勾选 child1
        state.toggle(&child1, 1_000_000);
        assert!(state.is_selected(&child1));
        assert!(state.is_selected(&grandchild));

        // 5. 清空选择
        state.clear();
        assert!(!state.is_selected(&parent));
        assert!(!state.is_selected(&child1));
        assert_eq!(state.total_size(), 0);
    }

    #[test]
    fn excluded_child_is_subtracted_from_total() {
        let mut state = DiskSelectionState::new();
        let parent = p("proj");
        let child = parent.join("node_modules");

        state.toggle(&parent, 1_000);
        assert_eq!(state.total_size(), 1_000);

        // 排除一个继承勾选的子项后，汇总体积必须相应减少，
        // 否则确认框会虚报「将释放多少空间」。
        state.toggle(&child, 400);
        assert_eq!(state.total_size(), 600);

        // 撤销排除后恢复
        state.toggle(&child, 400);
        assert_eq!(state.total_size(), 1_000);
    }

    #[test]
    fn total_size_tracks_multiple_explicit_picks() {
        let mut state = DiskSelectionState::new();
        state.toggle(&p("a"), 10);
        state.toggle(&p("b"), 25);
        assert_eq!(state.len(), 2);
        assert_eq!(state.total_size(), 35);

        state.toggle(&p("a"), 10);
        assert_eq!(state.len(), 1);
        assert_eq!(state.total_size(), 25);
    }

    /// 先勾子目录、再勾父目录：父项应当把子项收编，而不是两笔都记账。
    ///
    /// 这个数字会在**执行删除之前**显示给用户看，错了就是在误导人。
    #[test]
    fn selecting_a_parent_absorbs_already_selected_children() {
        let mut st = DiskSelectionState::new();
        let parent = p("a");
        let child = p("a/b");

        st.toggle(&child, 100);
        st.toggle(&parent, 1000);

        assert_eq!(st.total_size(), 1000, "父子体积被重复累加了");
        assert_eq!(st.len(), 1, "子项没有被父项收编");
        assert_eq!(st.resolve_targets(), vec![parent.clone()]);
        assert!(st.is_selected(&child), "子项仍应处于选中（继承自父）");
    }

    /// 但中间隔着排除项的孙子不能被收编——它靠自己那条显式勾选活着。
    #[test]
    fn absorption_stops_at_an_excluded_level() {
        let mut st = DiskSelectionState::new();
        let a = p("a");
        let b = p("a/b");
        let c = p("a/b/c");

        st.toggle(&a, 1000); // 勾 a
        st.toggle(&b, 300); // 排除 b
        st.toggle(&c, 50); // 但 b 底下的 c 还是要删

        assert!(st.is_selected(&a));
        assert!(!st.is_selected(&b));
        assert!(st.is_selected(&c), "显式勾选的 c 不该被 b 的排除吃掉");

        // 再勾一次 a（本就选中，这里退化成取消），c 的显式勾选要留着
        let mut targets = st.resolve_targets();
        targets.sort();
        assert!(targets.contains(&c));
    }

    /// 取消父项时，它底下的排除记录必须一并作废。
    ///
    /// 否则「取消 → 重新勾选」之后，被排除过的子项会静默地继续不删，
    /// 而界面上看起来整个目录都是勾上的。
    #[test]
    fn unchecking_a_parent_clears_its_exclusions() {
        let mut st = DiskSelectionState::new();
        let parent = p("a");
        let child = p("a/b");

        st.toggle(&parent, 1000); // 勾父
        st.toggle(&child, 100); // 排除子
        assert_eq!(st.total_size(), 900);

        st.toggle(&parent, 1000); // 取消父
        assert!(!st.is_selected(&parent));
        assert_eq!(st.total_size(), 0);

        st.toggle(&parent, 1000); // 重新勾父
        assert!(st.is_selected(&child), "重新勾选后子项仍被残留的排除挡着");
        assert_eq!(st.total_size(), 1000);
        assert_eq!(st.resolve_targets(), vec![parent]);
    }

    /// `clear` 之后必须是全新状态，不留任何残渣。
    #[test]
    fn clear_wipes_every_map() {
        let mut st = DiskSelectionState::new();
        let parent = p("a");
        st.toggle(&parent, 1000);
        st.toggle(&parent.join("b"), 100);
        st.clear();

        assert_eq!(st.total_size(), 0);
        assert_eq!(st.len(), 0);
        assert!(st.is_empty());
        assert!(!st.is_selected(&parent));
        assert!(st.resolve_targets().is_empty());
    }

    #[test]
    fn resolve_targets_returns_root_when_nothing_excluded() {
        let mut state = DiskSelectionState::new();
        let root = p("a");
        state.toggle(&root, 10);
        assert_eq!(state.resolve_targets(), vec![root]);
    }

    // ---- 就地子树替换 API 的测试 ----

    /// 构造一棵测试树：
    /// ```text
    /// /root
    /// ├── a.txt (100)
    /// ├── proj
    /// │   ├── b.txt (200)
    /// │   └── src
    /// │       ├── c.txt (300)
    /// │       └── d.txt (400)
    /// └── other
    ///     └── e.txt (500)
    /// ```
    /// 根节点聚合 = 1500, proj 聚合 = 900, src 聚合 = 700, other 聚合 = 500
    #[cfg(not(windows))]
    fn build_test_tree() -> super::SizeTree {
        use super::{SizeTree, TreeIndexEntry};
        let vol = super::VolumeId::from_mount_point(PathBuf::from("/root"));
        let entries = vec![
            TreeIndexEntry {
                parent: 0,
                name: "/root".into(),
                is_dir: true,
                size: 0,
                used: true,
                mtime: 0,
            }, // 0
            TreeIndexEntry {
                parent: 0,
                name: "a.txt".into(),
                is_dir: false,
                size: 100,
                used: true,
                mtime: 0,
            }, // 1
            TreeIndexEntry {
                parent: 0,
                name: "proj".into(),
                is_dir: true,
                size: 0,
                used: true,
                mtime: 0,
            }, // 2
            TreeIndexEntry {
                parent: 2,
                name: "b.txt".into(),
                is_dir: false,
                size: 200,
                used: true,
                mtime: 0,
            }, // 3
            TreeIndexEntry {
                parent: 2,
                name: "src".into(),
                is_dir: true,
                size: 0,
                used: true,
                mtime: 0,
            }, // 4
            TreeIndexEntry {
                parent: 4,
                name: "c.txt".into(),
                is_dir: false,
                size: 300,
                used: true,
                mtime: 0,
            }, // 5
            TreeIndexEntry {
                parent: 4,
                name: "d.txt".into(),
                is_dir: false,
                size: 400,
                used: true,
                mtime: 0,
            }, // 6
            TreeIndexEntry {
                parent: 0,
                name: "other".into(),
                is_dir: true,
                size: 0,
                used: true,
                mtime: 0,
            }, // 7
            TreeIndexEntry {
                parent: 7,
                name: "e.txt".into(),
                is_dir: false,
                size: 500,
                used: true,
                mtime: 0,
            }, // 8
        ];
        SizeTree::from_compact(vol, entries)
    }

    #[cfg(not(windows))]
    #[test]
    fn test_remove_subtree_updates_ancestors() {
        let mut tree = build_test_tree();
        // 删除 proj 子树（idx=2），聚合大小 900
        tree.remove_subtree_inplace(2);

        // 根节点聚合应从 1500 减到 600
        assert_eq!(tree.size_of(tree.root()), 600);
        assert_eq!(tree.file_count_of(tree.root()), 2); // a.txt + e.txt

        // proj 节点应不再有效
        assert!(!tree.valid(2));
        // other 子树不受影响
        assert_eq!(tree.size_of(7), 500);
    }

    #[cfg(not(windows))]
    #[test]
    fn upsert_file_replaces_leaf_and_updates_ancestors() {
        let mut tree = build_test_tree();
        let file = PathBuf::from("/root/proj/b.txt");

        assert!(tree.upsert_file(&file, 250));
        tree.rebuild_child_arrays();

        let node = tree.find_node_by_path(&file).unwrap();
        assert_eq!(tree.size_of(node), 250);
        assert_eq!(tree.size_of(tree.root()), 1_550);
        assert_eq!(tree.file_count_of(tree.root()), 5);
        assert_eq!(
            tree.compact_entries().len(),
            9,
            "持久化时不应保留旧文件墓碑"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn test_remove_then_append_matches_recompute() {
        let mut tree = build_test_tree();
        let old_node = tree
            .find_node_by_path(&PathBuf::from("/root/proj"))
            .unwrap();

        // 删除旧 proj 子树
        tree.remove_subtree_inplace(old_node);

        // 构造新子树：proj 下只有一个 f.txt (999)
        use super::{SizeTree, TreeIndexEntry};
        let new_vol = super::VolumeId::from_mount_point(PathBuf::from("/root/proj"));
        let new_entries = vec![
            TreeIndexEntry {
                parent: 0,
                name: "/root/proj".into(),
                is_dir: true,
                size: 0,
                used: true,
                mtime: 0,
            },
            TreeIndexEntry {
                parent: 0,
                name: "f.txt".into(),
                is_dir: false,
                size: 999,
                used: true,
                mtime: 0,
            },
        ];
        let new_subtree = SizeTree::from_compact(new_vol, new_entries);

        // 找到 proj 的父节点（根节点）
        let parent_idx = tree.find_node_by_path(&PathBuf::from("/root")).unwrap();
        tree.append_subtree(parent_idx, &new_subtree, "proj");
        tree.rebuild_child_arrays();

        // 验证增量更新后的聚合大小
        let incremental_root_size = tree.size_of(tree.root());
        let incremental_root_files = tree.file_count_of(tree.root());

        // 用 recompute_aggregates 做全量重算，对比是否一致
        let mut recompute_tree = tree.clone();
        recompute_tree.recompute_aggregates();

        assert_eq!(
            incremental_root_size,
            recompute_tree.size_of(recompute_tree.root()),
            "增量更新与全量重算的根节点大小不一致"
        );
        assert_eq!(
            incremental_root_files,
            recompute_tree.file_count_of(recompute_tree.root()),
            "增量更新与全量重算的根节点文件数不一致"
        );

        // 根节点聚合 = 100 (a.txt) + 999 (f.txt) + 500 (e.txt) = 1599
        assert_eq!(incremental_root_size, 1599);
        assert_eq!(incremental_root_files, 3);

        // 新 proj 目录应能通过路径找到
        let new_proj = tree.find_node_by_path(&PathBuf::from("/root/proj"));
        assert!(new_proj.is_some(), "新 proj 目录应能通过路径定位");
        assert_eq!(tree.size_of(new_proj.unwrap()), 999);
    }

    #[cfg(not(windows))]
    #[test]
    fn test_remove_nonexistent_subtree_is_noop() {
        let mut tree = build_test_tree();
        let original_size = tree.size_of(tree.root());
        // 删除不存在的节点不应有任何影响
        tree.remove_subtree_inplace(999);
        assert_eq!(tree.size_of(tree.root()), original_size);
    }

    #[cfg(not(windows))]
    #[test]
    fn test_count_used_dirs_and_files() {
        let tree = build_test_tree();
        // 目录: /root, proj, src, other = 4
        assert_eq!(tree.count_used_dirs(), 4);
        // 文件: a.txt, b.txt, c.txt, d.txt, e.txt = 5
        assert_eq!(tree.count_used_files(), 5);
    }

    #[cfg(not(windows))]
    #[test]
    fn tree_entry_is_packed_24_bytes() {
        assert_eq!(std::mem::size_of::<super::TreeEntry>(), 24);
        assert_eq!(std::mem::align_of::<super::TreeEntry>(), 8);
    }

    #[cfg(not(windows))]
    #[test]
    fn from_packed_builds_tree_from_pod_entries_and_pool() {
        // 名字池编码：每项 [u16 le 长度][字节]，name_off 指向项起始。
        // 原先这个用例靠已删除的 compacted_packed 造输入（生产路径只剩
        // walk.rs 一处），这里手工搭一个最小池，覆盖保持不变。
        let names = ["/", "proj", "a.txt", "src.rs"];
        let mut pool = Vec::new();
        let mut offs = Vec::new();
        for name in names {
            offs.push(pool.len() as u32);
            pool.extend_from_slice(&(name.len() as u16).to_le_bytes());
            pool.extend_from_slice(name.as_bytes());
        }
        // 0: 根（parent=ROOT_NODE=0）；1: proj 目录；2: 根下文件；3: proj 下文件。
        // 目录的 size/file_count 存多少都行——build_from_entries_with_pool
        // 会清零后沿父链重算。
        let entries = vec![
            super::TreeEntry::new(super::ROOT_NODE, offs[0], true, 0, 0, 0),
            super::TreeEntry::new(super::ROOT_NODE, offs[1], true, 0, 0, 0),
            super::TreeEntry::new(super::ROOT_NODE, offs[2], false, 100, 0, 0),
            super::TreeEntry::new(1, offs[3], false, 200, 0, 0),
        ];
        let vol = super::VolumeId::from_mount_point(PathBuf::from("/root"));
        let restored = super::SizeTree::from_packed(vol, pool, entries);

        assert_eq!(restored.size_of(restored.root()), 300);
        assert_eq!(restored.file_count_of(restored.root()), 2);
        let proj = restored
            .find_node_by_path(&PathBuf::from("/root/proj"))
            .unwrap();
        assert_eq!(restored.entry_name(proj), "proj");
        assert_eq!(restored.size_of(proj), 200);
    }

    #[cfg(not(windows))]
    #[test]
    fn search_finds_nested_and_is_case_insensitive() {
        let tree = build_test_tree();
        let hits = tree.search("B.TXT", 16);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].path.ends_with("b.txt"));
        let hits = tree.search("proj", 16);
        assert!(hits.iter().any(|h| h.name == "proj" && h.is_dir));
    }

    #[cfg(not(windows))]
    #[test]
    fn search_on_synthetic_tree_is_repeatable() {
        let root = PathBuf::from("/root");
        let mut snap = vec![TreeSnapshotEntry {
            path: root.clone(),
            is_dir: true,
            size: 0,
            mtime: 0,
        }];
        for i in 0..2000 {
            snap.push(TreeSnapshotEntry {
                path: root.join(format!("file_{i}.txt")),
                is_dir: false,
                size: 100 + i as u64,
                mtime: 0,
            });
        }
        let vol = super::VolumeId::from_mount_point(root);
        let tree = SizeTree::from_snapshot(vol, snap);
        let first = tree.search("file_1234.txt", 8);
        let second = tree.search("file_1234.txt", 8);
        assert_eq!(first.len(), 1);
        assert_eq!(first.len(), second.len());
        assert_eq!(first[0].name, second[0].name);
        assert_eq!(tree.search("file_99999.txt", 8).len(), 0);
    }
}
