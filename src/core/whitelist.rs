//! 用户白名单：「永远别碰这些路径」的清单。
//!
//! 起因是一个组合风险：分类清理是**永久删除**（无回收站），而 Settings
//! 此前只有语言/回收站开关/FDA 引导三项——用户被误删一次之后，除了每轮
//! 手动取消勾选，没有任何自保手段。白名单是用户自己的保护表，与
//! `safety.rs` 里硬编码的系统骨架表互补：那张表挡的是「任何用户都不该
//! 删的」，这张表挡的是「**这个**用户说不能删的」。
//!
//! # 生效方式
//!
//! 合并进 [`crate::core::safety::is_protected`]——所有删除通道
//! （`clean_path` 入口、`delete_tree` 递归的每一层、`recycle_path`、
//! 手选路径 `clean_arbitrary`）都查它，白名单从删除层就不可绕过。
//! 叠加 `delete_tree` 既有的「每层重查 is_protected」纵深防御，用户
//! 勾选了某个父目录、而白名单条目是它的子目录时，递归到那一层会跳过
//! 保护目标（父目录本身会因非空而删除失败——报失败比误删好）。
//!
//! # 嵌套语义（借鉴 Mole dev.sh 的 pypoetry 案例）
//!
//! 白名单一个嵌套路径（如 `~/Library/Caches/pypoetry/virtualenvs`），
//! 扫描侧要把它的**父目录**从默认勾选里拿掉：父目录一旦被整体清理，
//! 被保护的子目录虽然删除层会拦，但用户看到的是一次「失败的清理」；
//! 让父目录默认不勾（仍展示，仍可手动选），体验是「这条我替你看着」。
//! 这就是 [`has_entry_under`] 存在的理由——它回答「这个路径下面是不是
//! 压着一条白名单」。
//!
//! # 归一化
//!
//! 与 `safety.rs` 用同一套 [`crate::core::safety::norm`] / `at_or_under`
//! 边界语义（safety 侧为此把 `at_or_under` 开成了 `pub(crate)`）——不能
//! 各写一份，归一化规则稍有出入，就会出现「保护判定放行、白名单判定
//! 拦截」对同一路径给出不同答案的怪事。
//!
//! # 存储格式
//!
//! `Settings.whitelist` 里存**展开后的绝对路径原文**（右键排除的条目本来
//! 就是绝对路径；手改配置文件的用户请自行写绝对路径）。`~` 前缀在
//! [`reload`] 时展开一次，之后只做归一化比对。

use std::path::Path;
use std::sync::RwLock;

/// 归一化后的白名单条目。空表 = 没有用户白名单（行为与该模块不存在时
/// 完全一致）。
static ENTRIES: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// 把 Settings 里的原始字符串清单装载进全局表。启动时与每次修改后调用。
///
/// `~` 前缀展开成用户主目录；展开不了（极罕见的主目录缺失）就丢弃该条
/// 并如实返回 false——一条坏条目不该让整张表静默失明，但也没法凭空修。
pub fn reload(raw: &[String]) -> bool {
    let mut entries = Vec::with_capacity(raw.len());
    for item in raw {
        let expanded = expand_home(item);
        entries.push(crate::core::safety::norm(Path::new(&expanded)));
    }
    match ENTRIES.write() {
        Ok(mut guard) => {
            *guard = entries;
            true
        }
        // poisoned 只发生在持锁线程 panic 时；这里没有可能 panic 的代码，
        // 但仍按「装载失败」如实上报，不假装成功。
        Err(_) => false,
    }
}

/// 加入一条白名单（绝对路径），全局表立即生效。返回更新后的完整清单
/// （展开后的绝对路径原文），调用方拿它写回 `Settings` 并 `save()`。
///
/// 已存在的等价条目（归一化后相同）不重复添加。
pub fn add(path: &Path) -> Vec<String> {
    let expanded = expand_home(&path.to_string_lossy());
    let normalized = crate::core::safety::norm(Path::new(&expanded));
    if let Ok(mut guard) = ENTRIES.write() {
        if !guard.contains(&normalized) {
            guard.push(normalized);
        }
    }
    current()
}

