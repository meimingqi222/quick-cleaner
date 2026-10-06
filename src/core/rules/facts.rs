//! Bounded evidence evaluation. Unknown evidence never grants cleanup authority.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Evidence {
    Confirmed,
    Absent,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Condition {
    Fact { name: String },
    All { conditions: Vec<Condition> },
    Any { conditions: Vec<Condition> },
    Not { condition: Box<Condition> },
}
impl Condition {
    pub fn validate(&self, facts: &BTreeMap<String, Probe>) -> Result<(), String> {
        fn check(
            c: &Condition,
            facts: &BTreeMap<String, Probe>,
            depth: usize,
        ) -> Result<(), String> {
            if depth > 32 {
                return Err("Condition nesting limit".into());
            }
            match c {
                Condition::Fact { name } if !facts.contains_key(name) => Err("Unknown fact".into()),
                Condition::Fact { .. } => Ok(()),
                Condition::Not { condition } => check(condition, facts, depth + 1),
                Condition::All { conditions } | Condition::Any { conditions } => {
                    if conditions.is_empty() || conditions.len() > 256 {
                        return Err("Condition resource limit".into());
                    }
                    for c in conditions {
                        check(c, facts, depth + 1)?;
                    }
                    Ok(())
                }
            }
        }
        check(self, facts, 0)
    }
    pub fn evaluate(&self, facts: &BTreeMap<String, Evidence>) -> Evidence {
        self.evaluate_at(facts, 0)
    }
    fn evaluate_at(&self, facts: &BTreeMap<String, Evidence>, depth: usize) -> Evidence {
        if depth > 32 {
            return Evidence::Unknown;
        }
        match self {
            Self::Fact { name } => facts.get(name).copied().unwrap_or(Evidence::Unknown),
            Self::Not { condition } => match condition.evaluate_at(facts, depth + 1) {
                Evidence::Confirmed => Evidence::Absent,
                Evidence::Absent => Evidence::Confirmed,
                Evidence::Unknown => Evidence::Unknown,
            },
            Self::All { conditions } | Self::Any { conditions } => {
                if conditions.is_empty() || conditions.len() > 256 {
                    return Evidence::Unknown;
                }
                let all = matches!(self, Self::All { .. });
                let mut unknown = false;
                for c in conditions {
                    match c.evaluate_at(facts, depth + 1) {
                        Evidence::Absent if all => return Evidence::Absent,
                        Evidence::Confirmed if !all => return Evidence::Confirmed,
                        Evidence::Unknown => unknown = true,
                        _ => {}
                    }
                }
                if unknown {
                    Evidence::Unknown
                } else if all {
                    Evidence::Confirmed
                } else {
                    Evidence::Absent
                }
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Probe {
    Variable {
        name: String,
        equals: serde_json::Value,
    },
    File {
        path: String,
    },
    Text {
        path: String,
        signature: String,
    },
    Json {
        path: String,
        pointer: String,
        equals: serde_json::Value,
    },
}
impl Probe {
    pub fn path_template(&self) -> Option<&str> {
        match self {
            Self::Variable { .. } => None,
            Self::File { path } | Self::Text { path, .. } | Self::Json { path, .. } => Some(path),
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if let Self::Variable { name, equals } = self {
            return if name.is_empty()
                || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || serde_json::to_vec(equals).map_err(|e| e.to_string())?.len() > 65536
            {
                Err("Invalid variable fact".into())
            } else {
                Ok(())
            };
        }
        let path = match self {
            Self::File { path } | Self::Text { path, .. } | Self::Json { path, .. } => path,
            Self::Variable { .. } => unreachable!(),
        };
        if !super::relative(path) {
            return Err("Probe escapes evidence root".into());
        }
        match self {
            Self::Text { signature, .. } if signature.is_empty() || signature.len() > 65536 => {
                Err("Invalid signature".into())
            }
            Self::Json { pointer, .. } if !pointer.starts_with('/') || pointer.len() > 1024 => {
                Err("Invalid JSON pointer".into())
            }
            _ => Ok(()),
        }
    }
    pub fn evaluate(&self, root: &Path) -> Evidence {
        if matches!(self, Self::Variable { .. }) {
            return Evidence::Unknown;
        }
        if self.validate().is_err() {
            return Evidence::Unknown;
        }
        let path = match self {
            Self::File { path } | Self::Text { path, .. } | Self::Json { path, .. } => path,
            Self::Variable { .. } => return Evidence::Unknown,
        };
        if path.contains("${") {
            return Evidence::Unknown;
        }
        let target = root.join(path.replace('\\', "/"));
        match confined_file(root, &target) {
            Ok(false) => return Evidence::Absent,
            Err(_) => return Evidence::Unknown,
            Ok(true) => {}
        }
        if matches!(self, Self::File { .. }) {
            return Evidence::Confirmed;
        }
        let bytes = match read(&target) {
            Ok(b) => b,
            Err(_) => return Evidence::Unknown,
        };
        let matched = match self {
            Self::Text { signature, .. } => std::str::from_utf8(&bytes)
                .ok()
                .map(|s| s.contains(signature)),
            Self::Json {
                pointer, equals, ..
            } => serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .map(|v| v.pointer(pointer) == Some(equals)),
            Self::File { .. } => Some(true),
            Self::Variable { .. } => None,
        };
        match matched {
            Some(true) => Evidence::Confirmed,
            Some(false) => Evidence::Absent,
            None => Evidence::Unknown,
        }
    }

    pub fn evaluate_with(&self, root: &Path, values: &super::variables::Values) -> Evidence {
        if let Self::Variable { name, equals } = self {
            return match values.get(name) {
                Some(value) if value.evidence == Evidence::Confirmed => {
                    if value.value.as_ref() == Some(equals) {
                        Evidence::Confirmed
                    } else {
                        Evidence::Absent
                    }
                }
                Some(value) => value.evidence,
                None => Evidence::Unknown,
            };
        }
        let Some(template) = self.path_template() else {
            return Evidence::Unknown;
        };
        let Ok(paths) = super::variables::paths(template, values) else {
            return Evidence::Unknown;
        };
        if paths.len() != 1 {
            return Evidence::Unknown;
        }
        let path = paths[0].clone();
        let resolved = match self {
            Self::File { .. } => Self::File { path },
            Self::Text { signature, .. } => Self::Text {
                path,
                signature: signature.clone(),
            },
            Self::Json {
                pointer, equals, ..
            } => Self::Json {
                path,
                pointer: pointer.clone(),
                equals: equals.clone(),
            },
            Self::Variable { .. } => return Evidence::Unknown,
        };
        resolved.evaluate(root)
    }
}
pub fn confined_file(root: &Path, target: &Path) -> Result<bool, String> {
    confined(root, target, true)
}
pub fn confined_path(root: &Path, target: &Path) -> Result<bool, String> {
    confined(root, target, false)
}
fn confined(root: &Path, target: &Path, file_only: bool) -> Result<bool, String> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| "Evidence escapes root")?;
    if relative
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("Evidence escapes root".into());
    }
    // 根自身被换成链接：拒绝（夹具或安装目录都可能被整体替换）。
    let root_md = match std::fs::symlink_metadata(root) {
        Ok(md) if is_link(&md) => return Err("Redirected evidence root".into()),
        Ok(md) => md,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.to_string()),
    };
    // 祖先链只看**真实路径**。macOS 的系统临时目录位于 `/var → /private/var`
    // 之后，而 `env::temp_dir()` 给的是别名路径——按文本祖先判定会在那里看到
    // 链接，把整条发现链误判成 Unknown（DNS/QuickLook 与全部夹具都挂在这条
    // 路上）。canonicalize 解开系统别名后再逐跳复核：根以下任何一跳被换成
    // 链接仍然拒绝，安全性不变。
    let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    let components: Vec<std::path::Component> = relative.components().collect();
    if components.is_empty() {
        return Ok(root_md.is_file() || (!file_only && root_md.is_dir()));
    }
    let mut path = root;
    for (index, component) in components.iter().enumerate() {
        path.push(component);
        match std::fs::symlink_metadata(&path) {
            Ok(md) => {
                if is_link(&md) {
                    return Err("Redirected evidence".into());
                }
                if index + 1 == components.len() {
                    return Ok(md.is_file() || (!file_only && md.is_dir()));
                }
                if !md.is_dir() {
                    return Err("Invalid evidence ancestor".into());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(false)
}
pub(crate) fn is_link(md: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        md.file_attributes() & winapi::um::winnt::FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        md.file_type().is_symlink()
    }
}
pub(crate) fn read(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Evidence limit".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_negation_never_authorizes_cleanup() {
        let c = Condition::Not {
            condition: Box::new(Condition::Fact {
                name: "busy".into(),
            }),
        };
        assert_eq!(c.evaluate(&BTreeMap::new()), Evidence::Unknown);
        assert_eq!(
            Condition::All { conditions: vec![] }.evaluate(&BTreeMap::new()),
            Evidence::Unknown
        );
    }
    /// 根的**祖先**被安排成别名（macOS `/var → /private/var`、Windows 上把
    /// TEMP 重定向到别的盘的 junction）不应打断发现链；根自身是链接、或根以下
    /// 任何一跳是链接，仍然拒绝。三种形状都在同一夹具里断言。
    #[test]
    fn confined_path_tolerates_an_aliased_ancestor_but_rejects_redirected_links() {
        use crate::core::testing::fixture;
        let base = fixture("rules_confined_links");
        let real = base.join("real");
        let sub = real.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("child.txt"), b"x").unwrap();

        let link = base.join("alias");
        if !create_directory_link(&real, &link) {
            eprintln!("跳过：本机无法创建目录链接（RedirectionGuard 权限）");
            return;
        }

        // 祖先（link）是别名、根自身不是链接：必须照常判定。
        let aliased_root = link.join("sub");
        assert_eq!(
            confined_path(&aliased_root, &aliased_root.join("child.txt")),
            Ok(true),
            "祖先别名不该被当成重定向证据"
        );
        assert_eq!(
            confined_path(&aliased_root, &aliased_root.join("missing.txt")),
            Ok(false)
        );

        // 根自身是链接：仍然拒绝（安装目录被整体替换的防护）。
        assert_eq!(
            confined_path(&link, &link.join("child.txt")),
            Err("Redirected evidence root".into())
        );

        // 根以下的某一跳是链接：仍然拒绝。
        let inner = sub.join("inner");
        if create_directory_link(&real, &inner) {
            assert_eq!(
                confined_path(&aliased_root, &aliased_root.join("inner").join("child.txt")),
                Err("Redirected evidence".into()),
                "根以下的重定向必须继续拒绝"
            );
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 建一个指向目录的链接：Windows 用免特权的 junction，Unix 用 symlink。
    fn create_directory_link(target: &std::path::Path, link: &std::path::Path) -> bool {
        #[cfg(windows)]
        {
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .stdout(std::process::Stdio::null())
                .status();
            status.is_ok_and(|s| s.success())
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
    }

    #[test]
    fn evidence_is_bounded_and_confined() {
        let root = crate::core::testing::fixture("rules_evidence");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("manifest.json"), br#"{"owner":"fixture"}"#).unwrap();
        let probe = Probe::Json {
            path: "manifest.json".into(),
            pointer: "/owner".into(),
            equals: "fixture".into(),
        };
        assert_eq!(probe.evaluate(&root), Evidence::Confirmed);
        assert_eq!(
            Probe::File {
                path: "../manifest.json".into()
            }
            .evaluate(&root),
            Evidence::Unknown
        );
        assert_eq!(
            Probe::File {
                path: "absent".into()
            }
            .evaluate(&root),
            Evidence::Absent
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
