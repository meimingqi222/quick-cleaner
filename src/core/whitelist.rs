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
//! [`reload`] 时展开一次。全局表同时保存归一化形态（比对用）和原文
//! （写回 Settings 用）——归一化会折叠大小写，若只存归一化形态，写回
//! 时就只能展示 `/users/...` 这种被压小写的路径。旧版本写下的已压小写
//! 条目按原文照常生效，用户重新排除一次即可恢复正常大小写。

use std::path::Path;
use std::sync::RwLock;

/// 一条白名单条目的两种形态。
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    /// 归一化形态，只用于比对。
    norm: String,
    /// 展开 `~` 后的原文（保留大小写与原生分隔符），写回 Settings 时用。
    display: String,
}

/// 全局白名单表。空表 = 没有用户白名单（行为与该模块不存在时完全一致）。
static ENTRIES: RwLock<Vec<Entry>> = RwLock::new(Vec::new());

/// 把 Settings 里的原始字符串清单装载进全局表。启动时与每次修改后调用。
///
/// `~` 前缀展开成用户主目录；展开不了（极罕见的主目录缺失）就丢弃该条
/// ——一条坏条目不该让整张表静默失明，但也没法凭空修。返回 false 表示
/// 有条目被丢弃或装载失败，调用方应记日志而不是假装无事发生。
pub fn reload(raw: &[String]) -> bool {
    let mut entries = Vec::with_capacity(raw.len());
    let mut dropped = 0usize;
    for item in raw {
        let Some(expanded) = expand_home(item) else {
            dropped += 1;
            continue;
        };
        let norm = crate::core::safety::norm(Path::new(&expanded));
        entries.push(Entry {
            norm,
            display: expanded,
        });
    }
    match ENTRIES.write() {
        Ok(mut guard) => {
            *guard = entries;
            dropped == 0
        }
        // poisoned 只发生在持锁线程 panic 时；这里没有可能 panic 的代码，
        // 但仍按「装载失败」如实上报，不假装成功。
        Err(_) => false,
    }
}

/// 加入一条白名单（绝对路径），全局表立即生效。返回更新后的完整清单
/// （展开后的绝对路径原文），调用方拿它写回 `Settings` 并 `save()`。
///
/// 已存在的等价条目（归一化后相同）不重复添加。写锁拿不到（表已中毒）
/// 时返回 None——调用方必须原样保留 `Settings.whitelist`，绝不能把一个
/// 可能是空的清单写回磁盘覆盖用户的保护表。
pub fn add(path: &Path) -> Option<Vec<String>> {
    let expanded = expand_home(&path.to_string_lossy())?;
    let normalized = crate::core::safety::norm(Path::new(&expanded));
    {
        let mut guard = ENTRIES.write().ok()?;
        if !guard.iter().any(|e| e.norm == normalized) {
            guard.push(Entry {
                norm: normalized,
                display: expanded,
            });
        }
    }
    current()
}

/// 移除一条白名单（归一化后精确匹配），全局表立即生效。返回更新后的
/// 完整清单；语义同 [`add`]——写锁拿不到时返回 None，调用方不要动
/// `Settings`。
pub fn remove(path: &Path) -> Option<Vec<String>> {
    let expanded = expand_home(&path.to_string_lossy())?;
    let normalized = crate::core::safety::norm(Path::new(&expanded));
    {
        let mut guard = ENTRIES.write().ok()?;
        guard.retain(|e| e.norm != normalized);
    }
    current()
}

/// 当前清单（展开后的绝对路径原文，可直接序列化进 Settings）。
///
/// 表中毒时返回 None：此时「清单是什么」是未知的，调用方拿到的任何
/// 替代值（包括空表）写回磁盘都可能销毁用户的保护表。
pub fn current() -> Option<Vec<String>> {
    let guard = ENTRIES.read().ok()?;
    Some(guard.iter().map(|e| e.display.clone()).collect())
}

/// 这个路径（或它的任何子路径）是否被用户拉进了白名单。
///
/// 条目自身和它的整个子树都受保护——用户排除的是「这个东西」，不管
/// 清理器从哪个祖先路径递归进来。
///
/// 保护表必须 fail-closed：锁中毒时查不到内容，返回 true（把路径当作
/// 受保护）最多损失一次清理，返回 false 则可能误删用户明令保护的东西。
pub fn is_whitelisted(path: &Path) -> bool {
    let lower = crate::core::safety::norm(path);
    match ENTRIES.read() {
        Ok(guard) => guard
            .iter()
            .any(|e| crate::core::safety::at_or_under(&lower, &e.norm)),
        Err(_) => true,
    }
}

/// 这个路径下面是否压着至少一条白名单条目（即它是某条目的祖先）。
///
/// 供扫描侧把这类父目录从默认勾选里降级：整树清理会连带碰被保护子项，
/// 删除层虽然会拦，但与其让用户看到「失败的清理」，不如默认不勾。
/// 与 [`is_whitelisted`] 同理 fail-closed：查不了就当作压着条目，宁可
/// 不默认勾选。
pub fn has_entry_under(path: &Path) -> bool {
    let lower = crate::core::safety::norm(path);
    match ENTRIES.read() {
        Ok(guard) => guard
            .iter()
            .any(|e| crate::core::safety::at_or_under(&e.norm, &lower)),
        Err(_) => true,
    }
}

