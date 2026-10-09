//! Bundled declarative policy selects fixed capabilities; deletion authority remains in safety/cleaner.
#[cfg(any(test, windows))]
pub(crate) mod app_data;
pub mod directories;
pub mod execution;
pub mod facts;
pub mod flow;
mod plan;
pub(crate) mod uninstall;
pub mod variables;
pub mod versions;
pub use plan::{
    CleanupPlan, CompletionCondition, InstallationInstance, NativeKind, OfficialOperation,
    Operation, PlanAction, PlanStep, PlannedTarget, RuleObservation, RuleRef, TargetScope,
};

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, OnceLock};

include!(concat!(env!("OUT_DIR"), "/rule_schema.rs"));
pub const MAX_PACKAGE: usize = 8 * 1024 * 1024;
pub const CAPABILITIES: &[&str] = &[
    "file",
    "providers",
    "source_install",
    "python_module",
    "registration",
    "chromium",
    "build_markers",
    "docker",
    "snapshot",
    "owner",
    "declutter",
    "variables",
    "source_flow",
    "directory_selection",
    "updater_artifacts",
    "version_retention",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleBundle {
    pub schema: u32,
    pub sequence: u64,
    pub rules: Vec<RuleDefinition>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleDefinition {
    pub id: String,
    pub version: u32,
    pub platform: String,
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub lists: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub numbers: BTreeMap<String, u64>,
    #[serde(default)]
    pub provider_policies: BTreeMap<String, ProviderPolicy>,
    #[serde(default)]
    pub entries: Vec<PathRule>,
    #[serde(default)]
    pub directories: Vec<directories::DirectoryRule>,
    #[serde(default)]
    pub version_layouts: Vec<versions::VersionLayout>,
    #[serde(default)]
    pub markers: Vec<BuildMarker>,
    pub app: Option<SourceInstallRule>,
    #[serde(default)]
    pub facts: BTreeMap<String, facts::Probe>,
    #[serde(default)]
    pub variables: BTreeMap<String, variables::Variable>,
    pub detect: Option<facts::Condition>,
    #[serde(default)]
    pub preserve: Vec<String>,
    #[serde(default)]
    pub catalogs: BTreeMap<String, Vec<Layout>>,
    #[serde(default)]
    pub locations: BTreeMap<String, Vec<ResidualLocation>>,
    #[serde(default)]
    pub app_data_aliases: Vec<AppDataAlias>,
    #[serde(default)]
    pub script_uninstallers: Vec<uninstall::ScriptUninstaller>,
    #[serde(default)]
    pub uninstall_data_warnings: Vec<uninstall::RegisteredUninstallWarning>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppDataAlias {
    pub registry_id: String,
    pub name: String,
    pub publisher: String,
    pub directory: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResidualLocation {
    pub path: String,
    pub source: crate::core::apps::ResidualSource,
}

/// Discovery supplies a typed extent; policy cannot expand that extent.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderPolicy {
    pub recommended: bool,
    pub disposal: crate::core::cleaner::Disposal,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub path: String,
    pub zh: String,
    pub en: String,
    #[serde(default)]
    pub children: Vec<LayoutChild>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutChild {
    pub path: String,
    pub recommended: bool,
}
impl RuleDefinition {
    pub fn evidence_at(
        &self,
        root: &std::path::Path,
    ) -> (variables::Values, BTreeMap<String, facts::Evidence>) {
        let variables = variables::evaluate(&self.variables, root);
        let facts = self
            .facts
            .iter()
            .map(|(name, probe)| (name.clone(), probe.evaluate_with(root, &variables)))
            .collect();
        (variables, facts)
    }
    pub fn matches_at(&self, root: &std::path::Path) -> facts::Evidence {
        let Some(detect) = &self.detect else {
            return facts::Evidence::Confirmed;
        };
        let (_, values) = self.evidence_at(root);
        detect.evaluate(&values)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathRule {
    pub root: String,
    pub path: String,
    pub zh: String,
    pub en: String,
    pub category: String,
    pub recommended: bool,
    pub operation: PathOperation,
    pub disposal: crate::core::cleaner::Disposal,
    #[serde(default)]
    pub minimum_age_seconds: u64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PathOperation {
    File,
    Tree,
    Contents,
    Go,
    Pnpm,
    Trash,
}
impl PathOperation {
    pub fn operation(self) -> Operation {
        match self {
            Self::File => Operation::File,
            Self::Tree => Operation::Tree,
            Self::Contents => Operation::Contents,
            Self::Go => Operation::Go,
            Self::Pnpm => Operation::Pnpm,
            Self::Trash => Operation::Trash,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildMarker {
    pub dir: String,
    pub zh: String,
    pub en: String,
    #[serde(default)]
    pub sibling_any: Vec<String>,
}

/// A split source/dependency installation is a reusable mechanism, not an application adapter.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceInstallRule {
    pub actions: Vec<execution::SourceAction>,
    pub completion: execution::SourceCompletion,
    pub name: String,
    pub default_home: String,
    pub source: String,
    pub targets: Vec<String>,
    pub source_file: String,
    pub source_signature: String,
    pub constants: String,
    pub source_owner: String,
    pub stamp: String,
    pub version_pointer: String,
    pub state_dir: String,
    pub state_facts: String,
    pub environment_pointer: String,
    pub environments: String,
    pub environment_name: String,
    pub tools_dir: String,
    pub tool_facts: String,
    pub tool_lock: String,
    pub launchers: Vec<String>,
    pub profiles: String,
    pub module: String,
    pub module_file: String,
    pub module_args: Vec<String>,
    pub home_variable: String,
    pub python_prefix: String,
    pub installer: String,
    pub installer_signature: String,
    pub bootstrap_dir: String,
    pub bootstrap_prefix: String,
    pub bootstrap_extensions: Vec<String>,
    pub gateway_dir: String,
    pub gateway_files: Vec<String>,
    pub task_prefix: String,
    pub path_prefixes: Vec<String>,
    pub environment_variables: Vec<String>,
    pub preserve: Vec<String>,
    pub installer_process: String,
    pub unknown_processes: Vec<String>,
}

fn relative(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && !value.starts_with(['/', '\\'])
        && !value.contains([':', '\0', '*', '?', '\n', '\r'])
        && value.split(['/', '\\']).all(|part| {
            !part.is_empty() && part != "." && part != ".." && !part.ends_with(['.', ' '])
        })
}

fn drive_root(root: &str) -> Option<char> {
    let letter = root.strip_prefix("drive:")?;
    (letter.len() == 1 && letter.as_bytes()[0].is_ascii_uppercase())
        .then(|| letter.as_bytes()[0] as char)
}

fn valid_root(root: &str) -> bool {
    matches!(
        root,
        "home" | "local" | "roaming" | "cache" | "temp" | "user_temp" | "system"
    ) || drive_root(root).is_some()
}

impl RuleBundle {
    pub fn from_directory(path: &std::path::Path, sequence: u64) -> Result<Self, String> {
        let mut paths: Vec<_> = std::fs::read_dir(path)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .collect();
        paths.sort();
        if paths.len() > 4096 {
            return Err("Rule count limit".into());
        }
        let mut rules = Vec::new();
        for path in paths {
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            if bytes.len() > MAX_PACKAGE {
                return Err("Rule size limit".into());
            }
            let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
            rules.push(toml::from_str(text).map_err(|e| format!("{}: {e}", path.display()))?);
        }
        Self::parse(
            &serde_json::to_vec(&Self {
                schema: SCHEMA,
                sequence,
                rules,
            })
            .map_err(|e| e.to_string())?,
        )
    }
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_PACKAGE {
            return Err("Rule package exceeds 8 MiB".into());
        }
        let bundle: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        bundle.validate()?;
        Ok(bundle)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SCHEMA
            || self.sequence == 0
            || self.rules.is_empty()
            || self.rules.len() > 4096
        {
            return Err("Unsupported rule bundle".into());
        }
        let mut ids = BTreeSet::new();
        for rule in &self.rules {
            directories::validate(rule)?;
            versions::validate(rule)?;
            uninstall::validate(rule)?;
            if rule.app_data_aliases.len() > 128
                || rule.app_data_aliases.iter().any(|alias| {
                    rule.platform != "windows"
                        || alias.registry_id.is_empty()
                        || alias.registry_id.len() > 256
                        || alias.registry_id.contains(['/', '\\'])
                        || alias.registry_id.chars().any(char::is_control)
                        || alias.name.trim().is_empty()
                        || alias.name.len() > 256
                        || alias.publisher.trim().is_empty()
                        || alias.publisher.len() > 256
                        || alias.directory.len() > 128
                        || !crate::core::apps::is_safe_app_token(&alias.directory)
                        || !alias
                            .directory
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                })
            {
                return Err("Invalid registered app data alias".into());
            }
            if rule.provider_policies.len() > 128
                || rule.provider_policies.keys().any(|key| {
                    key.is_empty()
                        || key.len() > 128
                        || !key
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                })
                || (!rule.provider_policies.is_empty()
                    && !rule
                        .required
                        .iter()
                        .any(|capability| capability == "providers"))
            {
                return Err("Invalid or undeclared provider policy".into());
            }
            if rule.locations.len() > 64
                || rule
                    .locations
                    .values()
                    .any(|rows| rows.len() > 128 || rows.iter().any(|row| !relative(&row.path)))
            {
                return Err("Invalid residual locations".into());
            }
            if rule.catalogs.len() > 64
                || rule.catalogs.values().any(|rows| {
                    rows.len() > 1024
                        || rows.iter().any(|row| {
                            !relative(&row.path)
                                || row.zh.is_empty()
                                || row.en.is_empty()
                                || row.children.len() > 256
                                || row.children.iter().any(|child| !relative(&child.path))
                        })
                })
            {
                return Err("Invalid layout catalog".into());
            }
            if rule.facts.len() > 256 || rule.preserve.iter().any(|p| !relative(p)) {
                return Err("Invalid evidence/preserve scope".into());
            }
            for probe in rule.facts.values() {
                probe.validate()?;
                if let Some(path) = probe.path_template() {
                    variables::validate_template(path, &rule.variables)?;
                }
                if let facts::Probe::Variable { name, .. } = probe {
                    if !rule.variables.contains_key(name) {
                        return Err("Unknown variable fact".into());
                    }
                }
            }
            variables::validate(&rule.variables)?;
            if !rule.variables.is_empty()
                && !rule
                    .required
                    .iter()
                    .any(|capability| capability == "variables")
            {
                return Err("Undeclared variable capability".into());
            }
            for path in &rule.preserve {
                variables::validate_template(path, &rule.variables)?;
            }
            if let Some(detect) = &rule.detect {
                detect.validate(&rule.facts)?;
            }
            if rule.id.is_empty()
                || !rule
                    .id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
                || !ids.insert(&rule.id)
                || rule.version == 0
                || !matches!(rule.platform.as_str(), "all" | "windows" | "macos")
            {
                return Err(format!("Invalid/duplicate rule: {}", rule.id));
            }
            if rule
                .required
                .iter()
                .any(|c| !CAPABILITIES.contains(&c.as_str()))
            {
                return Err(format!("Unknown capability: {}", rule.id));
            }
            if rule.entries.len() > 4096
                || rule.markers.len() > 256
                || rule.lists.values().any(|list| {
                    list.len() > 4096
                        || list
                            .iter()
                            .any(|s| s.len() > 1024 || s.contains(['\0', '\r', '\n']))
                })
            {
                return Err("Rule resource limit".into());
            }
            for marker in &rule.markers {
                if !relative(&marker.dir)
                    || marker.dir.contains(['/', '\\'])
                    || marker.sibling_any.iter().any(|s| !relative(s))
                    || marker.zh.is_empty()
                    || marker.en.is_empty()
                {
                    return Err("Invalid build marker".into());
                }
            }
            if rule.id == "chromium" {
                for key in ["signature_leaves", "extra_leaves", "strong_leaves"] {
                    if rule.lists.get(key).is_none_or(|list| {
                        list.is_empty()
                            || list.iter().any(|s| !relative(s) || s.contains(['/', '\\']))
                    }) {
                        return Err("Invalid Chromium leaves".into());
                    }
                }
                // Profile names are policy too: browsers and content signatures share them.
                if !["exact", "prefixes", "suffixes"].iter().any(|mode| {
                    rule.lists
                        .get(&format!("profile_{mode}"))
                        .is_some_and(|list| {
                            !list.is_empty()
                                && list.iter().all(|s| relative(s) && !s.contains(['/', '\\']))
                        })
                }) {
                    return Err("Invalid Chromium profile policy".into());
                }
                if rule
                    .numbers
                    .get("signature_min")
                    .is_none_or(|n| !(2..=64).contains(n))
                {
                    return Err("Invalid Chromium signature threshold".into());
                }
            }
            if rule.id == "engine" {
                validate_engine_policy(rule)?;
                if rule.lists.get("providers").is_none_or(|list| {
                    list.iter().any(|p| {
                        !matches!(
                            p.as_str(),
                            "system" | "cache" | "browser" | "development" | "docker" | "macos"
                        )
                    })
                }) {
                    return Err("Invalid provider".into());
                }
                if rule.lists.get("photo_extensions").is_none_or(|list| {
                    list.is_empty()
                        || list
                            .iter()
                            .any(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_alphanumeric()))
                }) {
                    return Err("Invalid photo extension".into());
                }
            }
            if rule.id == "residual-macos" {
                validate_residual_policy(rule)?;
            }
            if rule.id == "cache" {
                validate_cache_policy(rule)?;
            }
            if rule.id == "macos" {
                for key in ["old_ide_roots", "user_login_roots", "system_login_roots"] {
                    if rule
                        .catalogs
                        .get(key)
                        .is_none_or(|rows| rows.is_empty() || rows.len() > 32)
                    {
                        return Err(format!("Missing macOS layout: {key}"));
                    }
                }
                if rule
                    .numbers
                    .get("login_item_min_age_seconds")
                    .is_none_or(|seconds| !(86400..=31536000).contains(seconds))
                {
                    return Err("Invalid login item age".into());
                }
                for key in [
                    "sensitive_cache_exact",
                    "sensitive_cache_prefixes",
                    "sensitive_group_contains",
                ] {
                    if rule.lists.get(key).is_none_or(|values| {
                        values.is_empty()
                            || values.iter().any(|name| {
                                name.trim().is_empty()
                                    || name.contains(['\0', '\n', '\r', '/', '\\'])
                            })
                    }) {
                        return Err(format!("Missing sensitive name policy: {key}"));
                    }
                }
            }
            for entry in &rule.entries {
                let temp_root = entry.root == "user_temp"
                    && entry.path == "."
                    && entry.operation == PathOperation::Contents
                    && rule.variables.is_empty();
                if !temp_root {
                    variables::validate_template(&entry.path, &rule.variables)?;
                }
                let required = match entry.operation {
                    PathOperation::Go | PathOperation::Pnpm => "owner",
                    _ => "file",
                };
                if !rule
                    .required
                    .iter()
                    .any(|capability| capability == required)
                    || (entry.disposal == crate::core::cleaner::Disposal::RecycleBin
                        && !matches!(entry.operation, PathOperation::File | PathOperation::Tree))
                {
                    return Err("Undeclared operation capability or incompatible disposal".into());
                }
                if (!relative(&entry.path) && !temp_root)
                    || !valid_root(&entry.root)
                    || crate::core::categories::CategoryId::from_rule(&entry.category).is_none()
                {
                    return Err(format!("Invalid path rule: {}", rule.id));
                }
            }
            if let Some(app) = &rule.app {
                execution::validate(&app.actions)?;
                if ![
                    "source_install",
                    "python_module",
                    "registration",
                    "source_flow",
                ]
                .iter()
                .all(|capability| rule.required.iter().any(|required| required == capability))
                {
                    return Err("Undeclared installation capability".into());
                }
                if app.module_file != format!("{}.py", app.module.replace('.', "/"))
                    || app.module_args.len() > 32
                    || app.module_args.iter().any(|s| {
                        s.is_empty()
                            || matches!(
                                s.as_str(),
                                "-c" | "-m"
                                    | "--command"
                                    | "--eval"
                                    | "--exec"
                                    | "--script"
                                    | "--code"
                            )
                            || !s.bytes().all(|b| {
                                b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')
                            })
                    })
                    || app.environment_name.is_empty()
                    || !relative(&app.environment_name)
                    || app.environment_name.contains(['/', '\\'])
                    || app.python_prefix.is_empty()
                    || app.bootstrap_prefix.is_empty()
                    || app.installer_process.is_empty()
                {
                    return Err("Invalid fixed Python module parameters".into());
                }
                let paths = [
                    &app.default_home,
                    &app.source,
                    &app.source_file,
                    &app.constants,
                    &app.source_owner,
                    &app.stamp,
                    &app.state_dir,
                    &app.state_facts,
                    &app.environments,
                    &app.tools_dir,
                    &app.tool_facts,
                    &app.tool_lock,
                    &app.profiles,
                    &app.module_file,
                    &app.installer,
                    &app.bootstrap_dir,
                    &app.gateway_dir,
                ];
                if paths.iter().any(|p| !relative(p))
                    || app
                        .targets
                        .iter()
                        .chain(&app.launchers)
                        .chain(&app.gateway_files)
                        .chain(&app.path_prefixes)
                        .chain(&app.preserve)
                        .any(|p| !relative(p))
                    || app.targets.is_empty()
                    || app.source_signature.is_empty()
                    || app.installer_signature.is_empty()
                    || app.module.split('.').any(|s| {
                        s.is_empty() || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    })
                    || app
                        .module_args
                        .iter()
                        .any(|s| s.contains(['\0', '\n', '\r']))
                    || app.task_prefix.is_empty()
                    || app.task_prefix.contains(['*', '?'])
                {
                    return Err(format!("Invalid installation rule: {}", rule.id));
                }
                if app
                    .environment_variables
                    .iter()
                    .chain(std::iter::once(&app.home_variable))
                    .any(|s| {
                        s.is_empty() || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    })
                {
                    return Err("Invalid environment variable".into());
                }
                if app.launchers.iter().any(|p| {
                    app.preserve
                        .iter()
                        .any(|keep| p == keep || p.starts_with(&format!("{keep}/")))
                }) {
                    return Err("Preserved launcher overlap".into());
                }
            }
        }
        for required in [
            "engine",
            "hermes",
            "chromium",
            "build",
            "development",
            "cache",
            "package-macos",
            "browsers",
            "macos",
            "windows",
            "residual-macos",
        ] {
            if !ids.contains(&required.to_owned()) {
                return Err(format!("Missing required rule: {required}"));
            }
        }
        Ok(())
    }
}

fn validate_cache_policy(rule: &RuleDefinition) -> Result<(), String> {
    if !rule
        .required
        .iter()
        .any(|capability| capability == "updater_artifacts")
    {
        return Err("Missing updater artifact capability".into());
    }
    let accepted = [
        "updater_directories_exact",
        "updater_directories_prefixes",
        "updater_files_exact",
        "updater_files_suffixes",
        "updater_display_suffixes",
        "updater_probe_exclude_prefixes",
    ];
    if !rule.required.iter().any(|capability| capability == "file")
        || rule
            .lists
            .keys()
            .any(|key| key.starts_with("updater_") && !accepted.contains(&key.as_str()))
    {
        return Err("Unknown or undeclared updater policy".into());
    }
    for key in [
        "updater_directories_exact",
        "updater_directories_prefixes",
        "updater_files_exact",
        "updater_files_suffixes",
        "updater_display_suffixes",
        "updater_probe_exclude_prefixes",
    ] {
        if rule.lists.get(key).is_none_or(|values| {
            values.is_empty()
                || values.len() > 128
                || values.iter().any(|value| {
                    value.trim().is_empty()
                        || value.len() > 128
                        || value.contains(['/', '\\', ':', '\0', '\r', '\n'])
                })
        }) {
            return Err(format!("Invalid updater policy: {key}"));
        }
    }
    if rule
        .numbers
        .get("updater_stale_seconds")
        .is_none_or(|seconds| !(86400..=31536000).contains(seconds))
    {
        return Err("Invalid updater age".into());
    }
    Ok(())
}

fn validate_engine_policy(rule: &RuleDefinition) -> Result<(), String> {
    for key in [
        "browser_cache",
        "cache_candidate",
        "agent_history",
        "linked_worktree",
        "development_candidate",
        "docker_image",
        "broken_login_item",
        "local_snapshot",
        "old_ide_data",
        "volume_trash",
        "user_trash",
        "updater_artifact",
        "brew_cleanup",
    ] {
        if !rule.provider_policies.contains_key(key) {
            return Err(format!("Missing provider policy: {key}"));
        }
    }
    for (key, minimum, maximum) in [
        ("photo_distance", 0, 5),
        ("photo_burst_distance", 0, 8),
        ("photo_burst_seconds", 0, 10),
        ("photo_min_size", 1, 200000000),
        ("photo_max_size", 1, 200000000),
        ("photo_max_depth", 1, 20),
        ("photo_candidate_limit", 2, 1500),
        ("photo_group_limit", 1, 30),
        ("photo_group_size_limit", 2, 12),
        ("photo_supplement_below", 0, 1500),
        ("duplicate_min_size", 65536, u64::MAX),
        ("duplicate_bucket_limit", 1, 500),
        ("large_result_limit", 1, 500),
        ("large_min_size", 1, u64::MAX),
        ("large_selected_age_days", 1, 36500),
        ("download_max_depth", 1, 4),
        ("default_min_size_filter", 1, u64::MAX),
        ("default_age_filter_months", 0, 1200),
    ] {
        if rule
            .numbers
            .get(key)
            .is_some_and(|value| *value < minimum || *value > maximum)
        {
            return Err(format!("Invalid or excessive policy: {key}"));
        }
    }
    let number = |key: &str, default| rule.numbers.get(key).copied().unwrap_or(default);
    if number("photo_min_size", 10000) > number("photo_max_size", 200000000)
        || number("photo_supplement_below", 500) > number("photo_candidate_limit", 1500)
    {
        return Err("Inconsistent photo policy".into());
    }
    for family in ["download_", "large_", "duplicate_"] {
        let mut extensions = BTreeSet::new();
        for (key, values) in &rule.lists {
            if key.starts_with(family) {
                for extension in values {
                    if extension.is_empty()
                        || !extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
                        || !extensions.insert(extension)
                    {
                        return Err("Invalid or ambiguous file classification".into());
                    }
                }
            }
        }
    }
    for key in ["photo_camera_prefixes", "photo_ignored_directories"] {
        if rule.lists.get(key).is_some_and(|values| {
            values
                .iter()
                .any(|value| !relative(value) || value.contains(['/', '\\']))
        }) {
            return Err("Invalid photo name policy".into());
        }
    }
    Ok(())
}

fn validate_residual_policy(rule: &RuleDefinition) -> Result<(), String> {
    // Remote policy can add exclusions, but cannot weaken this client's shipped ownership fences.
    static BASELINE: OnceLock<RuleBundle> = OnceLock::new();
    let baseline = BASELINE.get_or_init(|| {
        serde_json::from_slice(include_bytes!(concat!(env!("OUT_DIR"), "/rules.json")))
            .expect("embedded rule structure")
    });
    let baseline = baseline
        .rules
        .iter()
        .find(|rule| rule.id == "residual-macos")
        .ok_or("Missing embedded residual safety policy")?;
    for key in ["shared_vendor_prefixes", "protected_dot_dirs"] {
        let values = rule.lists.get(key).ok_or("Missing residual exclusions")?;
        if baseline
            .lists
            .get(key)
            .is_none_or(|required| required.iter().any(|v| !values.contains(v)))
        {
            return Err("Residual policy weakens shipped ownership fences".into());
        }
    }
    for (key, values) in &rule.lists {
        if values.iter().any(|value| match key.as_str() {
            "dot_config_parents" => !value.is_empty() && !relative(value),
            "helper_id_suffixes" | "id_file_suffixes" => {
                !value.starts_with('.') || value.contains(['/', '\\'])
            }
            _ => value.is_empty() || value.contains(['/', '\\']),
        }) {
            return Err("Invalid residual matching policy".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn root_scope_validation_rejects_skeleton_and_drive_escape() {
        let original = super::snapshot().bundle.clone();
        for (root, path, operation) in [
            ("home", ".", super::PathOperation::Contents),
            ("system", ".", super::PathOperation::Contents),
            ("drive:C", ".", super::PathOperation::Contents),
            ("user_temp", ".", super::PathOperation::Tree),
            ("drive:C:", "tmp", super::PathOperation::Contents),
            ("drive:../C", "tmp", super::PathOperation::Contents),
            ("drive:c", "tmp", super::PathOperation::Contents),
        ] {
            let mut bundle = original.clone();
            let rule = bundle
                .rules
                .iter_mut()
                .find(|rule| rule.id == "windows")
                .unwrap();
            let entry = rule
                .entries
                .iter_mut()
                .find(|entry| entry.root == "user_temp")
                .unwrap();
            entry.root = root.into();
            entry.path = path.into();
            entry.operation = operation;
            assert!(bundle.validate().is_err(), "{root}/{path}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn system_rules_keep_temp_scope_and_unknown_user_root_never_falls_back() {
        use super::*;
        use crate::core::cleaner::{CleanProgress, CleanTarget};
        use std::path::PathBuf;
        let fixture = crate::core::testing::fixture("rule_system_roots");
        let temp = fixture.join("foreground-temp");
        std::fs::create_dir(&temp).unwrap();
        std::fs::write(temp.join("sentinel.txt"), b"fixture").unwrap();
        let windows =
            PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:/Windows".into()));
        let mut roots = BTreeMap::from([
            ("system".into(), Some(windows.clone())),
            ("user_temp".into(), Some(temp.clone())),
            ("local".into(), Some(fixture.join("local"))),
            ("drive:C".into(), Some(fixture.join("drive-c"))),
        ]);
        let snapshot = snapshot();
        let mut targets = Vec::new();
        append_path_targets_with_roots(&mut targets, &snapshot, &roots);
        assert_eq!(
            targets
                .iter()
                .filter(|target| target.rule.id == "windows")
                .count(),
            9
        );
        let windows_temp = targets
            .iter()
            .find(|target| target.path == windows.join("Temp"))
            .unwrap();
        assert_eq!(windows_temp.operation, Operation::Contents);
        assert!(!windows_temp.recommended);
        let user_temp = targets.iter().find(|target| target.path == temp).unwrap();
        let identity = crate::core::model::capture_identity(&temp);
        let plan = Arc::new(CleanupPlan::new(
            user_temp.rule.clone(),
            vec![PlannedTarget {
                path: temp.clone(),
                operation: user_temp.operation.clone(),
                identity,
                disposal: user_temp.disposal,
            }],
        ));
        let selected = CleanTarget {
            plans: vec![plan],
            rule: Some(user_temp.rule.clone()),
            path: temp.clone(),
            operation: user_temp.operation.clone(),
            remove_dir: false,
            size_hint: None,
            identity,
            disposal: user_temp.disposal,
        };
        let report = crate::core::cleaner::clean_targets(&[selected], &CleanProgress::default());
        assert_eq!(report.ok, 1);
        assert!(temp.is_dir());
        assert!(!temp.join("sentinel.txt").exists());
        roots.insert("user_temp".into(), None);
        targets.clear();
        append_path_targets_with_roots(&mut targets, &snapshot, &roots);
        assert!(!targets.iter().any(|target| target.path == temp));
        assert!(targets
            .iter()
            .any(|target| target.path == windows.join("Temp")));
        let mut bundle = snapshot.bundle.clone();
        bundle
            .rules
            .iter_mut()
            .find(|rule| rule.id == "windows")
            .unwrap()
            .entries
            .iter_mut()
            .find(|entry| entry.path == "Temp")
            .unwrap()
            .operation = PathOperation::Tree;
        bundle.validate().unwrap();
        targets.clear();
        append_path_targets_with_roots(&mut targets, &Arc::new(RuleSnapshot { bundle }), &roots);
        assert!(!targets
            .iter()
            .any(|target| target.path == windows.join("Temp")));
        std::fs::remove_dir_all(fixture).unwrap();
    }
    #[test]
    fn provider_policy_defaults_match_migration_baseline() {
        let baseline: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../rules/fixtures/provider-policy-baseline.json"
        ))
        .unwrap();
        let snapshot = super::snapshot();
        let policies = &snapshot.definition("engine").provider_policies;
        assert_eq!(baseline.len(), 31);
        for row in baseline {
            let key = row["policy"].as_str().unwrap();
            let expected = super::ProviderPolicy {
                recommended: row["recommended"].as_bool().unwrap(),
                disposal: serde_json::from_value(row["disposal"].clone()).unwrap(),
            };
            assert_eq!(policies.get(key), Some(&expected), "policy: {key}");
        }
    }

    #[test]
    fn provider_policy_validation_rejects_missing_unbounded_and_script_fields() {
        use super::*;
        let bundle = snapshot().bundle.clone();
        for change in ["missing", "undeclared", "key", "budget"] {
            let mut modified = bundle.clone();
            let rule = modified
                .rules
                .iter_mut()
                .find(|rule| rule.id == "engine")
                .unwrap();
            match change {
                "missing" => {
                    rule.provider_policies.remove("docker_image");
                }
                "undeclared" => rule.required.retain(|capability| capability != "providers"),
                "key" => {
                    rule.provider_policies.insert(
                        "../escape".into(),
                        ProviderPolicy {
                            recommended: true,
                            disposal: crate::core::cleaner::Disposal::Permanent,
                        },
                    );
                }
                "budget" => {
                    for index in 0..129 {
                        rule.provider_policies.insert(
                            format!("fixture_{index}"),
                            ProviderPolicy {
                                recommended: true,
                                disposal: crate::core::cleaner::Disposal::Permanent,
                            },
                        );
                    }
                }
                _ => unreachable!(),
            }
            assert!(modified.validate().is_err(), "invalid: {change}");
        }
        let mut json = serde_json::to_value(bundle).unwrap();
        let engine = json["rules"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|rule| rule["id"] == "engine")
            .unwrap();
        engine["provider_policies"]["docker_image"]["command"] =
            "powershell -Command arbitrary".into();
        assert!(RuleBundle::parse(&serde_json::to_vec(&json).unwrap()).is_err());
    }
    #[test]
    fn bundled_snapshot_is_stable_and_scoped_rules_do_not_replace_it() {
        let bundled = snapshot();
        assert!(Arc::ptr_eq(&bundled, &embedded()));
        let mut bundle = bundled.bundle.clone();
        bundle.sequence += 1;
        let scoped = Arc::new(RuleSnapshot { bundle });
        with_snapshot(scoped.clone(), || {
            assert!(Arc::ptr_eq(&current(), &scoped));
            assert!(Arc::ptr_eq(&snapshot(), &bundled));
        });
        assert!(Arc::ptr_eq(&current(), &bundled));
    }
    #[test]
    fn layout_and_artifact_policies_reject_missing_unsafe_or_unbounded_values() {
        let original = snapshot().bundle.clone();
        for fault in [
            "artifact_missing",
            "artifact_name",
            "artifact_budget",
            "artifact_extra",
            "artifact_age",
            "login_age",
            "layout_missing",
            "container_escape",
            "container_policy",
        ] {
            let mut bundle = original.clone();
            let cache = bundle
                .rules
                .iter_mut()
                .find(|rule| rule.id == "cache")
                .unwrap();
            match fault {
                "artifact_missing" => {
                    cache.lists.remove("updater_files_exact");
                }
                "artifact_name" => {
                    cache
                        .lists
                        .get_mut("updater_files_exact")
                        .unwrap()
                        .push("../escape".into());
                }
                "artifact_budget" => {
                    cache
                        .lists
                        .insert("updater_files_suffixes".into(), vec![".zip".into(); 129]);
                }
                "artifact_extra" => {
                    cache
                        .lists
                        .insert("updater_files_prefixes".into(), vec![String::new()]);
                }
                "artifact_age" => {
                    cache.numbers.insert("updater_stale_seconds".into(), 0);
                }
                _ => {
                    let macos = bundle
                        .rules
                        .iter_mut()
                        .find(|rule| rule.id == "macos")
                        .unwrap();
                    match fault {
                        "login_age" => {
                            macos.numbers.insert("login_item_min_age_seconds".into(), 0);
                        }
                        "layout_missing" => {
                            macos.catalogs.remove("old_ide_roots");
                        }
                        "container_escape" | "container_policy" => {
                            let entry = macos
                                .directories
                                .iter_mut()
                                .find(|entry| entry.id == "group_caches")
                                .unwrap();
                            entry.select = directories::Selection::ContainerDirectories {
                                paths: vec![if fault == "container_escape" {
                                    "../outside"
                                } else {
                                    "Library/Caches"
                                }
                                .into()],
                                exclude_name_policy: if fault == "container_policy" {
                                    "missing"
                                } else {
                                    "sensitive_group"
                                }
                                .into(),
                            };
                        }
                        _ => unreachable!(),
                    }
                }
            }
            assert!(bundle.validate().is_err(), "{fault}");
        }
        let cache = snapshot().definition("cache").clone();
        assert_eq!(cache.numbers["updater_stale_seconds"], 7 * 86400);
        let macos = snapshot().definition("macos").clone();
        assert_eq!(macos.numbers["login_item_min_age_seconds"], 86400);
        assert_eq!(
            macos.catalogs["old_ide_roots"][0].path,
            "Library/Application Support/JetBrains"
        );
        assert_eq!(
            macos.catalogs["user_login_roots"][0].path,
            "Library/LaunchAgents"
        );
        assert_eq!(macos.catalogs["system_login_roots"][0].path, "LaunchAgents");
    }

    #[test]
    fn sensitive_name_policy_requires_complete_bounded_lists() {
        let base = embedded().bundle.clone();
        for key in [
            "sensitive_cache_exact",
            "sensitive_cache_prefixes",
            "sensitive_group_contains",
        ] {
            let mut missing = base.clone();
            missing
                .rules
                .iter_mut()
                .find(|rule| rule.id == "macos")
                .unwrap()
                .lists
                .remove(key);
            assert!(missing.validate().is_err());
            let mut invalid = base.clone();
            invalid
                .rules
                .iter_mut()
                .find(|rule| rule.id == "macos")
                .unwrap()
                .lists
                .get_mut(key)
                .unwrap()
                .push(String::new());
            assert!(invalid.validate().is_err());
        }
    }
    use super::*;
    #[test]
    fn explicit_path_policies_match_pre_migration_baseline() {
        let bundle = &embedded().bundle;
        let mut actual = Vec::new();
        for rule in &bundle.rules {
            for entry in &rule.entries {
                let category =
                    crate::core::categories::CategoryId::from_rule(&entry.category).unwrap();
                let path = std::path::Path::new("C:/isolated").join(&entry.path);
                // Trash scope is tied to the actual user/volume, not this synthetic root.
                if entry.operation != PathOperation::Trash {
                    assert_eq!(
                        entry.operation.operation(),
                        Operation::classify(&path, category.removes_directory())
                    );
                }
                assert_eq!(entry.disposal, category.disposal());
                actual.push(serde_json::json!({
                    "rule": rule.id, "root": entry.root, "path": entry.path,
                    "category": entry.category, "recommended": entry.recommended,
                    "operation": entry.operation, "disposal": entry.disposal,
                    "minimum_age_seconds": entry.minimum_age_seconds, "preserve": rule.preserve
                }));
            }
        }
        actual.sort_by_key(|value| {
            (
                value["rule"].as_str().unwrap().to_owned(),
                value["root"].as_str().unwrap().to_owned(),
                value["path"].as_str().unwrap().to_owned(),
            )
        });
        let expected: Vec<serde_json::Value> = serde_json::from_slice(include_bytes!(
            "../../../rules/fixtures/path-policy-baseline.json"
        ))
        .unwrap();
        assert_eq!(actual, expected);
    }

    /// 「仅配置增量」：给内置规则包加一条新声明的路径条目，通用路径能力立刻把它
    /// 展开成清理目标——没有为它加任何专用 Rust 分支。
    #[test]
    fn path_rule_only_fixture_surfaces_a_new_target_without_rust_changes() {
        let root = crate::core::testing::fixture("path_rule_only");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("QuickCleanerFixtureCache")).unwrap();
        let definition: RuleDefinition =
            toml::from_str(include_str!("../../../rules/fixtures/path-extra.toml")).unwrap();
        let mut bundle = embedded().bundle.clone();
        bundle.rules.push(definition);
        bundle.validate().unwrap();
        let snapshot = Arc::new(RuleSnapshot { bundle });
        let roots = BTreeMap::from([("home".to_string(), Some(root.clone()))]);
        let mut targets = Vec::new();
        append_path_targets_with_roots(&mut targets, &snapshot, &roots);
        let found = targets
            .iter()
            .find(|target| target.path == root.join("QuickCleanerFixtureCache"))
            .expect("a newly declared path must surface as a target");
        assert_eq!(
            found.category,
            crate::core::categories::CategoryId::UserCache
        );
        assert_eq!(found.operation, Operation::Contents);
        assert!(found.recommended);
        assert!(Arc::ptr_eq(&found.rule.snapshot, &snapshot));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn path_policy_requires_explicit_fields_and_compatible_capabilities() {
        let base = embedded().bundle.clone();
        let mut old_schema = base.clone();
        old_schema.schema = 1;
        assert!(old_schema.validate().is_err());
        let entry = base.rules.iter().flat_map(|r| &r.entries).next().unwrap();
        for field in ["operation", "disposal"] {
            let mut missing = serde_json::to_value(entry).unwrap();
            missing.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<PathRule>(missing).is_err());
        }
        let mut invalid = base.clone();
        let rule = invalid
            .rules
            .iter_mut()
            .find(|r| !r.entries.is_empty())
            .unwrap();
        rule.required.retain(|capability| capability != "owner");
        rule.entries[0].operation = PathOperation::Go;
        assert!(invalid.validate().is_err());
        let mut invalid = base.clone();
        let rule = invalid
            .rules
            .iter_mut()
            .find(|r| !r.entries.is_empty())
            .unwrap();
        rule.entries[0].operation = PathOperation::Contents;
        rule.entries[0].disposal = crate::core::cleaner::Disposal::RecycleBin;
        assert!(invalid.validate().is_err());
    }
    #[test]
    fn residual_locations_and_exclusions_remain_safe_policy() {
        let baseline = embedded().bundle.clone();
        let rule = baseline
            .rules
            .iter()
            .find(|r| r.id == "residual-macos")
            .unwrap();
        for (key, count) in [
            ("satellite_dirs", 7),
            ("user_family_dirs", 11),
            ("system_family_dirs", 8),
            ("orphan_roots", 9),
        ] {
            assert_eq!(rule.locations[key].len(), count);
        }
        for key in ["shared_vendor_prefixes", "protected_dot_dirs"] {
            let mut altered = baseline.clone();
            altered
                .rules
                .iter_mut()
                .find(|r| r.id == "residual-macos")
                .unwrap()
                .lists
                .get_mut(key)
                .unwrap()
                .remove(0);
            assert!(altered.validate().is_err());
        }
    }
    #[test]
    fn manifest_only_fixture_generates_targets_and_rechecks_membership() {
        let root = crate::core::testing::fixture("manifest_rule");
        std::fs::create_dir_all(root.join("Orion/cache")).unwrap();
        std::fs::write(
            root.join("Orion/manifest.json"),
            br#"{"owner":"orion","members":["cache"]}"#,
        )
        .unwrap();
        std::fs::write(root.join("Orion/cache/sentinel"), b"keep until authorized").unwrap();
        let mut rule: RuleDefinition =
            toml::from_str(include_str!("../../../rules/fixtures/manifest.toml")).unwrap();
        rule.entries[0].category = "UserCache".into();
        let mut bundle = embedded().bundle.clone();
        bundle.rules.push(rule);
        bundle.validate().unwrap();
        let snapshot = Arc::new(RuleSnapshot { bundle });
        let mut targets = Vec::new();
        with_snapshot(snapshot.clone(), || {
            append_path_targets(&mut targets, Some(&root))
        });
        let target = targets
            .iter()
            .find(|target| target.rule.id == "orion-fixture")
            .unwrap();
        assert_eq!(target.path, root.join("Orion/cache"));
        assert_eq!(
            target.operation,
            Operation::Tree,
            "category cannot change the declared operation"
        );
        let plan = CleanupPlan::new(
            target.rule.clone(),
            vec![PlannedTarget {
                path: target.path.clone(),
                operation: target.operation.clone(),
                identity: crate::core::model::capture_identity(&target.path),
                disposal: target.disposal,
            }],
        );
        let explanation = plan.explanation();
        assert!(matches!(
            plan.scope(0),
            Some(TargetScope::ManifestMember { .. })
        ));
        assert_eq!(plan.observations.len(), 1);
        assert!(plan.validate().is_ok());
        std::fs::write(
            root.join("Orion/manifest.json"),
            br#"{"owner":"orion","members":[]}"#,
        )
        .unwrap();
        assert!(plan.validate().is_err());
        assert_eq!(
            plan.explanation(),
            explanation,
            "execution recheck must not rewrite scan facts"
        );
        assert!(root.join("Orion/cache/sentinel").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn policy_cannot_increase_capability_limits_or_inject_script_flags() {
        let base = embedded().bundle.clone();
        for (key, value) in [
            ("photo_distance", 64),
            ("photo_candidate_limit", 1501),
            ("download_max_depth", 100),
        ] {
            let mut bundle = base.clone();
            bundle
                .rules
                .iter_mut()
                .find(|r| r.id == "engine")
                .unwrap()
                .numbers
                .insert(key.into(), value);
            assert!(bundle.validate().is_err());
        }
        let mut bundle = base.clone();
        bundle
            .rules
            .iter_mut()
            .find(|r| r.id == "engine")
            .unwrap()
            .lists
            .get_mut("download_archive")
            .unwrap()
            .push("exe".into());
        assert!(bundle.validate().is_err());
        for flag in ["-c", "--eval", "--command"] {
            let mut bundle = base.clone();
            bundle
                .rules
                .iter_mut()
                .find(|r| r.id == "hermes")
                .unwrap()
                .app
                .as_mut()
                .unwrap()
                .module_args
                .push(flag.into());
            assert!(bundle.validate().is_err());
        }
    }
    #[test]
    fn embedded_and_source_rules_have_identical_policy() {
        let source = RuleBundle::from_directory(
            std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/rules")),
            1,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(source).unwrap(),
            serde_json::to_value(&embedded().bundle).unwrap()
        );
    }

    /// macOS 残留锚点也在规则里，且是**平台无关的配置完整性**——Windows 上就能
    /// 静态核验这份配置不空。缺 key 会让 macOS 扫描器静默产出 0 目标（不报错、
    /// 不失败），是 Windows 构建与 macOS 编译都抓不到的静默回归。
    #[test]
    fn residual_macos_anchors_are_declared() {
        let snapshot = current();
        let rule = snapshot.definition("residual-macos");
        for key in [
            "satellite_dirs",
            "user_family_dirs",
            "system_family_dirs",
            "orphan_roots",
        ] {
            assert!(
                rule.locations.get(key).is_some_and(|rows| !rows.is_empty()),
                "residual-macos 缺少非空 locations.{key}（macOS 扫描会静默产出 0 目标）"
            );
        }
        for key in [
            "helper_id_suffixes",
            "shared_vendor_prefixes",
            "dot_config_parents",
            "protected_dot_dirs",
            "id_file_suffixes",
        ] {
            assert!(
                rule.lists.get(key).is_some_and(|rows| !rows.is_empty()),
                "residual-macos 缺少非空 lists.{key}"
            );
        }
    }
    #[test]
    fn malformed_rules_are_rejected_as_a_whole() {
        let base = embedded().bundle.clone();
        for required in [
            "engine",
            "hermes",
            "chromium",
            "build",
            "development",
            "cache",
            "package-macos",
            "browsers",
            "macos",
            "windows",
        ] {
            let mut bad = base.clone();
            bad.rules.retain(|r| r.id != required);
            assert!(bad.validate().is_err());
        }
        let mut bad = base.clone();
        bad.rules.push(bad.rules[0].clone());
        assert!(bad.validate().is_err());
        let mut bad = base.clone();
        bad.rules[0].required.push("shell".into());
        assert!(bad.validate().is_err());
        for path in [
            "../data",
            "C:/Users/user",
            "cache/..",
            "cache/*",
            "cache/secret.",
        ] {
            let mut bad = base.clone();
            bad.rules[0].entries.push(PathRule {
                root: "home".into(),
                path: path.into(),
                zh: "x".into(),
                en: "x".into(),
                category: "UserCache".into(),
                recommended: false,
                operation: PathOperation::Contents,
                disposal: crate::core::cleaner::Disposal::Permanent,
                minimum_age_seconds: 0,
            });
            assert!(bad.validate().is_err(), "{path}");
        }
    }
    /// 迁进规则的清理名单钉在扫描实际加载的那份快照上。
    /// 名字、预选和「明确不入表」都从这份快照读，不另解析一份期望。
    #[test]
    fn shipped_snapshot_keeps_moved_cleanup_names() {
        let snap = current();
        let pkg = snap.definition("package-macos");
        assert_eq!(pkg.platform, "macos");
        let entry = |path: &str| {
            pkg.entries
                .iter()
                .find(|rule| rule.path == path)
                .unwrap_or_else(|| panic!("{path} missing from package-macos"))
        };
        for path in [
            ".npm/_cacache",
            ".rustup/downloads",
            "Library/pnpm/store",
            ".pnpm-store",
            "Library/Caches/Homebrew",
            "Library/Caches/bun",
            "Library/Caches/go-build",
            "Library/Caches/go",
            "Library/Caches/gopls",
            "Library/Caches/goimports",
            "Library/Caches/node-gyp",
            "Library/Caches/pip",
            "Library/Caches/typescript",
        ] {
            let rule = entry(path);
            assert_eq!(rule.root, "home");
            assert_eq!(rule.category, "PackageCache");
            assert!(rule.recommended, "{path} should be selected");
        }
        for path in [".cargo/registry", "go/pkg/mod", ".gradle/caches"] {
            let rule = entry(path);
            assert_eq!(rule.root, "home");
            assert_eq!(rule.path, path);
            assert_eq!(rule.category, "PackageCache");
            assert!(!rule.recommended, "{path} stays unselected");
        }

        let browsers = snap.definition("browsers");
        let paths = |name: &str| -> Vec<&str> {
            browsers
                .catalogs
                .get(name)
                .unwrap_or_else(|| panic!("missing catalog {name}"))
                .iter()
                .map(|row| row.path.as_str())
                .collect()
        };
        assert_eq!(
            paths("app_support"),
            [
                "Google/Chrome",
                "Arc",
                "BraveSoftware/Brave-Browser",
                "Microsoft Edge",
                "Vivaldi",
                "Opera",
            ]
        );
        assert_eq!(
            paths("library_caches"),
            [
                "Google/Chrome",
                "com.apple.Safari",
                "Microsoft Edge",
                "Firefox",
                "BraveSoftware",
                "company.thebrowser.Browser",
                "Chromium",
                "com.operasoftware.Opera",
                "com.vivaldi.Vivaldi",
            ]
        );
        assert_eq!(
            paths("windows_user_data"),
            [
                "Google/Chrome/User Data",
                "Microsoft/Edge/User Data",
                "BraveSoftware/Brave-Browser/User Data",
            ]
        );

        assert_eq!(
            snap.list("macos", "jetbrains_products"),
            [
                "IntelliJ IDEA",
                "PyCharm",
                "WebStorm",
                "CLion",
                "GoLand",
                "RubyMine",
                "PhpStorm",
                "DataGrip",
                "Rider",
                "RustRover",
            ]
        );

        let macos = snap.definition("macos");
        let xcode = |path: &str, category: &str| {
            let rule = macos
                .entries
                .iter()
                .find(|rule| rule.path == path)
                .unwrap_or_else(|| panic!("{path} missing"));
            assert_eq!(rule.root, "home");
            assert_eq!(rule.category, category);
            assert!(!rule.recommended, "{path} stays unselected");
        };
        xcode("Library/Developer/Xcode/DerivedData", "DevBuild");
        xcode("Library/Developer/Xcode/iOS DeviceSupport", "DevBuild");
        xcode("Library/Application Support/MobileSync/Backup", "IosBackup");
        for rule in &snap.bundle.rules {
            for path in &rule.entries {
                assert!(
                    !path.path.contains("Xcode/Archives")
                        && !path.path.contains("CoreSimulator/Devices"),
                    "{} still lists {}",
                    rule.id,
                    path.path
                );
            }
        }

        let windows = snap.definition("windows");
        assert_eq!(windows.platform, "windows");
        let thumb = windows
            .entries
            .iter()
            .find(|rule| rule.path == "Microsoft/Windows/Explorer")
            .expect("thumbnail entry");
        assert_eq!(thumb.root, "local");
        assert_eq!(thumb.category, "Thumbnails");
        assert!(thumb.recommended);

        // 这条路径的平台是 windows，只有在 Windows 上才会被扫进目标表。
        #[cfg(windows)]
        {
            let mut targets = Vec::new();
            append_path_targets(&mut targets, None);
            let emitted = targets
                .iter()
                .find(|target| target.path.ends_with("Microsoft/Windows/Explorer"))
                .expect("thumbnail path is a target");
            assert_eq!(
                emitted.category,
                crate::core::categories::CategoryId::Thumbnails
            );
            assert!(emitted.recommended);
        }
    }

    #[test]
    fn pinned_snapshot_survives_new_publication() {
        let old = embedded();
        let mut new = old.bundle.clone();
        new.sequence += 1;
        // Test local nested snapshots without mutating the process-wide active rules.
        let new = Arc::new(RuleSnapshot { bundle: new });
        with_snapshot(old.clone(), || {
            assert!(Arc::ptr_eq(&current(), &old));
            with_snapshot(new.clone(), || assert!(Arc::ptr_eq(&current(), &new)));
            assert!(Arc::ptr_eq(&current(), &old));
        });
    }
}

#[derive(Clone, Debug)]
pub struct RuleSnapshot {
    pub bundle: RuleBundle,
}
impl RuleSnapshot {
    pub fn definition(&self, id: &str) -> &RuleDefinition {
        self.bundle
            .rules
            .iter()
            .find(|r| r.id == id)
            .expect("validated required rule")
    }
    pub fn list(&self, id: &str, key: &str) -> &[String] {
        self.definition(id)
            .lists
            .get(key)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
    pub fn matches_name(&self, id: &str, policy: &str, name: &str) -> bool {
        self.list(id, &format!("{policy}_exact"))
            .iter()
            .any(|entry| name == entry)
            || self
                .list(id, &format!("{policy}_prefixes"))
                .iter()
                .any(|prefix| name.starts_with(prefix))
            || self
                .list(id, &format!("{policy}_contains"))
                .iter()
                .any(|part| name.contains(part))
            || self
                .list(id, &format!("{policy}_suffixes"))
                .iter()
                .any(|suffix| name.ends_with(suffix))
    }
    pub fn number(&self, id: &str, key: &str, default: u64) -> u64 {
        self.definition(id)
            .numbers
            .get(key)
            .copied()
            .unwrap_or(default)
    }
    pub fn applications(&self) -> impl Iterator<Item = (&RuleDefinition, &SourceInstallRule)> {
        self.bundle
            .rules
            .iter()
            .filter(|r| r.platform == "all" || r.platform == std::env::consts::OS)
            .filter_map(|r| r.app.as_ref().map(|a| (r, a)))
    }
}
fn embedded() -> Arc<RuleSnapshot> {
    static EMBEDDED: OnceLock<Arc<RuleSnapshot>> = OnceLock::new();
    EMBEDDED
        .get_or_init(|| {
            Arc::new(RuleSnapshot {
                bundle: RuleBundle::parse(include_bytes!(concat!(env!("OUT_DIR"), "/rules.json")))
                    .expect("valid embedded rules"),
            })
        })
        .clone()
}
pub fn snapshot() -> Arc<RuleSnapshot> {
    embedded()
}

thread_local! { static PINNED: std::cell::RefCell<Option<Arc<RuleSnapshot>>> = const { std::cell::RefCell::new(None) }; }
pub fn current() -> Arc<RuleSnapshot> {
    PINNED.with(|p| p.borrow().clone()).unwrap_or_else(snapshot)
}
pub fn with_snapshot<T>(snapshot: Arc<RuleSnapshot>, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<Arc<RuleSnapshot>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            PINNED.with(|p| {
                p.replace(self.0.take());
            });
        }
    }
    let old = PINNED.with(|p| p.replace(Some(snapshot)));
    let _restore = Restore(old);
    run()
}

pub fn list(id: &str, key: &str) -> Vec<String> {
    current().list(id, key).to_vec()
}

pub struct RuleList {
    pub rule: &'static str,
    pub key: &'static str,
}
impl RuleList {
    pub fn iter(&self) -> std::vec::IntoIter<String> {
        list(self.rule, self.key).into_iter()
    }
    pub fn contains(&self, value: &&str) -> bool {
        current()
            .list(self.rule, self.key)
            .iter()
            .any(|s| s == value)
    }
}
impl IntoIterator for &RuleList {
    type Item = String;
    type IntoIter = std::vec::IntoIter<String>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub fn append_path_targets(
    targets: &mut Vec<crate::core::categories::ScanTarget>,
    home: Option<&std::path::Path>,
) {
    let snapshot = current();
    // 用户级根的解析方式按平台区分：
    // - Windows 从登记表解析**真实前台用户**的目录（`real_user_*`），与 home
    //   锚点相互独立——home 未知时 LOCALAPPDATA 仍可能可信（缩略图缓存就靠
    //   它），未知则各自返回 None；
    // - macOS 没有独立账户模型，用户目录一律由调用方的 home 推导：home 未知
    //   就整块不展开用户路径，而不是回退到 `dirs::*` 指向的进程账户
    //   （那会在「不扫用户目录」的前提下扫用户目录）。
    #[cfg(windows)]
    let (local, roaming) = (
        crate::platform::user_cache_dir(),
        crate::platform::user_data_dir(),
    );
    #[cfg(not(windows))]
    let (local, roaming) = (
        home.map(|h| h.join("Library/Caches")),
        home.map(|h| h.join("Library/Application Support")),
    );
    let mut roots = BTreeMap::from([
        ("home".into(), home.map(std::path::Path::to_path_buf)),
        ("local".into(), local.clone()),
        ("cache".into(), local),
        ("roaming".into(), roaming),
        ("temp".into(), Some(std::env::temp_dir())),
        ("user_temp".into(), crate::platform::user_temp_dir()),
        (
            "system".into(),
            std::env::var_os("SystemRoot")
                .map(std::path::PathBuf::from)
                .or_else(|| cfg!(windows).then(|| std::path::PathBuf::from("C:/Windows"))),
        ),
    ]);
    if cfg!(windows) {
        for entry in snapshot.bundle.rules.iter().flat_map(|rule| &rule.entries) {
            if let Some(letter) = drive_root(&entry.root) {
                roots
                    .entry(entry.root.clone())
                    .or_insert_with(|| Some(std::path::PathBuf::from(format!("{letter}:/"))));
            }
        }
    }
    append_path_targets_with_roots(targets, &snapshot, &roots);
    let mut enumeration = directories::Enumeration::default();
    directories::append(targets, &snapshot, &roots, &mut enumeration);
    versions::append(targets, &snapshot, &roots, &mut enumeration);
}

fn append_path_targets_with_roots(
    targets: &mut Vec<crate::core::categories::ScanTarget>,
    snapshot: &Arc<RuleSnapshot>,
    roots: &BTreeMap<String, Option<std::path::PathBuf>>,
) {
    for rule in &snapshot.bundle.rules {
        if rule.platform != "all" && rule.platform != std::env::consts::OS {
            continue;
        }
        let mut evidence = BTreeMap::new();
        for entry in &rule.entries {
            let Some(root) = roots.get(&entry.root).and_then(Option::as_ref) else {
                continue;
            };
            let observation = evidence.entry(root.clone()).or_insert_with(|| {
                Arc::new(RuleObservation::capture(
                    snapshot,
                    &rule.id,
                    Some(root.clone()),
                ))
            });
            if observation.detected != facts::Evidence::Confirmed {
                continue;
            }
            let expanded = if entry.root == "user_temp" && entry.path == "." {
                Ok(vec![".".into()])
            } else {
                variables::paths(&entry.path, &observation.variables)
            };
            let Ok(paths) = expanded else {
                continue;
            };
            for relative in paths {
                let path = if relative == "." {
                    root.clone()
                } else {
                    root.join(relative.replace('\\', "/"))
                };
                if !rule.variables.is_empty() && !facts::confined_path(root, &path).unwrap_or(false)
                {
                    continue;
                }
                let protected = if entry.operation == PathOperation::Contents {
                    crate::core::safety::is_contents_protected(&path)
                } else {
                    crate::core::safety::is_protected_residual_path(&path)
                };
                if protected {
                    continue;
                }
                let old_enough = std::fs::metadata(&path)
                    .and_then(|md| md.modified())
                    .ok()
                    .and_then(|m| m.elapsed().ok())
                    .is_some_and(|age| age.as_secs() >= entry.minimum_age_seconds);
                targets.push(crate::core::categories::ScanTarget {
                    operation: entry.operation.operation(),
                    disposal: entry.disposal,
                    rule: RuleRef {
                        snapshot: snapshot.clone(),
                        id: rule.id.clone(),
                        scope: Some(root.clone()),
                        contributors: Vec::new(),
                        blocked: None,
                        observation: Some(observation.clone()),
                    },
                    path,
                    label: crate::core::i18n::Text::new(&entry.zh, &entry.en),
                    category: crate::core::categories::CategoryId::from_rule(&entry.category)
                        .expect("validated category"),
                    recommended: entry.recommended
                        && (entry.minimum_age_seconds == 0 || old_enough),
                    size_hint: None,
                });
            }
        }
    }
}
