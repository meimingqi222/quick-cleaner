//! Bounded layout selection shares directory reads; safety remains authoritative.
use super::{facts, PathOperation, RuleDefinition, RuleObservation, RuleRef, RuleSnapshot};
use crate::core::{
    categories::{CategoryId, ScanTarget},
    cleaner::Disposal,
    i18n::Text,
    safety,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

const DIRECTORY_LIMIT: usize = 4096;
const ENTRY_LIMIT: usize = 16384;
const PROBE_LIMIT: usize = 512;
/// One declared template expands over declared name lists only, never over data.
const EXPANSION_LIMIT: usize = 64;
const SEGMENT_LIMIT: usize = 8;
/// Supplied by the selector; a template key cannot shadow or redefine them.
const RESERVED_TOKENS: [&str; 3] = ["name", "parent", "relative"];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryRule {
    pub id: String,
    pub root: DirectoryRoot,
    pub paths: Vec<String>,
    pub select: Selection,
    pub zh: String,
    pub en: String,
    pub category: String,
    pub operation: PathOperation,
    pub disposal: Disposal,
    pub recommended: bool,
    #[serde(default)]
    pub log_policy: Option<LogPolicy>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryRoot {
    Home,
    Roaming,
    UserTempParent,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Selection {
    Children {
        directories_only: bool,
        prefixes: Vec<String>,
    },
    ContainerDirectories {
        paths: Vec<String>,
        exclude_name_policy: String,
    },
    NamedFiles {
        name: String,
        child_depth: u8,
    },
    /// A named directory inside the declared path or inside each of its children.
    NamedDirectories {
        name: String,
        child_depth: u8,
    },
    /// Literal name parts select precise files, never a tree.
    Files {
        prefixes: Vec<String>,
        suffixes: Vec<String>,
    },
    /// The declared path itself; its kind has to match the typed operation.
    Path {
        directories_only: bool,
    },
    /// Directories named by a JSON object manifest inside the declared path.
    ManifestChildren {
        manifest: String,
        selected_value: bool,
    },
    /// Child directories whose manifest records a local path that no longer exists.
    OrphanedChildren {
        manifest: String,
        pointer: String,
        uri_prefix: String,
        scope: DirectoryRoot,
    },
    /// Top-level directories dispatched by declared name catalogs; unmatched
    /// children fall back to the entry's own shape, and a declared leaf
    /// signature wins over the catalogs.
    CatalogChildren {
        catalog_policy: BTreeMap<String, CatalogPolicy>,
        #[serde(default)]
        leaf_policy: Option<LeafPolicy>,
    },
}
/// Per-catalog target shape for `catalog_children`: where a matched child goes
/// and whether it is preselected. The rule's own category/recommended stay the
/// fallback for children no catalog claims.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogPolicy {
    pub category: String,
    pub recommended: bool,
}
/// Leaf-signature targets emitted inside a `catalog_children` child.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeafPolicy {
    pub signature: String,
    pub category: String,
    pub zh: String,
    pub en: String,
}
/// Implemented leaf-signature capabilities; the vocabulary itself lives in the
/// named rule, configuration only selects the capability.
const LEAF_SIGNATURES: [&str; 1] = ["chromium"];
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogPolicy {
    pub extensions: Vec<String>,
    pub exclude_exact: Vec<String>,
    pub exclude_prefixes: Vec<String>,
}
fn name(value: &str) -> bool {
    !value.trim().is_empty()
        && !matches!(value, "." | "..")
        && value.len() <= 128
        && !value.contains(['/', '\\', ':', '\0', '\n', '\r'])
}
/// Manifest file names, JSON pointers and URI prefixes stay literal.
fn manifest(value: &str) -> bool {
    name(value) && value.len() <= 64
}
fn pointer(value: &str) -> bool {
    value.len() <= 64
        && value.starts_with('/')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'-'))
}
fn uri_prefix(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'/' | b'.' | b'_' | b'-'))
}
fn names(values: &[String]) -> bool {
    values.len() <= 128 && values.iter().all(|v| name(v))
}
fn label(value: &str, tokens: &BTreeSet<String>) -> bool {
    let mut rest = value
        .replace("{name}", "")
        .replace("{parent}", "")
        .replace("{relative}", "");
    for token in tokens {
        rest = rest.replace(token, "");
    }
    bounded_label(value, &rest)
}
/// Leaf-signature labels may use `{trail}` in addition to the standard tokens.
fn leaf_label(value: &str) -> bool {
    let rest = value
        .replace("{name}", "")
        .replace("{parent}", "")
        .replace("{relative}", "")
        .replace("{trail}", "");
    bounded_label(value, &rest)
}
fn bounded_label(value: &str, rest: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 1024 && !rest.contains(['{', '}', '\0', '\r', '\n'])
}
/// The declared leaf-signature capability. Configuration only selects an
/// implemented capability; the vocabulary lives in the named rule.
fn leaf_signature(name: &str, dir: &Path) -> Vec<PathBuf> {
    match name {
        "chromium" => crate::core::categories::chromium::cache_leaves(dir),
        _ => Vec::new(),
    }
}
/// True when two dispatched catalogs claim the same child name.
fn duplicate_dispatch_paths(
    rule: &RuleDefinition,
    policy: &BTreeMap<String, CatalogPolicy>,
) -> bool {
    let mut seen = BTreeSet::new();
    policy
        .keys()
        .flat_map(|name| rule.catalogs[name].iter().map(|row| row.path.as_str()))
        .any(|path| !seen.insert(path))
}
/// Declared path templates expand over declared name lists and catalogs only.
fn template(path: &str, rule: &RuleDefinition) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut segments = 0usize;
    let mut expansions = 1usize;
    for segment in path.split(['/', '\\']) {
        segments += 1;
        if segments > SEGMENT_LIMIT {
            return Err("Path template is too deep".into());
        }
        let Some(key) = segment
            .strip_prefix('{')
            .and_then(|rest| rest.strip_suffix('}'))
        else {
            if segment.contains(['{', '}']) {
                return Err("Partial template segment".into());
            }
            continue;
        };
        if tokens.len() >= 2 || !name(key) || RESERVED_TOKENS.contains(&key) {
            return Err("Unsupported path template token".into());
        }
        let named = match (rule.catalogs.get(key), rule.lists.get(key)) {
            (Some(rows), _) if !rows.is_empty() => rows.len(),
            (None, Some(values)) if !values.is_empty() && names(values) => values.len(),
            _ => return Err(format!("Unknown or unbounded template source: {key}")),
        };
        expansions = expansions
            .checked_mul(named)
            .filter(|total| *total <= EXPANSION_LIMIT)
            .ok_or("Path template expands too far")?;
        tokens.push(format!("{{{key}}}"));
    }
    Ok(tokens)
}
pub(super) fn validate(rule: &RuleDefinition) -> Result<(), String> {
    if rule.directories.len() > 32
        || (!rule.directories.is_empty() && !rule.variables.is_empty())
        || (!rule.directories.is_empty()
            && !rule.required.iter().any(|c| c == "directory_selection"))
    {
        return Err("Undeclared or unbounded directory selection".into());
    }
    let mut ids = BTreeSet::new();
    for entry in &rule.directories {
        let valid_selection = match &entry.select {
            Selection::Children { prefixes, .. } => {
                names(prefixes) && entry.operation == PathOperation::Contents
            }
            Selection::ContainerDirectories {
                paths,
                exclude_name_policy,
            } => {
                !paths.is_empty()
                    && paths.len() <= 32
                    && paths.iter().all(|p| super::relative(p))
                    && name(exclude_name_policy)
                    && entry.operation == PathOperation::Contents
                    && entry.log_policy.is_none()
                    && ["exact", "prefixes", "contains", "suffixes"]
                        .iter()
                        .any(|mode| {
                            rule.lists
                                .get(&format!("{exclude_name_policy}_{mode}"))
                                .is_some_and(|values| !values.is_empty() && names(values))
                        })
            }
            Selection::NamedFiles {
                name: file,
                child_depth,
            } => {
                name(file)
                    && *child_depth <= 1
                    && entry.operation == PathOperation::File
                    && entry.log_policy.is_none()
            }
            Selection::NamedDirectories {
                name: dir,
                child_depth,
            } => {
                name(dir)
                    && *child_depth <= 1
                    && entry.operation == PathOperation::Contents
                    && entry.log_policy.is_none()
            }
            Selection::Files { prefixes, suffixes } => {
                names(prefixes)
                    && names(suffixes)
                    && (!prefixes.is_empty() || !suffixes.is_empty())
                    && entry.operation == PathOperation::File
                    && entry.log_policy.is_none()
            }
            Selection::Path { directories_only } => {
                (match entry.operation {
                    PathOperation::Contents => *directories_only,
                    PathOperation::File => !*directories_only,
                    _ => false,
                }) && entry.log_policy.is_none()
            }
            Selection::ManifestChildren { manifest: file, .. } => {
                manifest(file)
                    && entry.operation == PathOperation::Tree
                    && entry.log_policy.is_none()
            }
            Selection::OrphanedChildren {
                manifest: file,
                pointer: json,
                uri_prefix: prefix,
                scope,
            } => {
                manifest(file)
                    && pointer(json)
                    && uri_prefix(prefix)
                    && matches!(scope, DirectoryRoot::Home | DirectoryRoot::Roaming)
                    && entry.operation == PathOperation::Tree
                    && entry.log_policy.is_none()
            }
            Selection::CatalogChildren {
                catalog_policy,
                leaf_policy,
            } => {
                entry.operation == PathOperation::Contents
                    && entry.log_policy.is_none()
                    && catalog_policy.len() <= 32
                    && catalog_policy.iter().all(|(name, policy)| {
                        rule.catalogs.get(name).is_some_and(|rows| {
                            !rows.is_empty() && rows.len() <= 128
                        })
                            && CategoryId::from_rule(&policy.category).is_some()
                    })
                    && leaf_policy.as_ref().is_none_or(|policy| {
                        LEAF_SIGNATURES.contains(&policy.signature.as_str())
                            && CategoryId::from_rule(&policy.category).is_some()
                            && leaf_label(&policy.zh)
                            && leaf_label(&policy.en)
                    })
                    && entry.zh.contains("{name}")
                    && entry.en.contains("{name}")
                    // A child name must resolve to exactly one shape: the same
                    // path dispatched by two catalogs would make the plan depend
                    // on map iteration order.
                    && !duplicate_dispatch_paths(rule, catalog_policy)
            }
        };
        let mut tokens = BTreeSet::new();
        let valid_templates = entry.paths.iter().all(|path| {
            template(path, rule).is_ok_and(|found| found.iter().all(|t| tokens.insert(t.clone())))
        });
        if matches!(entry.select, Selection::OrphanedChildren { .. }) {
            // The recorded project path is evidence, not configuration.
            tokens.insert("{project}".into());
        }
        if !name(&entry.id)
            || !ids.insert(&entry.id)
            || entry.paths.is_empty()
            || entry.paths.len() > 32
            || entry.paths.iter().any(|p| !super::relative(p))
            || !valid_templates
            || !label(&entry.zh, &tokens)
            || !label(&entry.en, &tokens)
            || CategoryId::from_rule(&entry.category).is_none()
            || !valid_selection
            || entry.disposal != Disposal::Permanent
            || !rule.required.iter().any(|c| c == "file")
            || entry.log_policy.as_ref().is_some_and(|p| {
                p.extensions.is_empty()
                    || !names(&p.extensions)
                    || !names(&p.exclude_exact)
                    || !names(&p.exclude_prefixes)
            })
        {
            return Err(format!("Invalid directory selection: {}", entry.id));
        }
    }
    Ok(())
}
/// The anchor a declared root resolves to for this scan; missing roots are absent.
fn declared_root(
    root: DirectoryRoot,
    roots: &BTreeMap<String, Option<PathBuf>>,
) -> Option<PathBuf> {
    match root {
        DirectoryRoot::Home => roots.get("home")?.clone(),
        DirectoryRoot::Roaming => roots.get("roaming")?.clone(),
        DirectoryRoot::UserTempParent => roots
            .get("user_temp")?
            .as_ref()?
            .parent()
            .map(Path::to_path_buf),
    }
}
#[derive(Clone)]
struct Binding {
    token: String,
    zh: String,
    en: String,
}
impl Binding {
    fn render(&self, template: &str, en: bool) -> String {
        template.replace(&self.token, if en { &self.en } else { &self.zh })
    }
}
/// Validation bounded this expansion; configuration order is kept so plans stay stable.
fn expand(rule: &RuleDefinition, path: &str) -> Option<Vec<(String, Vec<Binding>)>> {
    let mut expanded = vec![(String::new(), Vec::new())];
    for segment in path.split(['/', '\\']) {
        let key = segment
            .strip_prefix('{')
            .and_then(|rest| rest.strip_suffix('}'));
        let values: Vec<(String, Option<Binding>)> = match key {
            None => vec![(segment.to_string(), None)],
            Some(key) => {
                let bind = |token: String, zh: String, en: String| Some(Binding { token, zh, en });
                rule.catalogs
                    .get(key)
                    .filter(|rows| !rows.is_empty())
                    .map(|rows| {
                        rows.iter()
                            .map(|row| {
                                (
                                    row.path.clone(),
                                    bind(format!("{{{key}}}"), row.zh.clone(), row.en.clone()),
                                )
                            })
                            .collect()
                    })
                    .or_else(|| {
                        rule.lists
                            .get(key)
                            .filter(|values| !values.is_empty() && names(values))
                            .map(|values| {
                                values
                                    .iter()
                                    .map(|value| {
                                        (
                                            value.clone(),
                                            bind(
                                                format!("{{{key}}}"),
                                                value.clone(),
                                                value.clone(),
                                            ),
                                        )
                                    })
                                    .collect()
                            })
                    })?
            }
        };
        let mut next = Vec::new();
        for (prefix, bindings) in &expanded {
            for (value, binding) in &values {
                if next.len() >= EXPANSION_LIMIT {
                    return None;
                }
                let mut bindings = bindings.clone();
                if let Some(binding) = binding {
                    bindings.push(binding.clone());
                }
                next.push((
                    if prefix.is_empty() {
                        value.clone()
                    } else {
                        format!("{prefix}/{value}")
                    },
                    bindings,
                ));
            }
        }
        expanded = next;
    }
    Some(expanded)
}
#[derive(Clone)]
pub(super) struct Child {
    pub(super) path: PathBuf,
    pub(super) name: String,
    pub(super) directory: bool,
}
#[derive(Clone, serde::Serialize)]
struct DirectoryProbe {
    state: facts::Evidence,
    reason: Option<String>,
}
#[derive(Default)]
pub(super) struct Enumeration {
    cache: BTreeMap<PathBuf, Option<Vec<Child>>>,
    probes: BTreeMap<PathBuf, DirectoryProbe>,
    manifests: BTreeMap<PathBuf, Option<serde_json::Value>>,
    entries: usize,
    manifest_rows: usize,
    reads: usize,
    manifest_reads: usize,
    budget_blocked: usize,
    candidate_checks: usize,
    candidate_budget_blocked: usize,
}
impl Enumeration {
    /// A bounded JSON manifest read; absent, invalid, oversized or over-budget stay unknown.
    pub(super) fn manifest(&mut self, anchor: &Path, path: &Path) -> Option<serde_json::Value> {
        if let Some(cached) = self.manifests.get(path) {
            return cached.clone();
        }
        let rows = if self.manifest_reads >= PROBE_LIMIT {
            None
        } else {
            self.manifest_reads += 1;
            facts::confined_file(anchor, path)
                .unwrap_or(false)
                .then(|| facts::read(path).ok())
                .flatten()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        };
        self.manifests.insert(path.to_path_buf(), rows.clone());
        rows
    }
    /// Manifest rows are charged against the same entry budget as inventories.
    pub(super) fn charge(&mut self, rows: usize) -> bool {
        self.manifest_rows += rows;
        self.entries + self.manifest_rows <= ENTRY_LIMIT
    }
    pub(super) fn directory(&mut self, anchor: &Path, path: &Path) -> Result<bool, String> {
        if let Some(probe) = self.probes.get(path) {
            return match probe.state {
                facts::Evidence::Confirmed => Ok(true),
                facts::Evidence::Absent => Ok(false),
                facts::Evidence::Unknown => Err(probe
                    .reason
                    .clone()
                    .unwrap_or_else(|| "Unknown directory".into())),
            };
        }
        if self.probes.len() >= PROBE_LIMIT {
            self.budget_blocked += 1;
            return Err("Directory probe budget exhausted".into());
        }
        let result = (|| {
            if path != anchor && !facts::confined_path(anchor, path)? {
                return Ok(false);
            }
            let md = match std::fs::symlink_metadata(path) {
                Ok(md) => md,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(e) => return Err(e.to_string()),
            };
            if facts::is_link(&md) || !md.is_dir() {
                return Err("Not a confined directory".into());
            }
            Ok(true)
        })();
        let probe = match &result {
            Ok(true) => DirectoryProbe {
                state: facts::Evidence::Confirmed,
                reason: None,
            },
            Ok(false) => DirectoryProbe {
                state: facts::Evidence::Absent,
                reason: None,
            },
            Err(reason) => DirectoryProbe {
                state: facts::Evidence::Unknown,
                reason: Some(reason.clone()),
            },
        };
        self.probes.insert(path.to_path_buf(), probe);
        result
    }
    pub(super) fn children(&mut self, anchor: &Path, path: &Path) -> Option<Vec<Child>> {
        if let Some(cached) = self.cache.get(path) {
            return cached.clone();
        }
        let result = self.read(anchor, path);
        if self.probes.contains_key(path) {
            if let Err(reason) = &result {
                self.probes.insert(
                    path.to_path_buf(),
                    DirectoryProbe {
                        state: facts::Evidence::Unknown,
                        reason: Some(reason.clone()),
                    },
                );
            }
            self.cache
                .insert(path.to_path_buf(), result.clone().ok().flatten());
        }
        result.ok().flatten()
    }
    fn read(&mut self, anchor: &Path, path: &Path) -> Result<Option<Vec<Child>>, String> {
        if !self.directory(anchor, path)? {
            return Ok(None);
        }
        if self.entries >= ENTRY_LIMIT {
            return Err("Directory entry budget exhausted".into());
        }
        self.reads += 1;
        let mut result = Vec::new();
        for (index, entry) in std::fs::read_dir(path)
            .map_err(|e| e.to_string())?
            .enumerate()
        {
            // An incomplete inventory is unknown, never a partial deletion grant.
            self.entries += 1;
            if self.entries > ENTRY_LIMIT || index >= DIRECTORY_LIMIT {
                return Err("Directory inventory budget exhausted".into());
            }
            let entry = entry.map_err(|e| e.to_string())?;
            let md = std::fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
            if facts::is_link(&md) || (!md.is_dir() && !md.is_file()) {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            result.push(Child {
                path: entry.path(),
                name,
                directory: md.is_dir(),
            });
        }
        result.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Some(result))
    }
}
#[derive(Clone, Copy)]
pub(super) enum ExpectedKind {
    File,
    Directory,
    Any,
}
impl Enumeration {
    pub(super) fn emit_target(
        &mut self,
        targets: &mut Vec<ScanTarget>,
        target: ScanTarget,
        kind: ExpectedKind,
    ) {
        if self.candidate_checks >= ENTRY_LIMIT {
            self.candidate_budget_blocked += 1;
            return;
        }
        self.candidate_checks += 1;
        let Some(anchor) = &target.rule.scope else {
            return;
        };
        let protected = if target.operation == super::Operation::Contents {
            safety::is_contents_protected(&target.path)
        } else {
            safety::is_protected_residual_path(&target.path)
        };
        if protected || !facts::confined_path(anchor, &target.path).unwrap_or(false) {
            return;
        }
        let Ok(metadata) = std::fs::symlink_metadata(&target.path) else {
            return;
        };
        let type_matches = match kind {
            ExpectedKind::File => metadata.is_file(),
            ExpectedKind::Directory => metadata.is_dir(),
            ExpectedKind::Any => metadata.is_dir() || metadata.is_file(),
        };
        if !facts::is_link(&metadata) && type_matches {
            targets.push(target);
        }
    }
    pub(super) fn diagnostics(&self) -> serde_json::Value {
        serde_json::json!({"directory_reads": self.reads, "inventory_entries": self.entries, "manifest_reads": self.manifest_reads, "manifest_rows": self.manifest_rows, "directory_probes": self.probes, "probe_limit": PROBE_LIMIT, "budget_blocked": self.budget_blocked, "candidate_checks": self.candidate_checks, "candidate_budget_blocked": self.candidate_budget_blocked})
    }
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
        append_rule(targets, snapshot, rule, roots, enumeration, None);
    }
}
fn append_rule(
    targets: &mut Vec<ScanTarget>,
    snapshot: &Arc<RuleSnapshot>,
    rule: &RuleDefinition,
    roots: &BTreeMap<String, Option<PathBuf>>,
    enumeration: &mut Enumeration,
    only: Option<&str>,
) {
    let mut observations = BTreeMap::new();
    for entry in &rule.directories {
        if only.is_some_and(|id| entry.id != id) {
            continue;
        }
        let Some(anchor) = declared_root(entry.root, roots) else {
            continue;
        };
        if !enumeration.directory(&anchor, &anchor).unwrap_or(false) {
            continue;
        }
        let observation = observations.entry(anchor.clone()).or_insert_with(|| {
            Arc::new(RuleObservation::capture(
                snapshot,
                &rule.id,
                Some(anchor.clone()),
            ))
        });
        if observation.detected != facts::Evidence::Confirmed {
            continue;
        }
        let emitter = Emitter {
            snapshot,
            rule,
            entry,
            anchor: &anchor,
            observation,
        };
        for declared in &entry.paths {
            let Some(expanded) = expand(rule, declared) else {
                continue;
            };
            for (relative, bindings) in expanded {
                let root = anchor.join(relative.replace('\\', "/"));
                if let Selection::ManifestChildren {
                    manifest,
                    selected_value,
                } = &entry.select
                {
                    let Some(rows) = enumeration.manifest(&anchor, &root.join(manifest)) else {
                        continue;
                    };
                    let Some(rows) = rows.as_object() else {
                        continue;
                    };
                    if !enumeration.charge(rows.len()) {
                        continue;
                    }
                    for (row, value) in rows.iter().take(DIRECTORY_LIMIT) {
                        if value.as_bool() != Some(*selected_value) || !name(row) {
                            continue;
                        }
                        let path = root.join(row);
                        if !enumeration.directory(&anchor, &path).unwrap_or(false) {
                            continue;
                        }
                        emitter.emit(
                            targets,
                            enumeration,
                            Candidate::new(path, row, &format!("{relative}/{row}")),
                            &bindings,
                            true,
                        );
                    }
                    continue;
                }
                if let Selection::Path { directories_only } = &entry.select {
                    let name = relative.rsplit('/').next().unwrap_or_default();
                    let present = if *directories_only {
                        enumeration.directory(&anchor, &root).unwrap_or(false)
                    } else {
                        facts::confined_file(&anchor, &root).unwrap_or(false)
                    };
                    if present {
                        emitter.emit(
                            targets,
                            enumeration,
                            Candidate::new(root, name, &relative),
                            &bindings,
                            true,
                        );
                    }
                    continue;
                }
                let Some(children) = enumeration.children(&anchor, &root) else {
                    continue;
                };
                match &entry.select {
                    Selection::Path { .. } | Selection::ManifestChildren { .. } => unreachable!(),
                    Selection::OrphanedChildren {
                        manifest,
                        pointer: json,
                        uri_prefix: prefix,
                        scope: declared,
                    } => {
                        let Some(scope) = declared_root(*declared, roots) else {
                            continue;
                        };
                        for child in children {
                            if !child.directory {
                                continue;
                            }
                            let recorded = enumeration
                                .manifest(&anchor, &child.path.join(manifest))
                                .and_then(|rows| {
                                    rows.pointer(json)
                                        .and_then(serde_json::Value::as_str)
                                        .map(str::to_owned)
                                });
                            let Some(recorded) = recorded else {
                                continue;
                            };
                            let Some(project) = recorded.strip_prefix(prefix.as_str()) else {
                                continue;
                            };
                            // An encoded URI cannot be resolved back to a local path.
                            if project.contains('%') {
                                continue;
                            }
                            let project = Path::new(project);
                            if !project.starts_with(&scope) || project.exists() {
                                continue;
                            }
                            let mut bindings = bindings.clone();
                            let display = project.display().to_string();
                            bindings.push(Binding {
                                token: "{project}".into(),
                                zh: display.clone(),
                                en: display,
                            });
                            emitter.emit(
                                targets,
                                enumeration,
                                Candidate::new(
                                    child.path.clone(),
                                    &child.name,
                                    &format!("{relative}/{}", child.name),
                                ),
                                &bindings,
                                true,
                            );
                        }
                    }
                    Selection::Files { prefixes, suffixes } => {
                        for child in children {
                            if child.directory
                                || !prefixes.is_empty()
                                    && !prefixes.iter().any(|p| child.name.starts_with(p))
                                || !suffixes.is_empty()
                                    && !suffixes.iter().any(|s| child.name.ends_with(s))
                            {
                                continue;
                            }
                            emitter.emit(
                                targets,
                                enumeration,
                                Candidate::new(
                                    child.path.clone(),
                                    &child.name,
                                    &format!("{relative}/{}", child.name),
                                ),
                                &bindings,
                                true,
                            );
                        }
                    }
                    Selection::Children {
                        directories_only,
                        prefixes,
                    } => {
                        for child in children {
                            if *directories_only && !child.directory
                                || !prefixes.is_empty()
                                    && !prefixes.iter().any(|p| child.name.starts_with(p))
                            {
                                continue;
                            }
                            let recommended = entry.log_policy.as_ref().is_none_or(|policy| {
                                (child.directory
                                    || child.path.extension().is_some_and(|ext| {
                                        policy
                                            .extensions
                                            .iter()
                                            .any(|e| ext.eq_ignore_ascii_case(e))
                                    }))
                                    && !policy.exclude_exact.contains(&child.name)
                                    && !policy
                                        .exclude_prefixes
                                        .iter()
                                        .any(|p| child.name.starts_with(p))
                                    && (!child.directory
                                        || !safety::holds_live_database(&child.path))
                            });
                            emitter.emit(
                                targets,
                                enumeration,
                                Candidate::new(
                                    child.path.clone(),
                                    &child.name,
                                    &format!("{relative}/{}", child.name),
                                ),
                                &bindings,
                                recommended,
                            );
                        }
                    }
                    Selection::ContainerDirectories {
                        paths,
                        exclude_name_policy,
                    } => {
                        for child in children {
                            if !child.directory
                                || snapshot.matches_name(
                                    &rule.id,
                                    exclude_name_policy,
                                    &child.name.to_lowercase(),
                                )
                            {
                                continue;
                            }
                            for suffix in paths {
                                let path = child.path.join(suffix.replace('\\', "/"));
                                if enumeration.directory(&anchor, &path).unwrap_or(false) {
                                    // A container location alone does not prove regenerable contents.
                                    emitter.emit(
                                        targets,
                                        enumeration,
                                        Candidate::new(
                                            path,
                                            &child.name,
                                            &format!("{relative}/{}/{}", child.name, suffix),
                                        ),
                                        &bindings,
                                        false,
                                    );
                                }
                            }
                        }
                    }
                    Selection::NamedDirectories { name, child_depth } => {
                        for child in children {
                            if child.name == *name && child.directory {
                                if enumeration.directory(&anchor, &child.path).unwrap_or(false) {
                                    emitter.emit(
                                        targets,
                                        enumeration,
                                        Candidate::new(child.path, name, &relative),
                                        &bindings,
                                        true,
                                    );
                                }
                            } else if child.directory && *child_depth == 1 {
                                let dir = child.path.join(name);
                                if enumeration.directory(&anchor, &dir).unwrap_or(false) {
                                    emitter.emit(
                                        targets,
                                        enumeration,
                                        Candidate::new(
                                            dir,
                                            name,
                                            &format!("{relative}/{}", child.name),
                                        ),
                                        &bindings,
                                        true,
                                    );
                                }
                            }
                        }
                    }
                    Selection::NamedFiles { name, child_depth } => {
                        for child in children {
                            if child.name == *name && !child.directory {
                                emitter.emit(
                                    targets,
                                    enumeration,
                                    Candidate::new(child.path, name, &relative),
                                    &bindings,
                                    true,
                                );
                            } else if child.directory && *child_depth == 1 {
                                let file = child.path.join(name);
                                if facts::confined_file(&anchor, &file).unwrap_or(false) {
                                    emitter.emit(
                                        targets,
                                        enumeration,
                                        Candidate::new(
                                            file,
                                            name,
                                            &format!("{relative}/{}", child.name),
                                        ),
                                        &bindings,
                                        true,
                                    );
                                }
                            }
                        }
                    }
                    Selection::CatalogChildren {
                        catalog_policy,
                        leaf_policy,
                    } => {
                        for child in children.iter().filter(|child| child.directory) {
                            // 内容签名先于名字：命中叶子词汇的目录只收叶子，
                            // 不能因为目录名也在派发表里就把宿主本体收进来。
                            if let Some(policy) = leaf_policy {
                                let leaves = leaf_signature(&policy.signature, &child.path);
                                if !leaves.is_empty() {
                                    let category = CategoryId::from_rule(&policy.category)
                                        .expect("validated category");
                                    for leaf in leaves {
                                        let trail = crate::core::categories::chromium::leaf_trail(
                                            &child.path,
                                            &leaf,
                                        );
                                        let Some(leaf_name) = trail.last().cloned() else {
                                            continue;
                                        };
                                        let trail = trail.join(" · ");
                                        let render = |template: &str| {
                                            template
                                                .replace("{name}", &child.name)
                                                .replace("{trail}", &trail)
                                        };
                                        emitter.push_target(
                                            targets,
                                            enumeration,
                                            leaf,
                                            Text::new(render(&policy.zh), render(&policy.en)),
                                            category,
                                            crate::core::categories::chromium::leaf_recommended(
                                                &leaf_name,
                                            ),
                                        );
                                    }
                                    continue;
                                }
                            }
                            if let Some((policy, row)) =
                                catalog_policy.iter().find_map(|(name, policy)| {
                                    rule.catalogs
                                        .get(name)
                                        .and_then(|rows| {
                                            rows.iter().find(|row| row.path == child.name)
                                        })
                                        .map(|row| (policy, row))
                                })
                            {
                                emitter.push_target(
                                    targets,
                                    enumeration,
                                    child.path.clone(),
                                    Text::new(row.zh.as_str(), row.en.as_str()),
                                    CategoryId::from_rule(&policy.category)
                                        .expect("validated category"),
                                    policy.recommended,
                                );
                                continue;
                            }
                            // 认不出的目录：整目录展示、不预选，兄弟不陪葬。
                            emitter.emit(
                                targets,
                                enumeration,
                                Candidate::new(
                                    child.path.clone(),
                                    &child.name,
                                    &format!("{relative}/{}", child.name),
                                ),
                                &bindings,
                                true,
                            );
                        }
                    }
                }
            }
        }
    }
}
struct Emitter<'a> {
    snapshot: &'a Arc<RuleSnapshot>,
    rule: &'a RuleDefinition,
    entry: &'a DirectoryRule,
    anchor: &'a Path,
    observation: &'a Arc<RuleObservation>,
}
/// One selected candidate and the label context it was selected under.
struct Candidate {
    path: PathBuf,
    name: String,
    relative: String,
}
impl Candidate {
    fn new(path: PathBuf, name: &str, relative: &str) -> Self {
        Self {
            path,
            name: name.to_string(),
            relative: relative.to_string(),
        }
    }
}
impl Emitter<'_> {
    fn emit(
        &self,
        targets: &mut Vec<ScanTarget>,
        enumeration: &mut Enumeration,
        candidate: Candidate,
        bindings: &[Binding],
        recommended: bool,
    ) {
        let Self { entry, .. } = self;
        let Candidate {
            path,
            name,
            relative,
        } = candidate;
        // `{parent}` is the directory holding the candidate: named-directory
        // selectors label `Firefox · <profile> · cache2`.
        let parent = path
            .parent()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let render = |template: &str, en: bool| {
            let value = template
                .replace("{name}", &name)
                .replace("{parent}", &parent)
                .replace("{relative}", &relative);
            bindings
                .iter()
                .fold(value, |value, binding| binding.render(&value, en))
        };
        self.push_target(
            targets,
            enumeration,
            path,
            Text::new(render(&entry.zh, false), render(&entry.en, true)),
            CategoryId::from_rule(&entry.category).expect("validated category"),
            entry.recommended && recommended,
        );
    }
    /// Catalog/leaf dispatch targets carry their own label, category and
    /// recommendation from the declared policy instead of the entry shape.
    fn push_target(
        &self,
        targets: &mut Vec<ScanTarget>,
        enumeration: &mut Enumeration,
        path: PathBuf,
        label: Text,
        category: CategoryId,
        recommended: bool,
    ) {
        let Self {
            snapshot,
            rule,
            entry,
            anchor,
            observation,
        } = self;
        let kind = match &entry.select {
            Selection::Children {
                directories_only: false,
                ..
            } => ExpectedKind::Any,
            Selection::Children {
                directories_only: true,
                ..
            }
            | Selection::ContainerDirectories { .. }
            | Selection::CatalogChildren { .. } => ExpectedKind::Directory,
            Selection::NamedFiles { .. } | Selection::Files { .. } => ExpectedKind::File,
            Selection::ManifestChildren { .. }
            | Selection::OrphanedChildren { .. }
            | Selection::NamedDirectories { .. } => ExpectedKind::Directory,
            Selection::Path { directories_only } => {
                if *directories_only {
                    ExpectedKind::Directory
                } else {
                    ExpectedKind::File
                }
            }
        };
        enumeration.emit_target(
            targets,
            ScanTarget {
                path,
                label,
                category,
                operation: entry.operation.operation(),
                disposal: entry.disposal,
                recommended,
                size_hint: None,
                rule: RuleRef {
                    snapshot: (*snapshot).clone(),
                    id: rule.id.clone(),
                    scope: Some(anchor.to_path_buf()),
                    contributors: Vec::new(),
                    blocked: None,
                    observation: Some((*observation).clone()),
                },
            },
            kind,
        );
    }
}
/// Every declared root is captured once per scan; a missing root stays absent.
pub(super) fn roots(
    home: Option<&Path>,
    roaming: Option<&Path>,
    user_temp: Option<&Path>,
) -> BTreeMap<String, Option<PathBuf>> {
    BTreeMap::from([
        ("home".to_string(), home.map(Path::to_path_buf)),
        ("roaming".to_string(), roaming.map(Path::to_path_buf)),
        ("user_temp".to_string(), user_temp.map(Path::to_path_buf)),
    ])
}
/// Diagnostic-only fixture roots; the result cannot be loaded as deletion authority.
pub fn explain_at(snapshot: &Arc<RuleSnapshot>, id: &str, root: &Path) -> serde_json::Value {
    let mut targets = Vec::new();
    let mut enumeration = Enumeration::default();
    append_rule(
        &mut targets,
        snapshot,
        snapshot.definition(id),
        &roots(Some(root), Some(root), Some(&root.join("user-temp"))),
        &mut enumeration,
        None,
    );
    let plans: Vec<_> = targets
        .into_iter()
        .map(|target| {
            let planned = super::PlannedTarget {
                operation: target.operation.for_scanned_path(&target.path),
                identity: crate::core::model::capture_identity(&target.path),
                path: target.path,
                disposal: target.disposal,
            };
            let mut plan = super::CleanupPlan::new(target.rule, vec![planned]);
            if let Err(reason) = plan.validate() {
                plan.blocked.push(reason);
            }
            let mut explanation = plan.explanation();
            explanation["recommended"] = serde_json::json!(target.recommended);
            explanation
        })
        .collect();
    serde_json::json!({"directory_reads": enumeration.reads, "inventory_entries": enumeration.entries, "manifest_reads": enumeration.manifest_reads, "manifest_rows": enumeration.manifest_rows, "directory_probes": enumeration.probes, "probe_limit": PROBE_LIMIT, "budget_blocked": enumeration.budget_blocked, "candidate_checks": enumeration.candidate_checks, "candidate_budget_blocked": enumeration.candidate_budget_blocked, "plans": plans})
}