/// 清空全局表。测试用——生产路径上白名单的增删走 [`add`] / [`remove`]。
///
/// 测试可能故意毒化锁，poisoned 的锁恢复不出 `Ok`，只能 `into_inner`
/// 硬取——测试进程里这是唯一能继续的方式。
#[cfg(test)]
pub(crate) fn clear() {
    let mut guard = match ENTRIES.write() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.clear();
}

/// 全局表是进程级单例，cargo test 默认并行跑——凡是用到本模块的测试
/// （本模块自身的，以及 cleaner/safety 里基于本模块的集成测试）都必须
/// 先锁这把锁，否则一个测试的 reload 会踩掉另一个刚装进去的条目，
/// 间歇性误报。
#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 取测试串行锁，**中毒也继续执行**。
///
/// 一个测试失败会把 `TEST_LOCK` 变成中毒态，之后每个 `.lock().unwrap()`
/// 都会以 `PoisonError` 崩掉——于是 1 个真实失败被放大成 9 个失败，而且
/// 报告里最显眼的那条（`PoisonError`）指向的测试其实完全无辜。串行锁的
/// 中毒状态不携带任何需要保住的不变量，接管内部值继续跑才是对的。
#[cfg(test)]
pub(crate) fn lock_for_test() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// `~` / `~/…` 展开为用户主目录；其余原样返回。展开不了（主目录缺失）
/// 返回 None——调用方丢弃该条，绝不能把字面 `~/…` 当成有效路径存进去，
/// 那会变成一条永不相配的哑条目。
fn expand_home(s: &str) -> Option<String> {
    if s == "~" {
        return crate::platform::user_home().map(|h| h.to_string_lossy().into_owned());
    }
    if let Some(rest) = s.strip_prefix("~/") {
        let home = crate::platform::user_home()?;
        return Some(home.join(rest).to_string_lossy().into_owned());
    }
    Some(s.to_string())
}

/// 用真实文件系统路径验证 add/reload 的往返一致性。
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn reload_then_is_whitelisted_covers_self_and_descendants() {
        let _guard = lock_for_test();
        clear();
        let base = crate::core::testing::fixture("qc_wl_roundtrip");
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
        let _guard = lock_for_test();
        clear();
        let base = crate::core::testing::fixture("qc_wl_ancestor");
        let target = base.join("keep");
        assert!(reload(&[target.to_string_lossy().into_owned()]));

        // 父目录是条目的祖先 → 要降级默认勾选
        assert!(has_entry_under(&base));
        assert!(has_entry_under(&target));
        // 条目自己和无关路径不是「压着条目」
        assert!(!has_entry_under(&target.join("child")));
        assert!(!has_entry_under(&crate::core::testing::fixture(
            "qc_wl_unrelated"
        )));
        clear();
    }

    #[test]
    fn add_deduplicates_equivalent_entries() {
        let _guard = lock_for_test();
        clear();
        let p = crate::core::testing::fixture("qc_wl_dedup");
        let _ = add(&p);
        // 同一路径的不同写法（尾斜杠）归一化后相同，不该出现两条
        let with_slash = PathBuf::from(format!("{}/", p.display()));
        let _ = add(&with_slash);
        assert_eq!(current().expect("锁未中毒").len(), 1);
        clear();
    }

    #[test]
    fn add_preserves_original_case_and_roundtrips() {
        let _guard = lock_for_test();
        clear();
        // 归一化会折叠大小写，但写回 Settings 的必须是原文——旧实现会把
        // 这里压成全小写，用户在配置文件里看到的路径面目全非。
        let p = PathBuf::from(
            std::env::temp_dir()
                .join("Qc_Wl_Case/MixedPath")
                .to_string_lossy()
                .into_owned(),
        );
        let list = add(&p).expect("add 应成功");
        assert_eq!(list, vec![p.to_string_lossy().into_owned()]);
        assert_eq!(current().expect("锁未中毒"), list);
        // 比对不受大小写影响
        assert!(is_whitelisted(&p));
        clear();
    }

    #[test]
    fn remove_deletes_only_exact_entry() {
        let _guard = lock_for_test();
        clear();
        let parent = crate::core::testing::fixture("qc_wl_remove");
        let child = parent.join("keep");
        let _ = add(&parent);
        let _ = add(&child);

        let list = remove(&parent).expect("remove 应成功");
        assert_eq!(list, vec![child.to_string_lossy().into_owned()]);
        assert!(!is_whitelisted(&parent));
        // 子条目不连带移除：用户只点名撤掉一条
        assert!(is_whitelisted(&child));
        // 移除不存在的条目也返回当前完整清单
        assert_eq!(
            remove(&parent).expect("remove 应成功"),
            vec![child.to_string_lossy().into_owned()]
        );
        clear();
    }

    #[test]
    fn reload_reports_success_and_plain_paths_roundtrip() {
        let _guard = lock_for_test();
        clear();
        let target = crate::core::testing::fixture("qc_wl_reload_ok");
        assert!(reload(&[target.to_string_lossy().into_owned()]));
        assert!(is_whitelisted(&target));
        assert_eq!(
            current().expect("锁未中毒"),
            vec![target.to_string_lossy().into_owned()]
        );
        clear();
    }

    #[test]
    fn home_tilde_prefix_expands() {
        let _guard = lock_for_test();
        clear();
        let home = dirs::home_dir().expect("测试环境必须有主目录");
        let raw = "~/qc_wl_tilde_probe".to_string();
        reload(&[raw]);

        assert!(is_whitelisted(&home.join("qc_wl_tilde_probe")));
        clear();
    }
}
