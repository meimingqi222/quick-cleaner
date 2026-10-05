//! Signed declarative policy selects fixed capabilities; deletion authority remains in safety/cleaner.
pub mod execution;
pub mod facts;
pub mod flow;
mod plan;
pub mod update;
pub mod variables;
pub use plan::{
    CleanupPlan, CompletionCondition, InstallationInstance, OfficialOperation, Operation,
    PlanAction, PlanStep, PlannedTarget, RuleObservation, RuleRef, TargetScope,
};

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, OnceLock, RwLock};

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
    pub entries: Vec<PathRule>,
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
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResidualLocation {
    pub path: String,
    pub source: crate::core::apps::ResidualSource,
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
            for entry in &rule.entries {
                variables::validate_template(&entry.path, &rule.variables)?;
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
                if !relative(&entry.path)
                    || !matches!(
                        entry.root.as_str(),
                        "home" | "local" | "roaming" | "cache" | "temp" | "system"
                    )
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

fn validate_engine_policy(rule: &RuleDefinition) -> Result<(), String> {
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
                assert_eq!(
                    entry.operation.operation(),
                    Operation::classify(&path, category.removes_directory())
                );
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
fn active() -> &'static RwLock<Arc<RuleSnapshot>> {
    static ACTIVE: OnceLock<RwLock<Arc<RuleSnapshot>>> = OnceLock::new();
    ACTIVE.get_or_init(|| RwLock::new(update::load_cached().unwrap_or_else(embedded)))
}
pub fn snapshot() -> Arc<RuleSnapshot> {
    active()
        .read()
        .map(|s| s.clone())
        .unwrap_or_else(|_| embedded())
}
pub(crate) fn activate(bundle: RuleBundle) {
    if let Ok(mut current) = active().write() {
        *current = Arc::new(RuleSnapshot { bundle });
    }
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
    for rule in &snapshot.bundle.rules {
        if rule.platform != "all" && rule.platform != std::env::consts::OS {
            continue;
        }
        let mut evidence = BTreeMap::new();
        for entry in &rule.entries {
            let root = match entry.root.as_str() {
                "home" => home.map(std::path::Path::to_path_buf),
                "local" | "cache" => crate::platform::user_cache_dir(),
                "roaming" => crate::platform::user_data_dir(),
                "temp" => Some(std::env::temp_dir()),
                "system" => std::env::var_os("SystemRoot").map(std::path::PathBuf::from),
                _ => None,
            };
            let Some(root) = root else {
                continue;
            };
            let observation = evidence.entry(root.clone()).or_insert_with(|| {
                Arc::new(RuleObservation::capture(
                    &snapshot,
                    &rule.id,
                    Some(root.clone()),
                ))
            });
            if observation.detected != facts::Evidence::Confirmed {
                continue;
            }
            let Ok(paths) = variables::paths(&entry.path, &observation.variables) else {
                continue;
            };
            for relative in paths {
                let path = root.join(relative.replace('\\', "/"));
                if !rule.variables.is_empty()
                    && !facts::confined_path(&root, &path).unwrap_or(false)
                {
                    continue;
                }
                if crate::core::safety::is_protected_residual_path(&path) {
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
