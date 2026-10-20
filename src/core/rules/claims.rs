//! 统一认领表：从规则声明推导「哪些路径已由固定声明产出目标」。
//!
//! provider 枚举目录时靠它跳过同一位置，避免一个目标被 provider 和
//! 规则各计一次体积。认领来源只看对当前平台生效的规则：
//!
//! - `[[entries]]` 的字面路径（含变量模板的不认领）；
//! - `[[directories]]` 中 `Path` / `SignatureLeaves` 展开的每个声明路径；
//! - `LayoutChildren` 每行的 `row.path/child.path`。
//!
//! 其余 selection（Children、NamedDirectories、ManifestChildren 等）在
//! 声明路径**之下**发现目标，不认领声明路径本身——`roaming_agent_worktrees`
//! 若认领 `Maka/workspaces`，dev 对 Maka 的签名扫描就会被截到子目录，是错的。
//! `version_layouts` 不认领。
//!
//! 深度语义与旧实现一致：只区分枚举根下一段（`owns`）和两段
//! （`claimed_children`），更深的认领不单独表达。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::core::rules::directories::Selection;
use crate::core::rules::RuleSnapshot;

/// 规范化后的绝对路径认领集合。
pub struct Claims {
    owned: BTreeSet<String>,
    /// 规范化父路径 → 被认领的直接孩子名。
    children: BTreeMap<String, BTreeSet<String>>,
}

