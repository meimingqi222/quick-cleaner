//! 清理目标的占用检测：正在运行的应用 + 被进程打开的文件。
//!
//! 目标是给每个 `ScanItem` 两种徽标（对标商业清理工具的「应用打开中 /
//! 系统占用」），并让清理入口跳过这些目标——macOS 允许删除正被打开的文件，
//! 不拦的话清"成功"了，应用却在写一个已消失的路径。
//!
//! 性能边界：整轮检测只有两次子进程调用（`lsof -F0n` 一次列全——本机
//! 实测约 15 秒、2 万条打开路径，`ps -axo comm=` 毫秒级），加上
//! O(打开文件数 × 路径深度) 的哈希查表。检测在后台线程与扫描并发跑，
//! 且结果**不阻塞**首屏、也不推迟第二阶段发现式扫描——列表先出，徽标
//! 在检测完成后合并进条目（见 `ui::actions::junk` 的扫描任务编排）。

use crate::core::i18n::{bilingual, Text};
use crate::core::scanner::CategorySummary;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 一个目标的占用状态。
///
/// `app` 是按目录名推出来的归属应用（正在运行）；`open` 表示有任意进程
/// 打开了目标子树内的路径（含目录句柄、cwd）。两者可同时成立，徽标优先
/// 展示 `app`——它对用户更有解释力。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Busy {
    pub app: Option<String>,
    pub open: bool,
}

impl Busy {
    fn is_empty(&self) -> bool {
        self.app.is_none() && !self.open
    }

    /// 徽标文案：`Some((文案, 是否应用级))`。应用级徽标是提示（可强勾），
    /// 纯 `open` 徽标偏阻断语义，UI 据此选配色。
    pub fn badge(&self) -> Option<(Text, bool)> {
        if let Some(app) = &self.app {
            return Some((
                bilingual(|l| match l {
                    crate::core::i18n::Language::Zh => format!("应用打开中 · {app}"),
                    crate::core::i18n::Language::En => format!("{app} running"),
                }),
                true,
            ));
        }
        if self.open {
            return Some((
                bilingual(|l| match l {
                    crate::core::i18n::Language::Zh => String::from("系统占用"),
                    crate::core::i18n::Language::En => String::from("In use"),
                }),
                false,
            ));
        }
        None
    }
}

/// 对全部固定目标跑一轮占用检测。只在 macOS 有实现，其余平台返回空表
/// （徽标不显示，清理也不跳过——行为与没有这个模块时完全一致）。
pub fn detect(targets: &[PathBuf]) -> HashMap<PathBuf, Busy> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = targets;
        HashMap::new()
    }
    #[cfg(target_os = "macos")]
    detect_macos(targets)
}

/// 把检测结果合入扫描条目：写 `busy`，并给被占用的条目降级 `recommended`。
///
/// 返回受影响的条目数。降级而不是在勾选层过滤，是为了让
/// `selection_is_recommended` 这类「勾选态是否等于推荐态」的比对天然一致；
/// 调用方（UI）负责在需要时重新执行 `select_recommended`。
pub fn apply_busy(categories: &mut [CategorySummary], busy: &HashMap<PathBuf, Busy>) -> usize {
    let mut n = 0;
    for cat in categories {
        for item in &mut cat.items {
            if let Some(b) = busy.get(&item.path) {
                if !b.is_empty() {
                    item.busy = Some(b.clone());
                    item.recommended = false;
                    n += 1;
                }
            }
        }
    }
    n
}

#[cfg(target_os = "macos")]
fn detect_macos(targets: &[PathBuf]) -> HashMap<PathBuf, Busy> {
    // lsof 报的是内核解析后的真实路径（/var/folders vs /private/var/
    // folders），目标表里的路径必须先 canonicalize 一份，否则 per-user
    // 临时目录一族永远匹配不上。映射的值仍是目标表里的原始路径——结果
    // 表的键必须与 ScanItem.path 对齐。canonicalize 失败（悬空/权限）就
    // 只留原路径自身。
    let mut want: HashMap<PathBuf, PathBuf> = HashMap::new();
    for t in targets {
        match t.canonicalize() {
            Ok(c) => {
                want.insert(c, t.clone());
            }
            Err(_) => {
                want.insert(t.clone(), t.clone());
            }
        }
    }

    let mut result: HashMap<PathBuf, Busy> = HashMap::new();
    let procs = running_process_paths();
    for path in targets {
        if let Some(app) = owning_app(path, &procs) {
            result.entry(path.clone()).or_default().app = Some(app);
        }
    }

    for open in open_file_paths() {
        // 从打开的路径沿父链向上找目标：打开的是目标内部的文件时，目标
        // 本身就是它的某个祖先。深度 ≈ 路径层级，单次查表 O(1)。
        let mut cur = open.to_path_buf();
        loop {
            if let Some(raw) = want.get(&cur) {
                result.entry(raw.clone()).or_default().open = true;
                break;
            }
            if !cur.pop() {
                break;
            }
        }
    }
    result
}