#[cfg(test)]
pub(crate) fn append_fixture(
    targets: &mut Vec<ScanTarget>,
    id: &str,
    home: &Path,
    parent: Option<&Path>,
) {
    let snapshot = super::current();
    append_rule(
        targets,
        &snapshot,
        snapshot.definition("macos"),
        &roots(
            Some(home),
            None,
            parent.map(|path| path.join("user-temp")).as_deref(),
        ),
        &mut Enumeration::default(),
        Some(id),
    );
}

/// Fixture entry point: the same selector production uses, against explicit roots.
#[cfg(test)]
pub(crate) fn scan_at(
    snapshot: &Arc<RuleSnapshot>,
    id: &str,
    roots: &BTreeMap<String, Option<PathBuf>>,
) -> (Vec<ScanTarget>, usize) {
    let mut targets = Vec::new();
    let mut enumeration = Enumeration::default();
    append_rule(
        &mut targets,
        snapshot,
        snapshot.definition(id),
        roots,
        &mut enumeration,
        None,
    );
    (targets, enumeration.reads)
}

/// Fixture roots for a home/roaming pair; production resolves the real user roots.
#[cfg(test)]
pub(crate) fn fixture_roots(home: &Path, roaming: &Path) -> BTreeMap<String, Option<PathBuf>> {
    roots(Some(home), Some(roaming), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scan(
        snapshot: &Arc<RuleSnapshot>,
        home: &Path,
        parent: Option<&Path>,
    ) -> (Vec<ScanTarget>, usize) {
        let user_temp = parent.map(|path| path.join("user-temp"));
        scan_at(
            snapshot,
            "macos",
            &roots(Some(home), None, user_temp.as_deref()),
        )
    }
    fn write(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"isolated").unwrap();
    }

    /// 同一夹具多次扫描：枚举/读取次数逐次一致且与目标数无关——声明式布局的
    /// 共享探测不随运行次数累积，也不产生重复全盘扫描。
    #[test]
    fn repeated_discovery_reads_shared_inventories_once_per_run() {
        let root = crate::core::testing::fixture("directory_layout_repeat");
        let home = root.join("home");
        let parent = root.join("temp");
        for directory in ["Notion", "OneDrive"] {
            std::fs::create_dir_all(home.join("Library/Logs").join(directory)).unwrap();
        }
        for directory in ["C/com.apple.dns", "T/com.apple.quicklook.worker"] {
            std::fs::create_dir_all(parent.join(directory)).unwrap();
        }
        let snapshot = super::super::snapshot();
        let mut baseline: Option<(Vec<String>, usize)> = None;
        for _ in 0..3 {
            let (targets, reads) = scan(&snapshot, &home, Some(&parent));
            let mut paths: Vec<String> = targets
                .iter()
                .map(|target| crate::core::safety::norm(&target.path))
                .collect();
            paths.sort();
            assert!(reads > 0 && reads <= 512, "读取次数有界：{reads}");
            match &baseline {
                None => baseline = Some((paths, reads)),
                Some((expected_paths, expected_reads)) => {
                    assert_eq!(&paths, expected_paths, "目标集合逐次一致");
                    assert_eq!(reads, *expected_reads, "读取次数逐次一致，不累积");
                }
            }
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn directory_layouts_preserve_baseline_and_share_reads() {
        let root = crate::core::testing::fixture("directory_layout_baseline");
        let home = root.join("home");
        let parent = root.join("temp");
        for directory in [
            "Notion",
            "DiagnosticReports",
            "OneDrive",
            "com.apple.CloudTelemetry",
            "Telemetry",
        ] {
            std::fs::create_dir_all(home.join("Library/Logs").join(directory)).unwrap();
        }
        for file in [
            "plain.log",
            "data.bin",
            "Telemetry/state.db",
            "Telemetry/state.db-wal",
        ] {
            write(&home.join("Library/Logs").join(file));
        }
        for directory in [
            "C/com.apple.dns",
            "C/com.apple.quicklook",
            "C/unrelated",
            "T/com.apple.networkd",
            "T/com.apple.quicklook.worker",
        ] {
            std::fs::create_dir_all(parent.join(directory)).unwrap();
        }
        write(&parent.join("C/com.apple.dns-file"));
        write(&home.join("Desktop/.DS_Store"));
        write(&home.join("Desktop/project/.DS_Store"));
        write(&home.join("Desktop/project/deep/.DS_Store"));
        std::fs::create_dir_all(home.join("Documents/.DS_Store")).unwrap();
        for suffix in ["Library/Caches", "Library/tmp", "Library/Logs"] {
            std::fs::create_dir_all(
                home.join("Library/Group Containers/group.example")
                    .join(suffix),
            )
            .unwrap();
            std::fs::create_dir_all(
                home.join("Library/Group Containers/TEAM.1Password")
                    .join(suffix),
            )
            .unwrap();
        }
        // Firefox 的 profile 缓存：有 cache2 的进表，没有的不进。
        write(
            &home.join(
                "Library/Application Support/Firefox/Profiles/abc.default-release/cache2/data",
            ),
        );
        std::fs::create_dir_all(
            home.join("Library/Application Support/Firefox/Profiles/xyz.profile-no-cache"),
        )
        .unwrap();
        let snapshot = super::super::snapshot();
        let (targets, reads) = scan(&snapshot, &home, Some(&parent));
        assert_eq!(
            reads, 7,
            "logs, C, T, Desktop, Documents, containers, Firefox profiles; \
             each shared inventory is read once"
        );
        let mut actual: Vec<_> = targets.iter().map(|t| serde_json::json!({
            "path": t.path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"),
            "category": format!("{:?}", t.category),
            "operation": t.operation,
            "disposal": t.disposal,
            "recommended": t.recommended,
        })).collect();
        actual.sort_by_key(|row| row["path"].as_str().unwrap().to_owned());
        let expected: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../rules/fixtures/directory-layout-baseline.json"
        ))
        .unwrap();
        assert_eq!(actual, expected);
        assert!(targets
            .iter()
            .all(|t| Arc::ptr_eq(&t.rule.snapshot, &snapshot)));
        assert!(home.join("Desktop/project/deep/.DS_Store").exists());
        let diagnostic = explain_at(&snapshot, "macos", &home);
        assert_eq!(diagnostic["directory_reads"], 5);
        assert_eq!(diagnostic["plans"].as_array().unwrap().len(), 13);
        assert!(diagnostic["plans"]
            .as_array()
            .unwrap()
            .iter()
            .all(|plan| plan["targets"].as_array().unwrap().len() == 1));
        let (without_user_temp, _) = scan(&snapshot, &home, None);
        assert!(without_user_temp.iter().all(|t| t.path.starts_with(&home)));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn named_directories_stay_precise_and_datadriven() {
        let root = crate::core::testing::fixture("directory_named_directories");
        let home = root.join("home");
        let profiles = home.join("Library/Application Support/Firefox/Profiles");
        write(&profiles.join("abc.default-release/cache2/data"));
        write(&profiles.join("def.dev-edition-default/cache2/data"));
        std::fs::create_dir_all(profiles.join("ghi.empty")).unwrap();
        // 同名的文件、以及形状像缓存的其他目录都不能进表。
        write(&profiles.join("cache2"));
        write(&profiles.join("abc.default-release/cookies.sqlite"));
        write(&profiles.join("abc.default-release/startupCache/data"));
        let original = super::super::snapshot();
        let (targets, _) = scan_at(&original, "macos", &roots(Some(&home), None, None));
        let firefox: Vec<_> = targets
            .iter()
            .filter(|target| target.category == crate::core::categories::CategoryId::BrowserCache)
            .collect();
        assert_eq!(firefox.len(), 2, "{firefox:?}");
        assert!(firefox.iter().all(|target| target.path.ends_with("cache2")
            && target.operation == super::super::Operation::Contents
            && target.recommended));
        assert_eq!(
            firefox[0].label.get(crate::core::i18n::Language::Zh),
            "Firefox · abc.default-release · cache2"
        );
        assert!(profiles
            .join("abc.default-release/cookies.sqlite")
            .is_file());
        for fault in ["depth", "operation", "name"] {
            let mut bundle = original.bundle.clone();
            let rule = bundle.rules.iter_mut().find(|r| r.id == "macos").unwrap();
            let entry = rule
                .directories
                .iter_mut()
                .find(|entry| entry.id == "firefox_profile_caches")
                .unwrap();
            match fault {
                "depth" => {
                    entry.select = Selection::NamedDirectories {
                        name: "cache2".into(),
                        child_depth: 2,
                    }
                }
                "operation" => entry.operation = PathOperation::File,
                "name" => {
                    entry.select = Selection::NamedDirectories {
                        name: "../cache2".into(),
                        child_depth: 1,
                    }
                }
                _ => unreachable!(),
            }
            assert!(bundle.validate().is_err(), "{fault}");
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn directory_rule_only_extension_keeps_old_snapshot_and_exact_files() {
        let root = crate::core::testing::fixture("directory_layout_extension");
        let home = root.join("home");
        write(&home.join("Downloads/isolated/project.marker"));
        write(&home.join("Downloads/isolated/preserved.data"));
        let original = super::super::snapshot();
        let mut bundle = original.bundle.clone();
        let rule = bundle.rules.iter_mut().find(|r| r.id == "macos").unwrap();
        let mut addition = rule
            .directories
            .iter()
            .find(|e| e.id == "finder_metadata")
            .unwrap()
            .clone();
        addition.id = "project_markers".into();
        addition.paths = vec!["Downloads".into()];
        addition.select = Selection::NamedFiles {
            name: "project.marker".into(),
            child_depth: 1,
        };
        addition.zh = "项目标记 · {relative}".into();
        addition.en = "Project marker · {relative}".into();
        rule.version += 1;
        rule.directories.push(addition);
        bundle.validate().unwrap();
        let changed = Arc::new(RuleSnapshot { bundle });
        assert!(scan(&original, &home, None).0.is_empty());
        let targets = scan(&changed, &home, None).0;
        assert_eq!(targets.len(), 1);
        assert_eq!(
            targets[0].path,
            home.join("Downloads/isolated/project.marker")
        );
        assert_eq!(targets[0].operation, super::super::Operation::File);
        assert!(Arc::ptr_eq(&targets[0].rule.snapshot, &changed));
        assert_eq!(
            targets[0].rule.observation.as_ref().unwrap().rule_version,
            changed.definition("macos").version
        );
        assert!(home.join("Downloads/isolated/preserved.data").exists());
        assert!(scan(&original, &home, None).0.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn directory_validation_rejects_escape_script_and_unbounded_modes() {
        let original = super::super::snapshot();
        for fault in [
            "escape",
            "dot",
            "depth",
            "operation",
            "missing_capability",
            "duplicate",
            "prefix",
            "label",
            "count",
        ] {
            let mut bundle = original.bundle.clone();
            let rule = bundle.rules.iter_mut().find(|r| r.id == "macos").unwrap();
            match fault {
                "escape" => rule.directories[0].paths = vec!["../outside".into()],
                "dot" => rule.directories[0].paths = vec![".".into()],
                "depth" => {
                    rule.directories[3].select = Selection::NamedFiles {
                        name: ".DS_Store".into(),
                        child_depth: 2,
                    }
                }
                "operation" => rule.directories[0].operation = PathOperation::Tree,
                "missing_capability" => rule.required.retain(|c| c != "directory_selection"),
                "duplicate" => rule.directories.push(rule.directories[0].clone()),
                "prefix" => {
                    rule.directories[1].select = Selection::Children {
                        directories_only: true,
                        prefixes: vec!["../".into()],
                    }
                }
                "label" => rule.directories[0].zh = "{arbitrary}".into(),
                "count" => rule.directories[0].paths = vec!["Library/Logs".into(); 33],
                _ => unreachable!(),
            }
            assert!(bundle.validate().is_err(), "{fault}");
        }
        let mut value = serde_json::to_value(&original.bundle).unwrap();
        let macos = value["rules"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r["id"] == "macos")
            .unwrap();
        macos["directories"][0]["select"]["command"] = "powershell arbitrary".into();
        assert!(super::super::RuleBundle::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    #[test]
    fn catalog_children_reject_undeclared_unbounded_and_ambiguous_dispatch() {
        use super::{CatalogPolicy, LeafPolicy};
        let policy = |category: &str| CatalogPolicy {
            category: category.into(),
            recommended: true,
        };
        let original = super::super::snapshot();
        for fault in [
            "unknown_catalog",
            "bad_category",
            "bad_signature",
            "leaf_category",
            "leaf_label",
            "trail_in_fallback",
            "static_fallback",
            "duplicate",
            "operation",
            "script",
        ] {
            let mut bundle = original.bundle.clone();
            if fault != "script" {
                let rule = bundle.rules.iter_mut().find(|r| r.id == "cache").unwrap();
                let entry = rule
                    .directories
                    .iter_mut()
                    .find(|entry| entry.id == "home_cache")
                    .unwrap();
                match fault {
                    "unknown_catalog" => {
                        entry.select = Selection::CatalogChildren {
                            catalog_policy: BTreeMap::from([(
                                "nobody".into(),
                                policy("PackageCache"),
                            )]),
                            leaf_policy: None,
                        }
                    }
                    "bad_category" => {
                        entry.select = Selection::CatalogChildren {
                            catalog_policy: BTreeMap::from([(
                                "agents".into(),
                                policy("NoSuchCategory"),
                            )]),
                            leaf_policy: None,
                        }
                    }
                    "bad_signature" => {
                        entry.select = Selection::CatalogChildren {
                            catalog_policy: BTreeMap::new(),
                            leaf_policy: Some(LeafPolicy {
                                signature: "electron".into(),
                                category: "UserCache".into(),
                                zh: "~/.cache/{name} · {trail}".into(),
                                en: "~/.cache/{name} · {trail}".into(),
                            }),
                        }
                    }
                    "leaf_category" => {
                        entry.select = Selection::CatalogChildren {
                            catalog_policy: BTreeMap::new(),
                            leaf_policy: Some(LeafPolicy {
                                signature: "chromium".into(),
                                category: "NoSuchCategory".into(),
                                zh: "~/.cache/{name} · {trail}".into(),
                                en: "~/.cache/{name} · {trail}".into(),
                            }),
                        }
                    }
                    "leaf_label" => {
                        entry.select = Selection::CatalogChildren {
                            catalog_policy: BTreeMap::new(),
                            leaf_policy: Some(LeafPolicy {
                                signature: "chromium".into(),
                                category: "UserCache".into(),
                                zh: "{arbitrary}".into(),
                                en: "~/.cache/{name} · {trail}".into(),
                            }),
                        }
                    }
                    "trail_in_fallback" => entry.zh = "~/.cache/{trail}".into(),
                    "static_fallback" => entry.zh = "~/.cache".into(),
                    "duplicate" => {
                        rule.catalogs
                            .get_mut("agents")
                            .unwrap()
                            .push(crate::core::rules::Layout {
                                path: "uv".into(),
                                zh: "重复".into(),
                                en: "duplicate".into(),
                                children: Vec::new(),
                            });
                    }
                    "operation" => entry.operation = PathOperation::Tree,
                    _ => unreachable!(),
                }
            } else {
                let mut value = serde_json::to_value(&bundle).unwrap();
                let cache = value["rules"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|r| r["id"] == "cache")
                    .unwrap();
                cache["directories"][0]["select"]["command"] = "powershell arbitrary".into();
                assert!(
                    super::super::RuleBundle::parse(&serde_json::to_vec(&value).unwrap()).is_err()
                );
                continue;
            }
            assert!(bundle.validate().is_err(), "{fault}");
        }
        // 未改动的声明保持通过。
        let mut bundle = original.bundle.clone();
        let rule = bundle.rules.iter_mut().find(|r| r.id == "cache").unwrap();
        rule.version += 1;
        bundle.validate().unwrap();
    }
    #[test]
    fn path_templates_stay_declarative_and_bounded() {
        let original = super::super::snapshot();
        for fault in [
            "unknown",
            "partial",
            "reserved",
            "expansion",
            "tokens",
            "depth",
            "label",
            "operation",
        ] {
            let mut bundle = original.bundle.clone();
            let rule = bundle
                .rules
                .iter_mut()
                .find(|r| r.id == "development")
                .unwrap();
            let entry = rule
                .directories
                .iter_mut()
                .find(|entry| entry.id == "vscode_task_storage")
                .unwrap();
            match fault {
                "unknown" => entry.paths = vec!["{nobody}/tasks".into()],
                "partial" => entry.paths = vec!["{vscode_hosts}-suffix/tasks".into()],
                "reserved" => entry.paths = vec!["{name}/tasks".into()],
                "expansion" => {
                    let wide: Vec<String> = (0..9).map(|i| format!("host-{i}")).collect();
                    rule.lists.insert("wide_hosts".into(), wide.clone());
                    rule.lists.insert("wide_extensions".into(), wide);
                    entry.paths = vec!["{wide_hosts}/{wide_extensions}/tasks".into()];
                }
                "tokens" => {
                    entry.paths = vec!["{vscode_hosts}/{vscode_extensions}/{vscode_hosts}".into()]
                }
                "depth" => entry.paths = vec!["a/b/c/d/e/f/g/h/i".into()],
                "label" => entry.zh = "{nobody} · {name}".into(),
                "operation" => entry.operation = PathOperation::File,
                _ => unreachable!(),
            }
            assert!(bundle.validate().is_err(), "{fault}");
        }
        let mut bundle = original.bundle.clone();
        let rule = bundle
            .rules
            .iter_mut()
            .find(|r| r.id == "development")
            .unwrap();
        rule.lists.insert(
            "collision".into(),
            vec!["../escape".into(), "name/with/separators".into()],
        );
        rule.directories
            .iter_mut()
            .find(|entry| entry.id == "agent_log_databases")
            .unwrap()
            .paths = vec!["{collision}".into()];
        assert!(
            bundle.validate().is_err(),
            "template values may not smuggle separators or traversal"
        );
    }
    #[test]
    fn manifest_selections_stay_literal_and_bounded() {
        let original = super::super::snapshot();
        for fault in [
            "name",
            "pointer",
            "pointer_escape",
            "prefix",
            "scope",
            "operation",
            "label",
            "flag_operation",
        ] {
            let mut bundle = original.bundle.clone();
            let rule = bundle
                .rules
                .iter_mut()
                .find(|r| r.id == "development")
                .unwrap();
            let entry = rule
                .directories
                .iter_mut()
                .find(|entry| entry.id == "orphaned_editor_workspaces")
                .unwrap();
            match fault {
                "name" => {
                    entry.select = Selection::OrphanedChildren {
                        manifest: "../workspace.json".into(),
                        pointer: "/folder".into(),
                        uri_prefix: "file://".into(),
                        scope: DirectoryRoot::Home,
                    }
                }
                "pointer" => {
                    entry.select = Selection::OrphanedChildren {
                        manifest: "workspace.json".into(),
                        pointer: "folder".into(),
                        uri_prefix: "file://".into(),
                        scope: DirectoryRoot::Home,
                    }
                }
                "pointer_escape" => {
                    entry.select = Selection::OrphanedChildren {
                        manifest: "workspace.json".into(),
                        pointer: "/a~0b".into(),
                        uri_prefix: "file://".into(),
                        scope: DirectoryRoot::Home,
                    }
                }
                "prefix" => {
                    entry.select = Selection::OrphanedChildren {
                        manifest: "workspace.json".into(),
                        pointer: "/folder".into(),
                        uri_prefix: "{uri}".into(),
                        scope: DirectoryRoot::Home,
                    }
                }
                "scope" => {
                    entry.select = Selection::OrphanedChildren {
                        manifest: "workspace.json".into(),
                        pointer: "/folder".into(),
                        uri_prefix: "file://".into(),
                        scope: DirectoryRoot::UserTempParent,
                    }
                }
                "operation" => entry.operation = PathOperation::Contents,
                "label" => {
                    // `{project}` 只在读取工作区清单的选择器里有值。
                    let entry = rule
                        .directories
                        .iter_mut()
                        .find(|entry| entry.id == "vscode_task_storage")
                        .unwrap();
                    entry.zh = "{vscode_hosts} · {project}".into();
                }
                "flag_operation" => {
                    let entry = rule
                        .directories
                        .iter_mut()
                        .find(|entry| entry.id == "obsolete_editor_extensions")
                        .unwrap();
                    entry.operation = PathOperation::Contents;
                }
                _ => unreachable!(),
            }
            assert!(bundle.validate().is_err(), "{fault}");
        }
        let mut bundle = original.bundle.clone();
        let rule = bundle
            .rules
            .iter_mut()
            .find(|r| r.id == "development")
            .unwrap();
        rule.directories
            .iter_mut()
            .find(|entry| entry.id == "orphaned_editor_workspaces")
            .unwrap()
            .zh = "孤立工作区 · {vscode_hosts} · {project}".into();
        assert!(
            bundle.validate().is_ok(),
            "the recorded project path is a declared token for orphaned children"
        );
    }
    #[test]
    fn manifest_reads_and_rows_share_the_session_budget() {
        let root = crate::core::testing::fixture("directory_manifest_budget");
        let home = root.join("home");
        std::fs::create_dir_all(home.join("nested")).unwrap();
        std::fs::write(
            home.join(".obsolete"),
            br#"{"pub.old-1.0.0":true,"pub.kept-2.0.0":false}"#,
        )
        .unwrap();
        let mut enumeration = Enumeration::default();
        assert!(enumeration.charge(ENTRY_LIMIT - 1));
        assert!(
            !enumeration.charge(2),
            "an over-budget manifest grants nothing at all"
        );
        // Bound the read itself: a directory is not a manifest, and a link never is either.
        assert!(enumeration.manifest(&home, &home.join("nested")).is_none());
        assert_eq!(enumeration.manifest_reads, 1);
        let manifest = enumeration
            .manifest(&home, &home.join(".obsolete"))
            .expect("fixture manifest");
        assert_eq!(manifest["pub.old-1.0.0"], serde_json::Value::Bool(true));
        let reads = enumeration.manifest_reads;
        assert!(enumeration
            .manifest(&home, &home.join(".obsolete"))
            .is_some());
        assert_eq!(
            enumeration.manifest_reads, reads,
            "the same manifest is read once per session"
        );
        let mut exhausted = Enumeration {
            manifest_reads: PROBE_LIMIT,
            ..Default::default()
        };
        assert!(exhausted.manifest(&home, &home.join(".obsolete")).is_none());
        assert_eq!(exhausted.manifest_reads, PROBE_LIMIT);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn templates_expand_declared_sources_once_per_scan() {
        let root = crate::core::testing::fixture("directory_template_expansion");
        let home = root.join("home");
        let roaming = root.join("roaming");
        let tasks = |host: &str, extension: &str| {
            roaming
                .join(host)
                .join("User/globalStorage")
                .join(extension)
                .join("tasks")
        };
        std::fs::create_dir_all(tasks("Code", "saoudrizwan.claude-dev")).unwrap();
        std::fs::create_dir_all(tasks("Trae", "kilocode.kilo-code")).unwrap();
        std::fs::create_dir_all(tasks("Code", "example.undeclared")).unwrap();
        let snapshot = super::super::snapshot();
        let (targets, reads) = scan_at(&snapshot, "development", &fixture_roots(&home, &roaming));
        let storage: Vec<_> = targets
            .iter()
            .filter(|target| target.operation == super::super::Operation::Contents)
            .collect();
        assert_eq!(storage.len(), 2, "{targets:?}");
        assert!(storage
            .iter()
            .all(|target| target.path.ends_with("tasks") && !target.recommended));
        assert!(
            !targets
                .iter()
                .any(|target| target.path == tasks("Code", "example.undeclared")),
            "an undeclared extension stays out of the layout"
        );
        assert_eq!(
            reads, 0,
            "a declared path is probed, not enumerated: no inventory is read"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn incomplete_directory_inventory_never_grants_partial_targets() {
        let root = crate::core::testing::fixture("directory_budget");
        std::fs::create_dir_all(root.join("Library/Logs")).unwrap();
        let mut enumeration = Enumeration {
            entries: ENTRY_LIMIT,
            ..Default::default()
        };
        assert!(enumeration
            .children(&root, &root.join("Library/Logs"))
            .is_none());
        assert_eq!(enumeration.reads, 0);
        assert!(enumeration.children(&root, &root.join("missing")).is_none());
        write(&root.join("Library/Logs/first.log"));
        write(&root.join("Library/Logs/second.log"));
        let mut partial = Enumeration {
            entries: ENTRY_LIMIT - 1,
            ..Default::default()
        };
        assert!(
            partial
                .children(&root, &root.join("Library/Logs"))
                .is_none(),
            "even a successfully read first entry cannot survive an incomplete inventory"
        );
        assert_eq!(partial.reads, 1);
        assert!(partial
            .children(&root, &root.join("Library/Logs"))
            .is_none());
        assert_eq!(
            partial.reads, 1,
            "unknown inventory is shared rather than retried by each selector"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn directory_probe_budget_covers_empty_missing_and_duplicate_roots() {
        let root = crate::core::testing::fixture("directory_probe_budget");
        let mut enumeration = Enumeration::default();
        for index in 0..PROBE_LIMIT {
            let path = root.join(format!("scope-{index}"));
            if index % 2 == 0 {
                std::fs::create_dir_all(&path).unwrap();
            }
            let result = enumeration.children(&root, &path);
            assert_eq!(result.is_some(), index % 2 == 0);
        }
        assert_eq!(enumeration.probes.len(), PROBE_LIMIT);
        assert_eq!(enumeration.reads, PROBE_LIMIT / 2);
        assert!(enumeration.children(&root, &root.join("scope-0")).is_some());
        assert!(enumeration.children(&root, &root.join("scope-1")).is_none());
        assert_eq!(enumeration.reads, PROBE_LIMIT / 2);
        let blocked = root.join("beyond-budget");
        std::fs::create_dir_all(&blocked).unwrap();
        assert!(enumeration.children(&root, &blocked).is_none());
        assert_eq!(enumeration.probes.len(), PROBE_LIMIT);
        assert_eq!(enumeration.budget_blocked, 1);
        assert!(blocked.is_dir());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn unknown_inventory_and_container_scope_remain_explainable_and_bounded() {
        let root = crate::core::testing::fixture("directory_unknown");
        let missing = root.join("missing");
        let file = root.join("not-directory");
        write(&file);
        let mut enumeration = Enumeration::default();
        assert!(enumeration.children(&root, &missing).is_none());
        assert!(enumeration.children(&root, &file).is_none());
        assert_eq!(enumeration.probes[&missing].state, facts::Evidence::Absent);
        assert_eq!(enumeration.probes[&file].state, facts::Evidence::Unknown);
        assert!(enumeration.probes[&file].reason.is_some());
        write(&root.join("Library/Logs/known.log"));
        let snapshot = super::super::snapshot();
        let mut targets = Vec::new();
        let mut exhausted = Enumeration {
            candidate_checks: ENTRY_LIMIT,
            ..Default::default()
        };
        append_rule(
            &mut targets,
            &snapshot,
            snapshot.definition("macos"),
            &roots(Some(&root), None, None),
            &mut exhausted,
            Some("logs"),
        );
        assert!(targets.is_empty());
        assert_eq!(exhausted.candidate_budget_blocked, 1);
        let diagnostic = explain_at(&snapshot, "macos", &root);
        assert!(diagnostic["directory_probes"]
            .as_object()
            .unwrap()
            .values()
            .any(|probe| probe["state"] == "Absent"));
        assert_eq!(diagnostic["probe_limit"], PROBE_LIMIT);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn container_paths_follow_configuration_without_claiming_parent_or_recommending_unknown_contents(
    ) {
        let root = crate::core::testing::fixture("container_layout_extension");
        let container = root.join("Library/Group Containers/group.example");
        std::fs::create_dir_all(container.join("Library/Alternative")).unwrap();
        write(&container.join("account.data"));
        let original = super::super::snapshot();
        let mut bundle = original.bundle.clone();
        let rule = bundle.rules.iter_mut().find(|r| r.id == "macos").unwrap();
        let entry = rule
            .directories
            .iter_mut()
            .find(|e| e.id == "group_caches")
            .unwrap();
        entry.recommended = true;
        entry.select = Selection::ContainerDirectories {
            paths: vec!["Library/Alternative".into()],
            exclude_name_policy: "sensitive_group".into(),
        };
        bundle.validate().unwrap();
        let snapshot = Arc::new(RuleSnapshot { bundle });
        let (targets, _) = scan(&snapshot, &root, None);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, container.join("Library/Alternative"));
        assert!(
            !targets[0].recommended,
            "a layout does not prove regenerable contents"
        );
        assert!(container.join("account.data").is_file());
        assert!(scan(&original, &root, None).0.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cached_directory_probe_does_not_authorize_a_replacement_file() {
        let root = crate::core::testing::fixture("container_type_replacement");
        let leaf = root.join("Library/Group Containers/group.example/Library/Caches");
        std::fs::create_dir_all(&leaf).unwrap();
        let mut enumeration = Enumeration::default();
        assert!(enumeration.directory(&root, &leaf).unwrap());
        std::fs::remove_dir(&leaf).unwrap();
        write(&leaf);
        let snapshot = super::super::snapshot();
        let mut targets = Vec::new();
        append_rule(
            &mut targets,
            &snapshot,
            snapshot.definition("macos"),
            &roots(Some(&root), None, None),
            &mut enumeration,
            Some("group_caches"),
        );
        assert!(targets.is_empty());
        assert!(leaf.is_file());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn directory_links_never_expand_layouts() {
        let root = crate::core::testing::fixture("directory_links");
        let home = root.join("home");
        let external = root.join("outside");
        write(&external.join(".DS_Store"));
        std::fs::create_dir_all(home.join("Desktop")).unwrap();
        let link = home.join("Desktop").join("linked");
        #[cfg(windows)]
        {
            let output = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(&external)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{:?}: {} {}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&external, &link).unwrap();
        assert!(scan(&super::super::snapshot(), &home, None).0.is_empty());
        assert!(external.join(".DS_Store").exists());
        std::fs::remove_dir(&link).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