impl Claims {
    pub fn collect(
        snapshot: &std::sync::Arc<RuleSnapshot>,
        roots: &BTreeMap<String, Option<PathBuf>>,
    ) -> Self {
        let mut claims = Claims {
            owned: BTreeSet::new(),
            children: BTreeMap::new(),
        };
        let mut claim = |path: PathBuf| {
            claims.owned.insert(crate::core::safety::norm(&path));
            if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
                claims
                    .children
                    .entry(crate::core::safety::norm(parent))
                    .or_default()
                    .insert(name.to_string_lossy().into_owned());
            }
        };
        for rule in &snapshot.bundle.rules {
            if rule.platform != "all" && rule.platform != std::env::consts::OS {
                continue;
            }
            for entry in &rule.entries {
                // 含模板的条目不是固定路径，不认领。
                if entry.path.contains('{') {
                    continue;
                }
                if let Some(Some(root)) = roots.get(&entry.root) {
                    claim(root.join(&entry.path));
                }
            }
            for entry in &rule.directories {
                let Some(root) = super::directories::declared_root(entry.root, roots) else {
                    continue;
                };
                match &entry.select {
                    Selection::Path { .. } | Selection::SignatureLeaves { .. } => {
                        for path in &entry.paths {
                            if let Some(expanded) = super::directories::expand_paths(rule, path) {
                                for segment in expanded {
                                    claim(root.join(&segment));
                                }
                            }
                        }
                    }
                    Selection::LayoutChildren { catalog } => {
                        for path in &entry.paths {
                            let Some(expanded) = super::directories::expand_paths(rule, path)
                            else {
                                continue;
                            };
                            let rows = rule.catalogs.get(catalog);
                            for segment in &expanded {
                                if let Some(row) = rows
                                    .and_then(|rows| rows.iter().find(|row| row.path == *segment))
                                {
                                    for child in &row.children {
                                        claim(root.join(segment).join(&child.path));
                                    }
                                }
                            }
                        }
                    }
                    // 其余 selection 在声明路径之下发现目标，不认领声明路径——
                    // 例如 LinkedWorktrees 若认领 `Maka/workspaces`，dev 对 Maka
                    // 的签名扫描就会被截到子目录。逐个列出，新增变体时强制决定。
                    Selection::Children { .. }
                    | Selection::ContainerDirectories { .. }
                    | Selection::NamedFiles { .. }
                    | Selection::NamedDirectories { .. }
                    | Selection::Files { .. }
                    | Selection::ManifestChildren { .. }
                    | Selection::OrphanedChildren { .. }
                    | Selection::CatalogChildren { .. }
                    | Selection::LinkedWorktrees { .. } => {}
                }
            }
        }
        claims
    }

    /// `dir/name` 本身被某条声明认领（整目录跳过）。
    pub fn owns(&self, dir: &Path, name: &str) -> bool {
        self.owned
            .contains(&crate::core::safety::norm(&dir.join(name)))
    }

    /// `dir/name` 下被认领的直接孩子名（两段认领）；没有则空。
    pub fn claimed_children(&self, dir: &Path, name: &str) -> Vec<String> {
        self.children
            .get(&crate::core::safety::norm(&dir.join(name)))
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合成一条规则：声明各种来源的认领路径，验证 collect 的来源过滤
    /// 与深度语义。
    fn synthetic(platform: &str) -> std::sync::Arc<RuleSnapshot> {
        let rule: crate::core::rules::RuleDefinition = toml::from_str(&format!(
            r#"
id = "synthetic"
version = 1
platform = "{platform}"
required = []

[[entries]]
root = "home"
path = "Claimed/Dir"
zh = "x"
en = "x"
category = "UserCache"
operation = "contents"
disposal = "permanent"
recommended = true

[[entries]]
root = "home"
path = "{{var}}/Templated"
zh = "x"
en = "x"
category = "UserCache"
operation = "contents"
disposal = "permanent"
recommended = true

[[entries]]
root = "home"
path = "Deep/A/B/C"
zh = "x"
en = "x"
category = "UserCache"
operation = "contents"
disposal = "permanent"
recommended = true

[[catalogs.layout]]
path = "Agent"
zh = "a"
en = "a"
children = [{{path = "Cache", recommended = true}}]

[[catalogs.roots]]
path = "Browser"
zh = "b"
en = "b"

[[directories]]
id = "d1"
root = "home"
paths = ["{{roots}}"]
select = {{ kind = "path", directories_only = true }}
zh = "{{roots}}"
en = "{{roots}}"
category = "BrowserCache"
operation = "contents"
disposal = "permanent"
recommended = true

[[directories]]
id = "d2"
root = "home"
paths = ["{{roots}}"]
select = {{ kind = "signature_leaves", signature = "chromium" }}
zh = "{{roots}} · {{trail}}"
en = "{{roots}} · {{trail}}"
category = "BrowserCache"
operation = "contents"
disposal = "permanent"
recommended = true

[[directories]]
id = "d3"
root = "home"
paths = ["{{layout}}"]
select = {{ kind = "layout_children", catalog = "layout" }}
zh = "{{layout}} · {{name}}"
en = "{{layout}} · {{name}}"
category = "AiAgents"
operation = "contents"
disposal = "permanent"
recommended = true

[[directories]]
id = "d4"
root = "home"
paths = ["{{layout}}"]
select = {{ kind = "children", directories_only = true, prefixes = [] }}
zh = "{{layout}} · {{name}}"
en = "{{layout}} · {{name}}"
category = "AiAgents"
operation = "contents"
disposal = "permanent"
recommended = true
"#
        ))
        .unwrap();
        std::sync::Arc::new(RuleSnapshot {
            bundle: crate::core::rules::RuleBundle {
                schema: 13,
                sequence: 1,
                rules: vec![rule],
            },
        })
    }

    fn home_roots(home: &Path) -> BTreeMap<String, Option<PathBuf>> {
        BTreeMap::from([("home".to_string(), Some(home.to_path_buf()))])
    }

    #[test]
    fn literal_entries_and_emitting_selections_claim_their_paths() {
        let home = Path::new("/qc-claims");
        let claims = Claims::collect(&synthetic(std::env::consts::OS), &home_roots(home));

        // entries 字面路径：两段认领——父目录部分，孩子是 Chrome 式认领。
        assert!(!claims.owns(home, "Claimed"));
        assert_eq!(claims.claimed_children(home, "Claimed"), ["Dir"]);
        assert!(claims.owns(&home.join("Claimed"), "Dir"));

        // Path / SignatureLeaves 认领展开后的声明路径本身。
        assert!(claims.owns(home, "Browser"));

        // LayoutChildren 认领 row/child，不认领 row 本身。
        assert!(!claims.owns(home, "Agent"));
        assert_eq!(claims.claimed_children(home, "Agent"), ["Cache"]);
        assert!(claims.owns(&home.join("Agent"), "Cache"));
    }

    #[test]
    fn templates_non_emitting_selections_and_deep_paths_do_not_shallow_claim() {
        let home = Path::new("/qc-claims");
        let claims = Claims::collect(&synthetic(std::env::consts::OS), &home_roots(home));

        // 含变量模板的 entry 不认领。
        assert!(!claims.owns(home, "{var}"));
        assert!(claims.claimed_children(home, "{var}").is_empty());

        // Children selection 不认领声明路径本身（只在其下发现目标）。
        // 上一条测试已证 Agent 不被 owns，这里补一个负面对照：
        // Deep/A/B/C 的三段认领不产生 A 或 A/B 的浅层认领。
        assert!(!claims.owns(home, "Deep"));
        assert!(!claims.owns(&home.join("Deep"), "A"));
        assert!(claims.claimed_children(home, "Deep").is_empty());
        // 但按原始深度查两段仍然成立：Deep/A 的孩子 B 是父，B 的孩子 C 被认领。
        assert_eq!(
            claims.claimed_children(&home.join("Deep").join("A"), "B"),
            ["C"]
        );
    }

    #[test]
    fn other_platform_rules_and_missing_roots_never_claim() {
        let home = Path::new("/qc-claims");
        let other = if cfg!(windows) { "macos" } else { "windows" };
        let claims = Claims::collect(&synthetic(other), &home_roots(home));
        assert!(!claims.owns(home, "Browser"));
        assert!(claims.claimed_children(home, "Claimed").is_empty());

        // 根缺失：不认领，也不 panic。
        let claims = Claims::collect(
            &synthetic(std::env::consts::OS),
            &BTreeMap::from([("home".to_string(), None)]),
        );
        assert!(!claims.owns(home, "Browser"));
        let claims = Claims::collect(&synthetic(std::env::consts::OS), &BTreeMap::new());
        assert!(!claims.owns(home, "Browser"));
    }
}