/// 当前全部进程的可执行文件路径（小写）。一次 `ps` 调用，毫秒级。
#[cfg(target_os = "macos")]
fn running_process_paths() -> Vec<String> {
    let Ok(out) = std::process::Command::new("/bin/ps")
        .args(["-axo", "comm="])
        .output()
    else {
        return Vec::new();
    };
    out.stdout
        .split(|&b| b == b'\n')
        .filter_map(|line| std::str::from_utf8(line).ok())
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 从目录名推归属应用：`/<stem>.app/` 形态的进程路径才算命中。
///
/// 候选词由目录名逐级剥壳得到：`@zcodedesktop-updater` → `zcodedesktop`，
/// `notion.id.ShipIt` → `notion.id` → 再按 `.` 取首段 → `notion`，
/// `com.google.antigravity` → 末段 → `antigravity`。命中哪个候选词，就用
/// 它的原文（非小写）做展示名。推不出候选词的目录（go-build、Logs 之类）
/// 自然没有候选，永不误报。
#[cfg(target_os = "macos")]
fn owning_app(target: &Path, procs_lower: &[String]) -> Option<String> {
    let name = target.file_name()?.to_string_lossy().into_owned();
    let stem = name
        .strip_suffix("-updater")
        .or_else(|| name.strip_suffix(".ShipIt"))
        .unwrap_or(&name)
        .trim_start_matches('@')
        .to_owned();
    if stem.is_empty() {
        return None;
    }

    let mut candidates: Vec<(String, String)> = vec![(stem.to_lowercase(), stem.clone())];
    if let Some((first, _)) = stem.split_once('.') {
        candidates.push((first.to_lowercase(), first.to_owned()));
    }
    if let Some((_, last)) = stem.rsplit_once('.') {
        candidates.push((last.to_lowercase(), last.to_owned()));
    }
    candidates.sort_by_key(|(low, _)| std::cmp::Reverse(low.len()));
    candidates.dedup_by(|a, b| a.0 == b.0);

    // 最长候选词优先：`notion.id` 整体撞不上时才轮到 `notion`，避免短词
    // 抢先把展示名截短。
    candidates
        .iter()
        .find(|(low, _)| {
            procs_lower
                .iter()
                .any(|p| p.contains(&format!("/{low}.app/")))
        })
        .map(|(_, display)| display.clone())
}

/// 全部进程当前打开的文件路径。一次 `lsof` 调用，本机实测约 15 秒——
/// 全程被首屏渲染与第二阶段发现式扫描掩盖，不占用用户等待时间。
#[cfg(target_os = "macos")]
fn open_file_paths() -> Vec<PathBuf> {
    let Ok(out) = std::process::Command::new("/usr/sbin/lsof")
        .args(["-F0n", "-w"])
        .output()
    else {
        return Vec::new();
    };
    extract_lsof_paths(&out.stdout, std::process::id())
}

/// 解析 `lsof -F0n` 输出：字段以字段字符开头、`\0` 分隔，进程组以 `p<pid>`
/// 起头，只关心其中的 `n`（路径）；非绝对路径（socket 别名、内核内部名）丢弃。
///
/// **必须跳过 `self_pid` 那一组**：占用检测与扫描是并发跑的，本进程正在称重的
/// 目录会以 `DIR` 句柄的形式出现在输出里，父链回溯会把**我们自己正在扫的那个
/// 目标**标成占用并取消预选。实测一个正在遍历 `~` 的 `find` 进程，lsof 报了
/// 8 条绝对路径，其中一条就是 `~/go/pkg/mod/...`——那是本工具自己的
/// PackageCache 目标。不排除的后果是「越大的缓存越容易被自己误判成占用」，
/// 表现为同一条目这次预选、下次不预选。
#[cfg(target_os = "macos")]
fn extract_lsof_paths(bytes: &[u8], self_pid: u32) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut skipping = false;
    for field in bytes.split(|&b| b == 0) {
        match field.first() {
            // 进程组边界：之后每个 `n` 都属于这个 pid，直到下一个 `p`
            Some(&b'p') => {
                skipping = std::str::from_utf8(&field[1..])
                    .ok()
                    .and_then(|s| s.trim().parse::<u32>().ok())
                    == Some(self_pid);
            }
            Some(&b'n') if !skipping => {
                if let Some(path) = std::str::from_utf8(&field[1..])
                    .ok()
                    .filter(|s| s.starts_with('/'))
                {
                    out.push(PathBuf::from(path));
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn parses_lsof_field_output() {
        let raw =
            b"p123\0cnode\0n/Users/x/a.txt\0p456\0nsocket\0n/private/var/folders/x/T\0nrelative\0\0";
        // 谁都不是自己：两组都收，非绝对路径丢掉
        assert_eq!(
            extract_lsof_paths(raw, 1),
            vec![
                PathBuf::from("/Users/x/a.txt"),
                PathBuf::from("/private/var/folders/x/T"),
            ]
        );
        // 456 是本进程：那一组的路径整组丢掉，包括 cwd 与 DIR 句柄
        assert_eq!(extract_lsof_paths(raw, 456), vec![PathBuf::from("/Users/x/a.txt")]);
        // pid 解析不出来（异常输出）时按「不是自己」处理，宁可多收不误漏
        assert_eq!(extract_lsof_paths(b"p??\0n/Users/x/b.txt\0", 1).len(), 1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn open_file_marks_target_via_ancestors() {
        let dir = std::env::temp_dir().join(format!("qc_inuse_{}", std::process::id()));
        let inner = dir.join("sub/deep/file.bin");
        std::fs::create_dir_all(inner.parent().unwrap()).unwrap();
        std::fs::write(&inner, b"x").unwrap();

        // 模拟 detect_macos 的 want 映射与父链回溯。lsof 报告的是内核
        // canonical 路径，所以回溯也从 canonical 形态出发。
        let canonical = dir.canonicalize().unwrap();
        let mut want: HashMap<PathBuf, PathBuf> = HashMap::new();
        want.insert(canonical.clone(), dir.clone());
        let mut result: HashMap<PathBuf, Busy> = HashMap::new();
        for open in [canonical.join("sub/deep/file.bin"), canonical.join("sub")] {
            let mut cur = open;
            loop {
                if let Some(raw) = want.get(&cur) {
                    result.entry(raw.clone()).or_default().open = true;
                    break;
                }
                if !cur.pop() {
                    break;
                }
            }
        }
        assert!(result.get(&dir).unwrap().open);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn owning_app_matches_by_dir_name_stem() {
        let procs = vec![
            "/applications/microsoft edge.app/contents/macos/microsoft edge".into(),
            "/applications/notion.app/contents/macos/notion".into(),
            "/applications/antigravity.app/contents/macos/antigravity".into(),
            "/system/applications/safari.app/contents/macos/safari".into(),
        ];
        let cases = [
            (
                "/Users/x/Library/Caches/Microsoft Edge",
                Some("Microsoft Edge"),
            ),
            ("/Users/x/Library/Caches/notion-updater", Some("notion")),
            ("/Users/x/Library/Caches/notion.id.ShipIt", Some("notion")),
            (
                "/Users/x/Library/Caches/com.google.antigravity",
                Some("antigravity"),
            ),
            ("/Users/x/Library/Caches/com.apple.Safari", Some("Safari")),
            ("/Users/x/Library/Caches/termius-updater", None),
            ("/Users/x/Library/Caches/go-build", None),
            ("/Users/x/Library/Logs", None),
        ];
        for (path, want) in cases {
            assert_eq!(
                owning_app(Path::new(path), &procs).as_deref(),
                want,
                "{path}"
            );
        }
    }

    #[test]
    fn badge_prefers_app_over_open() {
        let b = Busy {
            app: Some("Edge".into()),
            open: true,
        };
        let (text, app_level) = b.badge().unwrap();
        assert!(text.get(crate::core::i18n::Language::Zh).contains("Edge"));
        assert!(app_level);

        let b = Busy {
            app: None,
            open: true,
        };
        let (_, app_level) = b.badge().unwrap();
        assert!(!app_level);

        assert!(Busy::default().badge().is_none());
    }

    #[test]
    fn apply_busy_downgrades_recommended() {
        use crate::core::categories::CategoryId;
        use crate::core::i18n::Text;
        use crate::core::scanner::{CategorySummary, ScanItem};
        use std::path::PathBuf;

        let item = |p: &str| ScanItem {
            path: PathBuf::from(p),
            label: Text::same("x"),
            size: 1,
            file_count: 0,
            category: CategoryId::UserTemp,
            last_modified: 0,
            recommended: true,
            busy: None,
        };
        let mut cats = vec![CategorySummary {
            category: CategoryId::UserTemp,
            total_size: 2,
            items: vec![item("/a"), item("/b")],
        }];
        let mut busy: HashMap<PathBuf, Busy> = HashMap::new();
        busy.insert(
            PathBuf::from("/a"),
            Busy {
                app: Some("A".into()),
                open: false,
            },
        );
        // 空的 Busy 不算占用，不该碰条目
        busy.insert(PathBuf::from("/c"), Busy::default());

        assert_eq!(apply_busy(&mut cats, &busy), 1);
        assert!(cats[0].items[0].busy.is_some());
        assert!(!cats[0].items[0].recommended);
        assert!(cats[0].items[1].busy.is_none());
        assert!(cats[0].items[1].recommended);
    }
}
