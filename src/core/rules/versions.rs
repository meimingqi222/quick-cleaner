//! Version layouts select fixed file capabilities and freeze pointer/lock facts.
use super::{
    directories::{Enumeration, ExpectedKind},
    facts::{self, Evidence},
    PathOperation, RuleDefinition, RuleObservation, RuleRef, RuleSnapshot,
};
use crate::core::{
    categories::{CategoryId, ScanTarget},
    cleaner::Disposal,
    i18n::Text,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionRoot {
    Home,
    Roaming,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionPlatform {
    #[default]
    All,
    Windows,
    Unix,
}
impl VersionPlatform {
    fn matches(self) -> bool {
        match self {
            Self::All => true,
            Self::Windows => cfg!(windows),
            Self::Unix => cfg!(unix),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionLayout {
    pub id: String,
    pub root: VersionRoot,
    #[serde(default)]
    pub platform: VersionPlatform,
    pub path: String,
    pub versions: String,
    pub current: Option<String>,
    pub lock: Option<LockPolicy>,
    pub leaf: Option<String>,
    pub prefixes: Vec<String>,
    pub exclude: Vec<String>,
    pub zh: String,
    pub en: String,
    pub category: String,
    pub operation: PathOperation,
    pub disposal: Disposal,
    pub recommended: bool,
    pub downloads: Option<DownloadPolicy>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockPolicy {
    pub path: String,
    pub maximum_age_seconds: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DownloadPolicy {
    pub path: String,
    pub suffixes: Vec<String>,
    pub zh: String,
    pub en: String,
    pub operation: PathOperation,
    pub disposal: Disposal,
    pub recommended: bool,
}
fn token(value: &str) -> bool {
    !value.trim().is_empty()
        && !matches!(value, "." | "..")
        && value.len() <= 128
        && !value.contains(['/', '\\', ':', '\0', '\r', '\n'])
}
fn text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 1024
        && !value
            .replace("{name}", "")
            .contains(['{', '}', '\0', '\r', '\n'])
}
pub(super) fn validate(rule: &RuleDefinition) -> Result<(), String> {
    if rule.version_layouts.len() > 32
        || (!rule.version_layouts.is_empty()
            && (!rule.variables.is_empty()
                || !["file", "version_retention"]
                    .iter()
                    .all(|capability| rule.required.iter().any(|value| value == capability))))
    {
        return Err("Undeclared or unbounded version layouts".into());
    }
    let mut ids = BTreeSet::new();
    for layout in &rule.version_layouts {
        if !token(&layout.id)
            || !ids.insert(&layout.id)
            || !super::relative(&layout.path)
            || (layout.versions != "." && !super::relative(&layout.versions))
            || layout
                .current
                .as_ref()
                .is_some_and(|path| !super::relative(path))
            || layout
                .leaf
                .as_ref()
                .is_some_and(|path| !super::relative(path))
            || layout.current.is_none()
                && (layout.leaf.is_none()
                    || layout.prefixes.is_empty()
                    || layout.downloads.is_some())
            || layout.current.is_some() && layout.leaf.is_some()
            || layout.prefixes.len() > 128
            || layout.exclude.len() > 128
            || layout
                .prefixes
                .iter()
                .chain(&layout.exclude)
                .any(|value| !token(value))
            || layout.lock.as_ref().is_some_and(|lock| {
                !super::relative(&lock.path) || !(60..=86400).contains(&lock.maximum_age_seconds)
            })
            || !text(&layout.zh)
            || !text(&layout.en)
            || CategoryId::from_rule(&layout.category).is_none()
            || layout.operation != PathOperation::Contents
            || layout.disposal != Disposal::Permanent
            || layout.downloads.as_ref().is_some_and(|download| {
                !super::relative(&download.path)
                    || download.suffixes.is_empty()
                    || download.suffixes.len() > 128
                    || download.suffixes.iter().any(|suffix| !token(suffix))
                    || !text(&download.zh)
                    || !text(&download.en)
                    || download.operation != PathOperation::File
                    || download.disposal != Disposal::Permanent
            })
        {
            return Err(format!("Invalid version layout: {}", layout.id));
        }
    }
    Ok(())
}
fn join(root: &Path, relative: &str) -> PathBuf {
    if relative == "." {
        root.to_path_buf()
    } else {
        root.join(relative.replace('\\', "/"))
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct VersionGuard {
    pub layout: String,
    pub current: Evidence,
    pub kept: Option<String>,
    pub lock: Evidence,
    pub updating: bool,
    pub reason: Option<String>,
}
impl VersionGuard {
    fn capture(layout: &VersionLayout, anchor: &Path) -> Self {
        let base = join(anchor, &layout.path);
        let versions = join(&base, &layout.versions);
        let (current, kept, mut reason) = match &layout.current {
            None => (Evidence::Confirmed, None, None),
            Some(relative) => match current_name(anchor, &versions, &join(&base, relative)) {
                Ok(Some(name)) => (Evidence::Confirmed, Some(name), None),
                Ok(None) => (Evidence::Absent, None, None),
                Err(reason) => (Evidence::Unknown, None, Some(reason)),
            },
        };
        let (lock, updating) = match &layout.lock {
            None => (Evidence::Confirmed, false),
            Some(policy) => match lock_state(
                anchor,
                &join(&base, &policy.path),
                policy.maximum_age_seconds,
            ) {
                Ok(Some(updating)) => (Evidence::Confirmed, updating),
                Ok(None) => (Evidence::Absent, false),
                Err(error) => {
                    reason = Some(error);
                    (Evidence::Unknown, false)
                }
            },
        };
        Self {
            layout: layout.id.clone(),
            current,
            kept,
            lock,
            updating,
            reason,
        }
    }
    fn idle(&self) -> bool {
        self.lock != Evidence::Unknown && !self.updating && self.current != Evidence::Unknown
    }
    pub(super) fn revalidate(
        &self,
        rule: &RuleDefinition,
        scope: Option<&Path>,
    ) -> Result<(), String> {
        let anchor = scope.ok_or("Missing version scope")?;
        let layout = rule
            .version_layouts
            .iter()
            .find(|layout| layout.id == self.layout)
            .ok_or("Missing frozen version layout")?;
        if !facts::confined_path(anchor, &join(anchor, &layout.path)).unwrap_or(false)
            || !std::fs::symlink_metadata(anchor)
                .is_ok_and(|md| md.is_dir() && !facts::is_link(&md))
        {
            return Err("Version root changed or unavailable".into());
        }
        let live = Self::capture(layout, anchor);
        if !self.idle() || !live.idle() {
            return Err(live
                .reason
                .or_else(|| self.reason.clone())
                .unwrap_or_else(|| "Version update is active or unknown".into()));
        }
        if self.current != live.current || self.kept != live.kept {
            return Err("Current version changed after scanning".into());
        }
        Ok(())
    }
}
fn current_name(anchor: &Path, versions: &Path, pointer: &Path) -> Result<Option<String>, String> {
    let parent = pointer.parent().ok_or("Missing current parent")?;
    if !facts::confined_path(anchor, parent)? {
        return Err("Current parent unavailable".into());
    }
    let md = match std::fs::symlink_metadata(pointer) {
        Ok(md) => md,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if !facts::is_link(&md) {
        return Err("Current pointer is not a supported link".into());
    }
    let target = std::fs::read_link(pointer).map_err(|error| error.to_string())?;
    if target.as_os_str().len() > 8192 {
        return Err("Current pointer budget exceeded".into());
    }
    let resolved = if target.is_absolute() {
        target
    } else {
        parent.join(target)
    };
    let resolved = std::fs::canonicalize(resolved).map_err(|error| error.to_string())?;
    let declared_versions = versions.to_path_buf();
    let versions = std::fs::canonicalize(versions).map_err(|error| error.to_string())?;
    if resolved.parent() != Some(versions.as_path()) {
        return Err("Current pointer escapes version inventory".into());
    }
    let name = resolved
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| token(name))
        .ok_or("Invalid current version name")?;
    let candidate = declared_versions.join(name);
    if !facts::confined_path(anchor, &candidate)?
        || !std::fs::symlink_metadata(&candidate)
            .is_ok_and(|md| md.is_dir() && !facts::is_link(&md))
    {
        return Err("Current target is not a confined directory".into());
    }
    Ok(Some(name.into()))
}
fn lock_state(anchor: &Path, path: &Path, maximum_age: u64) -> Result<Option<bool>, String> {
    match facts::confined_file(anchor, path) {
        Ok(false) => match std::fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
            Ok(_) => return Err("Update lock is not a confined file".into()),
        },
        Ok(true) => {}
        Err(error) => return Err(error),
    }
    let age = std::fs::symlink_metadata(path)
        .and_then(|md| md.modified())
        .map_err(|error| error.to_string())?
        .elapsed()
        .map_err(|error| error.to_string())?;
    Ok(Some(age.as_secs() < maximum_age))
}
pub(super) fn append(
    targets: &mut Vec<ScanTarget>,
    snapshot: &Arc<RuleSnapshot>,
    roots: &BTreeMap<String, Option<PathBuf>>,
    enumeration: &mut Enumeration,
) {
    for rule in &snapshot.bundle.rules {
        if rule.platform != "all" && rule.platform != std::env::consts::OS {
            continue;
        }
        for layout in &rule.version_layouts {
            if !layout.platform.matches() {
                continue;
            }
            let key = match layout.root {
                VersionRoot::Home => "home",
                VersionRoot::Roaming => "roaming",
            };
            let Some(anchor) = roots.get(key).and_then(Option::as_deref) else {
                continue;
            };
            append_layout(targets, snapshot, rule, layout, anchor, enumeration);
        }
    }
}
fn append_layout(
    targets: &mut Vec<ScanTarget>,
    snapshot: &Arc<RuleSnapshot>,
    rule: &RuleDefinition,
    layout: &VersionLayout,
    anchor: &Path,
    enumeration: &mut Enumeration,
) {
    let base = join(anchor, &layout.path);
    let versions = join(&base, &layout.versions);
    if !enumeration.directory(anchor, anchor).unwrap_or(false)
        || !enumeration.directory(anchor, &base).unwrap_or(false)
    {
        return;
    }
    let Some(children) = enumeration.children(anchor, &versions) else {
        return;
    };
    let guard = VersionGuard::capture(layout, anchor);
    let mut observation = RuleObservation::capture(snapshot, &rule.id, Some(anchor.to_path_buf()));
    if observation.detected != Evidence::Confirmed {
        return;
    }
    observation.version_guard = Some(guard.clone());
    let reference = RuleRef {
        snapshot: snapshot.clone(),
        id: rule.id.clone(),
        scope: Some(anchor.to_path_buf()),
        contributors: Vec::new(),
        blocked: guard.reason.clone(),
        observation: Some(Arc::new(observation)),
    };
    let emitter = VersionEmitter {
        layout,
        reference,
        enumeration,
    };
    emitter.emit(targets, children, &base, &guard);
}
struct VersionEmitter<'a> {
    layout: &'a VersionLayout,
    reference: RuleRef,
    enumeration: &'a mut Enumeration,
}
impl VersionEmitter<'_> {
    fn emit(
        self,
        targets: &mut Vec<ScanTarget>,
        children: Vec<super::directories::Child>,
        base: &Path,
        guard: &VersionGuard,
    ) {
        let category = CategoryId::from_rule(&self.layout.category).expect("validated category");
        for child in children {
            if !child.directory
                || self.layout.exclude.contains(&child.name)
                || guard.kept.as_ref() == Some(&child.name)
                || !self.layout.prefixes.is_empty()
                    && !self
                        .layout
                        .prefixes
                        .iter()
                        .any(|prefix| child.name.starts_with(prefix))
            {
                continue;
            }
            let path = self
                .layout
                .leaf
                .as_ref()
                .map_or_else(|| child.path.clone(), |leaf| join(&child.path, leaf));
            if !self
                .enumeration
                .directory(
                    self.reference.scope.as_deref().expect("version scope"),
                    &path,
                )
                .unwrap_or(false)
            {
                continue;
            }
            self.enumeration.emit_target(
                targets,
                ScanTarget {
                    path,
                    label: Text::new(
                        self.layout.zh.replace("{name}", &child.name),
                        self.layout.en.replace("{name}", &child.name),
                    ),
                    category,
                    operation: self.layout.operation.operation(),
                    disposal: self.layout.disposal,
                    recommended: self.layout.recommended
                        && guard.idle()
                        && guard.current == Evidence::Confirmed,
                    size_hint: None,
                    rule: self.reference.clone(),
                },
                ExpectedKind::Directory,
            );
        }
        if let Some(download) = &self.layout.downloads {
            let anchor = self.reference.scope.as_deref().expect("version scope");
            if let Some(files) = self
                .enumeration
                .children(anchor, &join(base, &download.path))
            {
                for file in files {
                    if file.directory
                        || !download
                            .suffixes
                            .iter()
                            .any(|suffix| file.name.ends_with(suffix))
                    {
                        continue;
                    }
                    self.enumeration.emit_target(
                        targets,
                        ScanTarget {
                            path: file.path,
                            label: Text::new(
                                download.zh.replace("{name}", &file.name),
                                download.en.replace("{name}", &file.name),
                            ),
                            category,
                            operation: download.operation.operation(),
                            disposal: download.disposal,
                            recommended: download.recommended && guard.idle(),
                            size_hint: None,
                            rule: self.reference.clone(),
                        },
                        ExpectedKind::File,
                    );
                }
            }
        }
    }
}
pub fn explain_at(snapshot: &Arc<RuleSnapshot>, id: &str, root: &Path) -> serde_json::Value {
    let rule = snapshot.definition(id);
    let mut enumeration = Enumeration::default();
    let mut targets = Vec::new();
    for layout in &rule.version_layouts {
        if layout.platform.matches() {
            append_layout(&mut targets, snapshot, rule, layout, root, &mut enumeration);
        }
    }
    let plans: Vec<_> = targets
        .into_iter()
        .map(|target| {
            let planned = super::PlannedTarget {
                operation: target.operation,
                identity: crate::core::model::capture_identity(&target.path),
                path: target.path,
                disposal: target.disposal,
            };
            let mut plan = super::CleanupPlan::new(target.rule, vec![planned]);
            if let Err(reason) = plan.validate() {
                plan.blocked.push(reason);
            }
            let mut result = plan.explanation();
            result["recommended"] = serde_json::json!(target.recommended);
            result
        })
        .collect();
    let mut result = enumeration.diagnostics();
    result["plans"] = serde_json::json!(plans);
    result
}
#[cfg(test)]
pub(crate) fn append_fixture(targets: &mut Vec<ScanTarget>, id: &str, home: &Path, roaming: &Path) {
    let snapshot = super::current();
    let rule = snapshot.definition("development");
    let layout = rule
        .version_layouts
        .iter()
        .find(|layout| layout.id == id)
        .expect("fixture layout");
    let anchor = match layout.root {
        VersionRoot::Home => home,
        VersionRoot::Roaming => roaming,
    };
    append_layout(
        targets,
        &snapshot,
        rule,
        layout,
        anchor,
        &mut Enumeration::default(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::rules::{CleanupPlan, Operation, PlannedTarget, RuleBundle};

    fn link(target: &Path, pointer: &Path) {
        #[cfg(windows)]
        {
            let output = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(pointer.components().collect::<PathBuf>())
                .arg(target.components().collect::<PathBuf>())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, pointer).unwrap();
    }
    fn layout(id: &str) -> VersionLayout {
        super::super::snapshot()
            .definition("development")
            .version_layouts
            .iter()
            .find(|l| l.id == id)
            .unwrap()
            .clone()
    }
    fn scan(root: &Path, layout: VersionLayout) -> Vec<ScanTarget> {
        let mut bundle = super::super::snapshot().bundle.clone();
        let rule = bundle
            .rules
            .iter_mut()
            .find(|r| r.id == "development")
            .unwrap();
        rule.version_layouts = vec![layout];
        bundle.validate().unwrap();
        let snapshot = Arc::new(RuleSnapshot { bundle });
        let mut targets = Vec::new();
        append_layout(
            &mut targets,
            &snapshot,
            snapshot.definition("development"),
            &snapshot.definition("development").version_layouts[0],
            root,
            &mut Enumeration::default(),
        );
        targets
    }
    fn plan(target: &ScanTarget) -> CleanupPlan {
        CleanupPlan::new(
            target.rule.clone(),
            vec![PlannedTarget {
                path: target.path.clone(),
                operation: target.operation.clone(),
                disposal: target.disposal,
                identity: crate::core::model::capture_identity(&target.path),
            }],
        )
    }

    /// 为一个版本布局在 `anchor` 下造出「旧版本 + 当前版本（current 指针）+ 锁 +
    /// 下载包」的最小结构，供金样扫描。
    fn build_version_fixture(anchor: &Path, layout: &VersionLayout) {
        let base = join(anchor, &layout.path);
        let versions = join(&base, &layout.versions);
        std::fs::create_dir_all(versions.join("9.9.9")).unwrap();
        std::fs::create_dir_all(versions.join("8.8.8")).unwrap();
        if let Some(current) = &layout.current {
            let pointer = join(&base, current);
            if let Some(parent) = pointer.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            link(&versions.join("8.8.8"), &pointer);
        }
        if let Some(lock) = &layout.lock {
            let lock_path = join(&base, &lock.path);
            if let Some(parent) = lock_path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&lock_path, b"x").unwrap();
        }
        if let Some(downloads) = &layout.downloads {
            let dir = join(&base, &downloads.path);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("installer.tar.gz"), b"pkg").unwrap();
        }
    }

    /// 版本布局金样：迁移前后每个布局的目标 / 推荐 / 操作 / 处置逐项对照。
    #[test]
    fn version_layouts_match_the_baseline() {
        let root = crate::core::testing::fixture("version_layout_baseline");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let snapshot = super::super::snapshot();
        let mut actual = Vec::new();
        for layout in snapshot.definition("development").version_layouts.clone() {
            if !layout.platform.matches() {
                continue;
            }
            build_version_fixture(&root, &layout);
            for target in scan(&root, layout.clone()) {
                actual.push(serde_json::json!({
                    "layout": layout.id,
                    "path": target.path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"),
                    "category": format!("{:?}", target.category),
                    "operation": target.operation,
                    "disposal": target.disposal,
                    "recommended": target.recommended,
                }));
            }
        }
        actual.sort_by_key(|row| {
            (
                row["layout"].as_str().unwrap().to_owned(),
                row["path"].as_str().unwrap().to_owned(),
            )
        });
        let expected: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../rules/fixtures/version-layout-baseline.json"
        ))
        .unwrap();
        assert_eq!(actual, expected);
        let _ = std::fs::remove_dir_all(&root);
    }
    #[test]
    fn version_pointer_retention_and_locks_revalidate_frozen_plans() {
        let root = crate::core::testing::fixture("version_pointer_guard");
        let layout = layout("codex_cli");
        let base = join(&root, &layout.path);
        let versions = base.join("releases");
        for name in ["old", "current-version", "next"] {
            std::fs::create_dir_all(versions.join(name)).unwrap();
        }
        let pointer = base.join("current");
        link(&versions.join("current-version"), &pointer);
        let targets = scan(&root, layout.clone());
        assert_eq!(targets.len(), 2);
        assert!(targets
            .iter()
            .all(|t| t.recommended && t.operation == Operation::Contents));
        assert!(targets
            .iter()
            .all(|t| t.path != versions.join("current-version")));
        let frozen = plan(&targets[0]);
        frozen.validate().unwrap();
        std::fs::write(base.join("install.lock"), b"active").unwrap();
        assert!(frozen.validate().is_err());
        assert!(scan(&root, layout.clone())
            .iter()
            .all(|t| !t.recommended && plan(t).validate().is_err()));
        let lock = std::fs::OpenOptions::new()
            .write(true)
            .open(base.join("install.lock"))
            .unwrap();
        lock.set_times(
            std::fs::FileTimes::new()
                .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(7200)),
        )
        .unwrap();
        drop(lock);
        assert!(scan(&root, layout.clone()).iter().all(|t| t.recommended));
        std::fs::remove_file(base.join("install.lock")).unwrap();
        std::fs::remove_dir(&pointer)
            .or_else(|_| std::fs::remove_file(&pointer))
            .unwrap();
        link(&versions.join("next"), &pointer);
        assert!(frozen
            .validate()
            .unwrap_err()
            .contains("Current version changed"));
        assert_eq!(frozen.rule.snapshot.definition("development").version, 5);
        std::fs::remove_dir(&pointer)
            .or_else(|_| std::fs::remove_file(&pointer))
            .unwrap();
        assert!(scan(&root, layout).iter().all(|t| !t.recommended));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn invalid_version_pointer_or_lock_never_grants_cleanup() {
        let root = crate::core::testing::fixture("version_unknown_guard");
        let layout = layout("codex_cli");
        let base = join(&root, &layout.path);
        std::fs::create_dir_all(base.join("releases/old")).unwrap();
        let external = root.join("external");
        std::fs::create_dir_all(&external).unwrap();
        let pointer = base.join("current");
        link(&external, &pointer);
        let targets = scan(&root, layout.clone());
        assert_eq!(targets.len(), 1);
        assert!(targets[0].rule.blocked.is_some());
        assert!(plan(&targets[0]).validate().is_err());
        std::fs::remove_dir(&pointer)
            .or_else(|_| std::fs::remove_file(&pointer))
            .unwrap();
        std::fs::write(&pointer, b"ordinary file").unwrap();
        assert!(scan(&root, layout.clone())
            .iter()
            .all(|t| plan(t).validate().is_err()));
        std::fs::remove_file(&pointer).unwrap();
        link(&base.join("releases/old"), &pointer);
        std::fs::create_dir_all(base.join("install.lock")).unwrap();
        assert!(scan(&root, layout.clone())
            .iter()
            .all(|t| plan(t).validate().is_err()));
        std::fs::remove_dir(base.join("install.lock")).unwrap();
        std::fs::remove_dir(&pointer)
            .or_else(|_| std::fs::remove_file(&pointer))
            .unwrap();
        assert!(external.is_dir());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn version_downloads_are_exact_files_and_leaf_layouts_keep_version_parents() {
        let root = crate::core::testing::fixture("version_download_leaf");
        let devin = layout("devin_windows");
        let base = join(&root, &devin.path);
        std::fs::create_dir_all(base.join("_versions/old")).unwrap();
        let downloads = base.join("_versions/_download");
        std::fs::create_dir_all(downloads.join("directory.tar.gz")).unwrap();
        std::fs::write(downloads.join("installer.tar.gz"), b"archive").unwrap();
        std::fs::write(downloads.join("preserved.txt"), b"data").unwrap();
        let targets = scan(&root, devin);
        assert_eq!(targets.len(), 2);
        let archive = targets
            .iter()
            .find(|t| t.operation == Operation::File)
            .unwrap();
        assert_eq!(archive.path, downloads.join("installer.tar.gz"));
        assert!(archive.recommended);
        plan(archive).validate().unwrap();
        assert!(
            !targets
                .iter()
                .find(|t| t.operation == Operation::Contents)
                .unwrap()
                .recommended
        );
        let zed = layout("zed_node");
        let node = join(&root, &zed.path);
        for leaf in ["node-v1/cache", "node-v2", "unrelated/cache"] {
            std::fs::create_dir_all(node.join(leaf)).unwrap();
        }
        let targets = scan(&root, zed);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, node.join("node-v1/cache"));
        assert!(targets[0].recommended);
        plan(&targets[0]).validate().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn version_rule_only_fixture_uses_shared_discovery_and_pinned_snapshot() {
        let root = crate::core::testing::fixture("version_rule_only");
        let rule: RuleDefinition = toml::from_str(include_str!(
            "../../../rules/fixtures/version-layout-extra.toml"
        ))
        .unwrap();
        let cache = root.join("Alternative/runtime/release-1/generated/cache");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::create_dir_all(root.join("Alternative/runtime/release-protected/generated/cache"))
            .unwrap();
        let original = super::super::snapshot();
        let mut bundle = original.bundle.clone();
        bundle.rules.push(rule);
        bundle.validate().unwrap();
        let snapshot = Arc::new(RuleSnapshot { bundle });
        let explanation = explain_at(&snapshot, "fixture-version-layout", &root);
        assert_eq!(explanation["plans"].as_array().unwrap().len(), 1);
        assert_eq!(explanation["plans"][0]["recommended"], false);
        assert_eq!(explanation["plans"][0]["blocked"], serde_json::json!([]));
        assert_eq!(explanation["directory_reads"], 1);
        assert!(explain_at(&original, "development", &root)["plans"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(cache.is_dir());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn version_layout_validation_rejects_escape_scripts_and_unbounded_parameters() {
        let original = super::super::snapshot();
        for fault in [
            "path",
            "versions",
            "current",
            "lock",
            "age",
            "capability",
            "operation",
            "prefix",
            "id",
            "count",
        ] {
            let mut bundle = original.bundle.clone();
            let rule = bundle
                .rules
                .iter_mut()
                .find(|r| r.id == "development")
                .unwrap();
            let layout = &mut rule.version_layouts[0];
            match fault {
                "path" => layout.path = "../outside".into(),
                "versions" => layout.versions = "C:/outside".into(),
                "current" => layout.current = Some("../current".into()),
                "lock" => layout.lock.as_mut().unwrap().path = "../lock".into(),
                "age" => layout.lock.as_mut().unwrap().maximum_age_seconds = 0,
                "capability" => rule.required.retain(|c| c != "version_retention"),
                "operation" => layout.operation = PathOperation::Tree,
                "prefix" => layout.prefixes = vec!["..".into()],
                "id" => layout.id = ".".into(),
                "count" => rule.version_layouts = vec![layout.clone(); 33],
                _ => unreachable!(),
            }
            assert!(bundle.validate().is_err(), "{fault}");
        }
        let mut value = serde_json::to_value(&original.bundle).unwrap();
        let rule = value["rules"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r["id"] == "development")
            .unwrap();
        rule["version_layouts"][0]["command"] = "powershell arbitrary".into();
        assert!(RuleBundle::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    }
}