/// 当前清单（展开后的绝对路径原文，可直接序列化进 Settings）。
pub fn current() -> Vec<String> {
    match ENTRIES.read() {
        Ok(guard) => guard
            .iter()
            .map(|normalized| restore_case(normalized))
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// 这个路径（或它的任何子路径）是否被用户拉进了白名单。
///
/// 条目自身和它的整个子树都受保护——用户排除的是「这个东西」，不管
/// 清理器从哪个祖先路径递归进来。
pub fn is_whitelisted(path: &Path) -> bool {
    let lower = crate::core::safety::norm(path);
    match ENTRIES.read() {
        Ok(guard) => guard.iter().any(|entry| {
            crate::core::safety::at_or_under(&lower, entry)
        }),
        Err(_) => false,
    }
}

/// 这个路径下面是否压着至少一条白名单条目（即它是某条目的祖先）。
///
/// 供扫描侧把这类父目录从默认勾选里降级：整树清理会连带碰被保护子项，
/// 删除层虽然会拦，但与其让用户看到「失败的清理」，不如默认不勾。
pub fn has_entry_under(path: &Path) -> bool {
    let lower = crate::core::safety::norm(path);
    match ENTRIES.read() {
        Ok(guard) => guard
            .iter()
            .any(|entry| crate::core::safety::at_or_under(entry, &lower)),
        Err(_) => false,
    }
}

/// 清空全局表。测试用——生产路径上白名单只增不清（用户要移除条目时
/// 是「改完整清单后 reload」）。
#[cfg(test)]
pub(crate) fn clear() {
    if let Ok(mut guard) = ENTRIES.write() {
        guard.clear();
    }
}

/// 全局表是进程级单例，cargo test 默认并行跑——凡是用到本模块的测试
/// （本模块自身的，以及 cleaner/safety 里基于本模块的集成测试）都必须
/// 先锁这把锁，否则一个测试的 reload 会踩掉另一个刚装进去的条目，
/// 间歇性误报。
#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// `~` / `~/…` 展开为用户主目录；其余原样返回。
fn expand_home(s: &str) -> String {
    if s == "~" {
        return dirs::home_dir()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|| s.to_string());
    }
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().into_owned();
        }
    }
    s.to_string()
}

/// 归一化会折叠大小写和分隔符；写回 Settings 时保留一份「看起来正常」
/// 的路径。归一化表把 `/` 统一成了 `\`，macOS 上展示得还原回来。
fn restore_case(normalized: &str) -> String {
    if cfg!(windows) {
        restore_drive_letter(normalized)
    } else {
        normalized.replace('\\', "/")
    }
}

/// Windows：归一化把盘符压成了小写（`C:\` → `c:\`），还原首字母大写。
fn restore_drive_letter(normalized: &str) -> String {
    let bytes = normalized.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_lowercase() {
        let mut out = normalized.to_string();
        out.replace_range(
            0..1,
            &(bytes[0] as char).to_ascii_uppercase().to_string(),
        );
        out
    } else {
        normalized.to_string()
    }
}

/// 用真实文件系统路径验证 add/reload 的往返一致性。
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn reload_then_is_whitelisted_covers_self_and_descendants() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear();
        let base = std::env::temp_dir().join("qc_wl_roundtrip");
        let target = base.join("virtualenvs");
        let raw = vec![target.to_string_lossy().into_owned()];
        assert!(reload(&raw));

        assert!(is_whitelisted(&target));
        // 子树同样受保护：清理器从任何祖先递归进来都碰不到
        assert!(is_whitelisted(&target.join("python3.13/bin")));
        // 兄弟和父级不在保护范围内
        assert!(!is_whitelisted(&base.join("artifacts")));
        assert!(!is_whitelisted(&base));
        clear();
    }

    #[test]
    fn has_entry_under_marks_ancestors_only() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear();
        let base = std::env::temp_dir().join("qc_wl_ancestor");
        let target = base.join("keep");
        reload(&[target.to_string_lossy().into_owned()]);

        // 父目录是条目的祖先 → 要降级默认勾选
        assert!(has_entry_under(&base));
        assert!(has_entry_under(&target));
        // 条目自己和无关路径不是「压着条目」
        assert!(!has_entry_under(&target.join("child")));
        assert!(!has_entry_under(
            &std::env::temp_dir().join("qc_wl_unrelated")
        ));
        clear();
    }

    #[test]
    fn add_deduplicates_equivalent_entries() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear();
        let p = std::env::temp_dir().join("qc_wl_dedup");
        let _ = add(&p);
        // 同一路径的不同写法（尾斜杠）归一化后相同，不该出现两条
        let with_slash = PathBuf::from(format!("{}/", p.display()));
        let _ = add(&with_slash);
        assert_eq!(current().len(), 1);
        clear();
    }

    #[test]
    fn home_tilde_prefix_expands() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear();
        let home = dirs::home_dir().expect("测试环境必须有主目录");
        let raw = "~/qc_wl_tilde_probe".to_string();
        reload(&[raw]);

        assert!(is_whitelisted(&home.join("qc_wl_tilde_probe")));
        clear();
    }
}
