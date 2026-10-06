use super::RuleSnapshot;
use crate::core::apps::OfficialUninstaller;
use crate::core::model::TargetIdentity;
use crate::core::safety::{at_or_under, norm};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Debug, serde::Serialize)]
pub struct RuleObservation {
    pub rule_id: String,
    pub rule_version: u32,
    pub schema: u32,
    pub sequence: u64,
    pub scope: Option<PathBuf>,
    pub variables: super::variables::Values,
    pub facts: std::collections::BTreeMap<String, super::facts::Evidence>,
    pub detected: super::facts::Evidence,
    pub provider_policy: Option<(String, super::ProviderPolicy)>,
    pub version_guard: Option<super::versions::VersionGuard>,
}
impl RuleObservation {
    pub fn capture(snapshot: &RuleSnapshot, id: &str, scope: Option<PathBuf>) -> Self {
        let definition = snapshot.definition(id);
        let (variables, facts) = scope
            .as_ref()
            .map(|root| definition.evidence_at(root))
            .unwrap_or_default();
        let detected = definition
            .detect
            .as_ref()
            .map_or(super::facts::Evidence::Confirmed, |condition| {
                condition.evaluate(&facts)
            });
        Self {
            rule_id: id.into(),
            rule_version: definition.version,
            schema: snapshot.bundle.schema,
            sequence: snapshot.bundle.sequence,
            scope,
            variables,
            facts,
            detected,
            provider_policy: None,
            version_guard: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RuleRef {
    pub snapshot: Arc<RuleSnapshot>,
    pub id: String,
    pub scope: Option<PathBuf>,
    pub contributors: Vec<RuleRef>,
    pub blocked: Option<String>,
    pub observation: Option<Arc<RuleObservation>>,
}
impl PartialEq for RuleRef {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.scope == other.scope
            && Arc::ptr_eq(&self.snapshot, &other.snapshot)
            && self.contributors == other.contributors
            && self.blocked == other.blocked
            && match (&self.observation, &other.observation) {
                (Some(left), Some(right)) => Arc::ptr_eq(left, right),
                (None, None) => true,
                _ => false,
            }
    }
}
impl Eq for RuleRef {}
impl RuleRef {
    pub fn provider(id: &str, key: &str) -> Self {
        let mut rule = Self::new(id, None);
        let mut observation = RuleObservation::capture(&rule.snapshot, id, None);
        match rule.snapshot.definition(id).provider_policies.get(key) {
            Some(policy) => observation.provider_policy = Some((key.into(), policy.clone())),
            None => rule.blocked = Some(format!("Missing provider policy: {id}/{key}")),
        }
        rule.observation = Some(Arc::new(observation));
        rule
    }
    pub fn new(id: impl Into<String>, scope: Option<PathBuf>) -> Self {
        Self {
            snapshot: super::current(),
            id: id.into(),
            scope,
            contributors: Vec::new(),
            blocked: None,
            observation: None,
        }
    }
    pub fn engine() -> Self {
        Self {
            snapshot: super::current(),
            id: "engine".into(),
            scope: None,
            contributors: Vec::new(),
            blocked: None,
            observation: None,
        }
    }
    pub fn build() -> Self {
        Self {
            snapshot: super::current(),
            id: "build".into(),
            scope: None,
            contributors: Vec::new(),
            blocked: None,
            observation: None,
        }
    }
    pub fn preserved(&self) -> Vec<PathBuf> {
        let mut preserved: Vec<_> = self
            .contributors
            .iter()
            .flat_map(RuleRef::preserved)
            .collect();
        let Some(root) = &self.scope else {
            return preserved;
        };
        let definition = self.snapshot.definition(&self.id);
        let values = self
            .observation
            .as_ref()
            .map(|observation| observation.variables.clone())
            .unwrap_or_else(|| super::variables::evaluate(&definition.variables, root));
        preserved.extend(
            definition
                .preserve
                .iter()
                .chain(definition.app.iter().flat_map(|app| &app.preserve))
                .flat_map(|p| {
                    super::variables::paths(p, &values).unwrap_or_else(|_| vec![String::new()])
                })
                .map(|p| root.join(p.replace('\\', "/")))
                .collect::<Vec<_>>(),
        );
        preserved
    }
    /// Cheap precheck (no filesystem): does this rule — or any merged contributor
    /// — declare preserved paths? Discovery uses it to avoid evaluating variables
    /// for every target when nothing can be preserved.
    pub fn declares_preserve(&self) -> bool {
        let definition = self.snapshot.definition(&self.id);
        !definition.preserve.is_empty()
            || definition
                .app
                .as_ref()
                .is_some_and(|app| !app.preserve.is_empty())
            || self.contributors.iter().any(RuleRef::declares_preserve)
    }
    pub fn merge(&mut self, other: &Self) {
        if self == other || self.contributors.contains(other) {
            return;
        }
        if !Arc::ptr_eq(&self.snapshot, &other.snapshot) {
            self.blocked = Some("Mixed rule snapshots".into());
        }
        self.contributors.push(other.clone());
    }
    pub fn revalidate(&self) -> Result<(), String> {
        if let Some(reason) = &self.blocked {
            return Err(reason.clone());
        }
        if self
            .observation
            .as_ref()
            .is_some_and(|observation| observation.detected != super::facts::Evidence::Confirmed)
        {
            return Err("Scan ownership evidence absent or unknown".into());
        }
        for contributor in &self.contributors {
            contributor.revalidate()?;
        }
        let definition = self.snapshot.definition(&self.id);
        if self.observation.as_ref().is_some_and(|observation| {
            observation.rule_id != self.id
                || observation.rule_version != definition.version
                || observation.sequence != self.snapshot.bundle.sequence
                || observation.schema != self.snapshot.bundle.schema
                || observation.scope != self.scope
        }) {
            return Err("Rule observation does not match its snapshot and scope".into());
        }
        if let Some(guard) = self
            .observation
            .as_ref()
            .and_then(|observation| observation.version_guard.as_ref())
        {
            guard.revalidate(definition, self.scope.as_deref())?;
        }
        if let Some(root) = &self.scope {
            let values = super::variables::evaluate(&definition.variables, root);
            for preserve in &definition.preserve {
                super::variables::paths(preserve, &values)?;
            }
        } else if !definition.variables.is_empty() {
            return Err("Missing variable scope".into());
        }
        if definition.detect.is_some() {
            let root = self.scope.as_ref().ok_or("Missing evidence scope")?;
            if definition.matches_at(root) != super::facts::Evidence::Confirmed {
                return Err("Ownership evidence changed or unavailable".into());
            }
        }
        Ok(())
    }

    pub fn observed(mut self) -> Self {
        if self.observation.is_none() {
            self.observation = Some(Arc::new(RuleObservation::capture(
                &self.snapshot,
                &self.id,
                self.scope.clone(),
            )));
        }
        self.contributors = self
            .contributors
            .into_iter()
            .map(RuleRef::observed)
            .collect();
        self
    }

    fn observations(&self, output: &mut Vec<Arc<RuleObservation>>) {
        if let Some(observation) = &self.observation {
            output.push(observation.clone());
        }
        for contributor in &self.contributors {
            contributor.observations(output);
        }
    }

    fn manifest_membership(&self) -> bool {
        self.snapshot
            .definition(&self.id)
            .variables
            .values()
            .any(|variable| matches!(variable, super::variables::Variable::JsonSet { .. }))
            || self.contributors.iter().any(RuleRef::manifest_membership)
    }

    fn blocked_reasons(&self, output: &mut Vec<String>) {
        output.extend(self.blocked.iter().cloned());
        for contributor in &self.contributors {
            contributor.blocked_reasons(output);
        }
    }

    pub fn preserved_live(&self) -> Vec<PathBuf> {
        let mut live = self.clone();
        live.observation = None;
        live.contributors.clear();
        let mut paths = live.preserved();
        paths.extend(self.contributors.iter().flat_map(RuleRef::preserved_live));
        paths
    }

    fn authorizes(&self, path: &std::path::Path) -> Result<(), String> {
        for contributor in &self.contributors {
            contributor.authorizes(path)?;
        }
        let definition = self.snapshot.definition(&self.id);
        if definition.variables.is_empty() {
            return Ok(());
        }
        let root = self.scope.as_ref().ok_or("Missing variable scope")?;
        let values = super::variables::evaluate(&definition.variables, root);
        let authorized = definition.entries.iter().any(|entry| {
            super::variables::paths(&entry.path, &values).is_ok_and(|members| {
                members
                    .iter()
                    .any(|member| norm(&root.join(member.replace('\\', "/"))) == norm(path))
            })
        });
        if !authorized {
            return Err("Manifest membership changed or unavailable".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeKind {
    RegistryKey,
    RegistryValue,
    ScheduledTask,
    SystemExtension,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    File,
    Tree,
    GitWorktree {
        registration: PathBuf,
    },
    Contents,
    Docker {
        reference: String,
    },
    Brew,
    Go,
    Pnpm,
    Snapshot {
        name: String,
    },
    Trash,
    /// Native (non-filesystem) residue: a registry entry, scheduled task or system
    /// extension. `identifier` is the native id (registry path / value name, task
    /// path, `teamID/bundleID`); the target's `path` carries the same string so the
    /// plan's scope and target key stay uniform.
    Native {
        native: NativeKind,
        identifier: String,
    },
    Registration,
    OfficialUninstall,
}
impl Operation {
    #[cfg(test)]
    pub fn classify(path: &std::path::Path, remove_directory: bool) -> Self {
        if let Some(reference) = crate::core::model::docker_rmi_ref(path) {
            Self::Docker { reference }
        } else if crate::core::brew::is_brew_virtual(path) {
            Self::Brew
        } else if let Some(name) = crate::core::model::snapshot_name(path) {
            Self::Snapshot { name }
        } else if crate::platform::is_system_trash(path) {
            Self::Trash
        } else if crate::core::owner::is_go_modcache(path) {
            Self::Go
        } else if crate::core::owner::is_pnpm_store(path) {
            Self::Pnpm
        } else if remove_directory {
            if path.join(".git").is_file() {
                Self::GitWorktree {
                    registration: crate::core::worktrees::inspect(path)
                        .map(|entry| entry.admin)
                        .unwrap_or_else(|_| path.join(".git")),
                }
            } else {
                Self::Tree
            }
        } else {
            Self::Contents
        }
    }

    pub fn is_native_resource(&self) -> bool {
        matches!(
            self,
            Self::Docker { .. } | Self::Snapshot { .. } | Self::Brew | Self::Native { .. }
        )
    }

    pub fn for_scanned_path(&self, path: &std::path::Path) -> Self {
        if matches!(self, Self::Tree | Self::Contents)
            && std::fs::symlink_metadata(path).is_ok_and(|md| md.is_file())
        {
            Self::File
        } else if *self == Self::Tree && path.join(".git").is_file() {
            Self::GitWorktree {
                registration: crate::core::worktrees::inspect(path)
                    .map(|entry| entry.admin)
                    .unwrap_or_else(|_| path.join(".git")),
            }
        } else {
            self.clone()
        }
    }

    /// Native resources are identified by typed parameters, never by their display path.
    pub fn target_key(&self, path: &std::path::Path) -> String {
        match self {
            Self::Docker { reference } => format!("docker:{reference}"),
            Self::Snapshot { name } => format!(
                "snapshot:{}",
                super::execution::snapshot_date(name).unwrap_or(name)
            ),
            Self::Brew => "brew:cleanup".into(),
            Self::Native { native, identifier } => format!("native:{native:?}:{identifier}"),
            _ => format!("path:{}", norm(path)),
        }
    }

    pub fn validate_target(&self, path: &std::path::Path, remove_directory: bool) -> bool {
        if self.is_native_resource() {
            // Native residue (registry / task / extension) is identified by its typed
            // `identifier`, not a display path, so it has no virtual-path requirement.
            if let Self::Native { .. } = self {
                return true;
            }
            // A legacy display URI is allowed to differ from the actual resource parameter.
            // A real filesystem selection must never be silently converted into a native operation.
            crate::core::model::is_virtual_path(path)
                && match self {
                    Self::Docker { reference } => crate::core::docker::valid_reference(reference),
                    Self::Snapshot { name } => super::execution::snapshot_date(name).is_some(),
                    Self::Brew => true,
                    _ => false,
                }
        } else {
            if crate::core::model::is_virtual_path(path) {
                return false;
            }
            let ordinary_filesystem = !crate::platform::is_system_trash(path)
                && !crate::core::owner::is_go_modcache(path)
                && !crate::core::owner::is_pnpm_store(path);
            match self {
                Self::File => {
                    remove_directory
                        && ordinary_filesystem
                        && std::fs::symlink_metadata(path).is_ok_and(|md| md.is_file())
                }
                Self::Tree => {
                    remove_directory && ordinary_filesystem && !path.join(".git").is_file()
                }
                Self::Contents => !remove_directory && ordinary_filesystem,
                Self::GitWorktree { registration } => {
                    remove_directory
                        && ordinary_filesystem
                        && crate::core::worktrees::inspect(path)
                            .is_ok_and(|worktree| worktree.admin == *registration)
                }
                Self::Go => crate::core::owner::is_go_modcache(path),
                Self::Pnpm => crate::core::owner::is_pnpm_store(path),
                Self::Trash => crate::platform::is_system_trash(path),
                _ => false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn typed_filesystem_guards_reject_owner_bypass_and_unknown_worktree() {
        use super::Operation;
        let root = crate::core::testing::fixture("typed_operation_extent");
        assert!(Operation::Tree.validate_target(&root, true));
        assert!(Operation::Contents.validate_target(&root, false));
        assert!(!Operation::File.validate_target(&root, true));
        let file = root.join("fixture.txt");
        std::fs::write(&file, b"fixture").unwrap();
        assert!(Operation::File.validate_target(&file, true));
        assert!(!Operation::File.validate_target(&file, false));
        for path in [
            "docker://image/display",
            "brew://cleanup",
            "tmutil://snapshot/display",
        ] {
            let path = std::path::Path::new(path);
            assert!(!Operation::Tree.validate_target(path, true));
            assert!(!Operation::Contents.validate_target(path, false));
            assert!(!Operation::File.validate_target(path, true));
        }
        for (path, owner) in [
            (root.join("go/pkg/mod"), Operation::Go),
            (root.join(".pnpm-store"), Operation::Pnpm),
        ] {
            assert!(owner.validate_target(&path, false));
            assert!(!Operation::Tree.validate_target(&path, true));
            assert!(!Operation::Contents.validate_target(&path, false));
        }
        std::fs::write(root.join(".git"), b"gitdir: missing\n").unwrap();
        assert!(!Operation::Tree.validate_target(&root, true));
        assert!(!Operation::GitWorktree {
            registration: root.join(".git")
        }
        .validate_target(&root, true));
        assert!(file.is_file());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn frozen_official_operation_retains_arguments_and_rejects_replaced_cwd() {
        let root = crate::core::testing::fixture("official_cwd");
        let cwd = root.join("home");
        std::fs::create_dir(&cwd).unwrap();
        let exe = root.join("runtime.exe");
        std::fs::write(&exe, b"fixture").unwrap();
        let mut command = crate::core::apps::OfficialUninstaller {
            provider: "fixture".into(),
            executable: exe,
            arguments: vec!["--preserve".into()],
            working_directory: cwd.clone(),
            installed_artifacts: vec![cwd.join("source")],
        };
        let frozen = super::OfficialOperation::capture(&command, &[]).unwrap();
        command.arguments.push("--changed".into());
        assert_eq!(frozen.command().unwrap().arguments, ["--preserve"]);
        std::fs::rename(&cwd, root.join("saved-home")).unwrap();
        std::fs::create_dir(&cwd).unwrap();
        assert!(frozen.command().is_err());
        std::fs::remove_dir(&cwd).unwrap();
        assert!(frozen.command().is_err());
        assert!(super::OfficialOperation::capture(&command, &[]).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn frozen_installation_instances_reject_expansion_appearance_and_replacement() {
        let root = crate::core::testing::fixture("frozen_installation");
        let existing = root.join("artifact");
        let absent = root.join("missing");
        std::fs::write(&existing, b"original").unwrap();
        let instance =
            super::InstallationInstance::capture(root.clone(), &[existing.clone(), absent.clone()])
                .unwrap();
        assert!(instance
            .validate_live_paths(&root, std::slice::from_ref(&existing))
            .is_ok());
        assert!(instance
            .validate_live_paths(&root, &[root.join("new-claim")])
            .is_err());
        assert!(instance
            .validate_live_paths(
                &root.join("other-instance"),
                std::slice::from_ref(&existing)
            )
            .is_err());
        std::fs::write(&absent, b"appeared after scan").unwrap();
        assert!(instance.revalidate().is_err());
        std::fs::remove_file(&absent).unwrap();
        std::fs::rename(&existing, root.join("saved-original")).unwrap();
        std::fs::write(&existing, b"replaced").unwrap();
        assert!(instance.revalidate().is_err());
        std::fs::remove_file(&existing).unwrap();
        assert!(
            instance.revalidate().is_ok(),
            "confirmed disappearance permits half-uninstall recovery"
        );
        assert!(super::InstallationInstance::capture(root.clone(), &[]).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    use super::*;
    #[test]
    fn typed_operation_preserves_existing_owner_routes() {
        for (path, operation) in [
            (
                "docker://image/sha256:123",
                Operation::Docker {
                    reference: "sha256:123".into(),
                },
            ),
            ("brew://cleanup", Operation::Brew),
            ("C:/Users/test/go/pkg/mod", Operation::Go),
            ("C:/Users/test/.pnpm-store", Operation::Pnpm),
            ("C:/Users/test/cache", Operation::Contents),
        ] {
            assert_eq!(
                Operation::classify(std::path::Path::new(path), false),
                operation,
                "{path}"
            );
        }
        assert_eq!(
            Operation::classify(std::path::Path::new("C:/Users/test/app"), true),
            Operation::Tree
        );
    }
    #[test]
    fn typed_operation_matches_all_legacy_dispatch_predicates() {
        for path in [
            "docker://image/ghcr.io/example/image:latest",
            "brew://cleanup",
            "brew://other",
            "tmutil://snapshot/2026-10-04-120000",
            "C:/Users/test/go/pkg/mod",
            "C:/Users/test/go/pkg/mod/cache",
            "C:/Users/test/.pnpm-store",
            "C:/Users/test/cache",
            "C:/Users/test/app.exe",
        ] {
            let path = std::path::Path::new(path);
            for remove in [false, true] {
                let operation = Operation::classify(path, remove);
                assert_eq!(
                    matches!(operation, Operation::Docker { .. }),
                    crate::core::model::docker_rmi_ref(path).is_some()
                );
                assert_eq!(
                    operation == Operation::Brew,
                    crate::core::brew::is_brew_virtual(path)
                );
                assert_eq!(
                    matches!(operation, Operation::Snapshot { .. }),
                    crate::core::model::snapshot_name(path).is_some()
                );
                assert_eq!(
                    operation == Operation::Go,
                    crate::core::owner::is_go_modcache(path)
                );
                assert_eq!(
                    operation == Operation::Pnpm,
                    crate::core::owner::is_pnpm_store(path)
                );
            }
        }
    }

    #[test]
    fn native_parameters_and_keys_do_not_follow_display_uris() {
        let operation = Operation::Docker {
            reference: "example/app:2".into(),
        };
        let first = std::path::Path::new("docker://image/display-one");
        let second = std::path::Path::new("docker://image/display-two");
        assert_eq!(operation.target_key(first), operation.target_key(second));
        assert!(operation.validate_target(first, false));
        assert!(!operation.validate_target(std::path::Path::new("C:/cache"), false));
        let encoded = serde_json::to_vec(&operation).unwrap();
        let decoded: Operation = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, operation);
        assert!(serde_json::from_str::<Operation>(
            r#"{"kind":"docker","reference":"app:2","script":"anything"}"#
        )
        .is_err());
    }
    #[test]
    fn mixed_snapshot_explanation_retains_unknown_and_conflict_without_panicking() {
        let mut bundle = super::super::embedded().bundle.clone();
        bundle
            .rules
            .push(toml::from_str(include_str!("../../../rules/fixtures/manifest.toml")).unwrap());
        bundle.validate().unwrap();
        let contributor = RuleRef {
            snapshot: Arc::new(RuleSnapshot { bundle }),
            id: "orion-fixture".into(),
            scope: None,
            contributors: vec![],
            blocked: None,
            observation: None,
        }
        .observed();
        let mut rule = RuleRef::engine();
        rule.merge(&contributor);
        let plan = CleanupPlan::new(
            rule,
            vec![PlannedTarget {
                path: PathBuf::from("C:/fixture/cache"),
                operation: Operation::Tree,
                identity: None,
                disposal: crate::core::cleaner::Disposal::Permanent,
            }],
        );
        let explanation = plan.explanation();
        assert_eq!(explanation["observations"].as_array().unwrap().len(), 2);
        assert!(explanation["blocked"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason.as_str().unwrap().contains("Mixed rule snapshots")));
        assert!(explanation["blocked"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason.as_str().unwrap().contains("orion-fixture")));
        assert!(plan.validate().is_err());
    }

    #[test]
    fn exported_scopes_and_completion_do_not_guess_from_display_paths() {
        let root = PathBuf::from("C:/fixture/cache");
        let target = |operation| PlannedTarget {
            path: root.clone(),
            operation,
            identity: None,
            disposal: crate::core::cleaner::Disposal::Permanent,
        };
        let plan = CleanupPlan::new(
            RuleRef::engine(),
            vec![
                target(Operation::Tree),
                target(Operation::Contents),
                target(Operation::File),
                target(Operation::Docker {
                    reference: "example/app:2".into(),
                }),
            ],
        );
        assert!(matches!(plan.scope(0), Some(TargetScope::OwnedTree { .. })));
        assert!(matches!(plan.scope(1), Some(TargetScope::Contents { .. })));
        assert!(matches!(plan.scope(2), Some(TargetScope::File { .. })));
        assert_eq!(
            plan.scope(3),
            Some(TargetScope::NativeResource {
                key: "docker:example/app:2".into()
            })
        );
        assert_eq!(
            plan.targets[3].completion(),
            CompletionCondition::DockerReferenceAbsent {
                reference: "example/app:2".into()
            }
        );
        assert!(matches!(
            plan.targets[1].completion(),
            CompletionCondition::ContentsEmpty { .. }
        ));
        let encoded = plan.explanation();
        assert_eq!(encoded["steps"].as_array().unwrap().len(), 12);
        assert_eq!(
            encoded["targets"][3]["completion"]["reference"],
            "example/app:2"
        );
    }

    #[test]
    fn source_and_worktree_steps_preserve_verification_before_records() {
        let target = |operation| PlannedTarget {
            path: PathBuf::from("C:/fixture/app"),
            operation,
            identity: None,
            disposal: crate::core::cleaner::Disposal::Permanent,
        };
        let source = super::super::embedded()
            .bundle
            .rules
            .iter()
            .find(|rule| rule.app.is_some())
            .unwrap()
            .id
            .clone();
        let plan = CleanupPlan::new(
            RuleRef::new(source, None),
            vec![
                target(Operation::OfficialUninstall),
                target(Operation::GitWorktree {
                    registration: PathBuf::from("C:/fixture/repo/.git/worktrees/one"),
                }),
            ],
        );
        let steps = plan.steps();
        assert_eq!(steps.len(), 10);
        for (step, action) in steps[..5]
            .iter()
            .zip(super::super::execution::SOURCE_ACTIONS)
        {
            assert_eq!(step.action, PlanAction::SourceLifecycle { action });
        }
        assert!(matches!(
            steps[7].action,
            PlanAction::Verify {
                condition: CompletionCondition::PathAbsent { .. }
            }
        ));
        assert!(matches!(
            steps[8].action,
            PlanAction::UnregisterWorktree { .. }
        ));
        assert!(matches!(
            steps[9].action,
            PlanAction::Verify {
                condition: CompletionCondition::WorktreeAndRegistrationAbsent { .. }
            }
        ));
        assert!(
            steps[5].depends_on.is_empty(),
            "independent resources must not depend on another target"
        );
        assert_eq!(steps[8].depends_on, [7]);
        assert_eq!(steps[9].depends_on, [8]);
    }

    #[test]
    fn frozen_and_live_preservation_both_block_manifest_targets() {
        let root = crate::core::testing::fixture("plan_preserve_union");
        std::fs::create_dir_all(root.join("Orion/cache")).unwrap();
        std::fs::write(root.join("Orion/cache/sentinel"), b"keep").unwrap();
        let manifest = root.join("Orion/manifest.json");
        std::fs::write(
            &manifest,
            br#"{"owner":"orion","members":["cache"],"saved":["config"]}"#,
        )
        .unwrap();
        let mut definition: super::super::RuleDefinition =
            toml::from_str(include_str!("../../../rules/fixtures/manifest.toml")).unwrap();
        definition.preserve = vec!["${app}/${saved}".into()];
        definition.variables.insert(
            "saved".into(),
            super::super::variables::Variable::JsonSet {
                path: "${app}/manifest.json".into(),
                pointer: "/saved".into(),
            },
        );
        let mut bundle = super::super::embedded().bundle.clone();
        bundle.rules.push(definition);
        bundle.validate().unwrap();
        let snapshot = Arc::new(RuleSnapshot { bundle });
        let rule = RuleRef {
            snapshot,
            id: "orion-fixture".into(),
            scope: Some(root.clone()),
            contributors: vec![],
            blocked: None,
            observation: None,
        };
        let target = PlannedTarget {
            path: root.join("Orion/cache"),
            operation: Operation::Tree,
            identity: crate::core::model::capture_identity(&root.join("Orion/cache")),
            disposal: crate::core::cleaner::Disposal::Permanent,
        };
        let plan = CleanupPlan::new(rule.clone(), vec![target.clone()]);
        assert!(plan.validate().is_ok());
        let explanation = plan.explanation();
        std::fs::write(
            &manifest,
            br#"{"owner":"orion","members":["cache"],"saved":["cache"]}"#,
        )
        .unwrap();
        assert!(
            plan.validate().is_err(),
            "newly protected target must be blocked"
        );
        assert_eq!(explanation, plan.explanation());
        let frozen = CleanupPlan::new(rule, vec![target]);
        std::fs::write(
            &manifest,
            br#"{"owner":"orion","members":["cache"],"saved":["config"]}"#,
        )
        .unwrap();
        assert!(
            frozen.validate().is_err(),
            "a removed live preserve must remain frozen"
        );
        assert!(root.join("Orion/cache/sentinel").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preservation_and_conflicts_cannot_grant_deletion() {
        let root = crate::core::testing::fixture("rules_plan");
        std::fs::create_dir_all(root.join("keep")).unwrap();
        let target = PlannedTarget {
            path: root.clone(),
            operation: Operation::Tree,
            identity: crate::core::model::capture_identity(&root),
            disposal: crate::core::cleaner::Disposal::Permanent,
        };
        let mut plan = CleanupPlan {
            rule: RuleRef::engine(),
            targets: vec![target.clone()],
            preserve: vec![root.join("keep")],
            blocked: vec![],
            observations: vec![],
            installation: None,
            official: None,
        };
        assert!(plan.validate().is_err());
        plan.preserve.clear();
        assert!(plan.validate().is_ok());
        plan.targets.push(PlannedTarget {
            operation: Operation::Contents,
            ..target
        });
        assert!(plan.validate().is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// 保留项的两个方向要有各自的解释：目标在保留项里、目标覆盖保留项
    /// （后者需要拆分子目标，当前拒绝并说明是哪条保留项）。
    #[test]
    fn preserved_overlap_reasons_name_the_direction_and_path() {
        let root = crate::core::testing::fixture("rules_plan_preserve_direction");
        std::fs::create_dir_all(root.join("keep/inner")).unwrap();
        std::fs::create_dir_all(root.join("target/child")).unwrap();
        let plan = |path: &std::path::Path, keep: PathBuf| CleanupPlan {
            rule: RuleRef::engine(),
            targets: vec![PlannedTarget {
                path: path.to_path_buf(),
                operation: Operation::Contents,
                identity: crate::core::model::capture_identity(path),
                disposal: crate::core::cleaner::Disposal::Permanent,
            }],
            preserve: vec![keep],
            blocked: vec![],
            observations: vec![],
            installation: None,
            official: None,
        };
        let inside = plan(&root.join("keep/inner"), root.join("keep"))
            .validate()
            .unwrap_err();
        assert!(inside.contains("inside a preserved path"), "{inside}");
        assert!(inside.contains("keep"), "{inside}");
        let covering = plan(&root.join("target"), root.join("target/child"))
            .validate()
            .unwrap_err();
        assert!(covering.contains("covers a preserved path"), "{covering}");
        assert!(covering.contains("child"), "{covering}");
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[derive(Clone, Debug)]
pub struct PlannedTarget {
    pub path: PathBuf,
    pub operation: Operation,
    pub identity: Option<TargetIdentity>,
    pub disposal: crate::core::cleaner::Disposal,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TargetScope {
    File {
        path: PathBuf,
    },
    OwnedTree {
        path: PathBuf,
    },
    Contents {
        path: PathBuf,
    },
    ManifestMember {
        path: PathBuf,
        extent: Box<TargetScope>,
    },
    Worktree {
        checkout: PathBuf,
        registration: PathBuf,
    },
    NativeResource {
        key: String,
    },
    Registration {
        identifier: PathBuf,
    },
    Installation {
        root: PathBuf,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompletionCondition {
    PathAbsent {
        path: PathBuf,
    },
    ContentsEmpty {
        path: PathBuf,
    },
    DockerReferenceAbsent {
        reference: String,
    },
    SnapshotAbsent {
        name: String,
    },
    BrewPreviewEmpty,
    GoCacheAbsent {
        path: PathBuf,
    },
    PnpmStoreHealthy {
        path: PathBuf,
    },
    TrashInventoryEmpty {
        root: PathBuf,
    },
    WorktreeAndRegistrationAbsent {
        checkout: PathBuf,
        registration: PathBuf,
    },
    RegistrationAbsent {
        identifier: PathBuf,
    },
    InstallationArtifactsAndRegistrationsAbsent {
        root: PathBuf,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanAction {
    Revalidate,
    Apply {
        operation: Operation,
    },
    Verify {
        condition: CompletionCondition,
    },
    SourceLifecycle {
        action: super::execution::SourceAction,
    },
    UnregisterWorktree {
        registration: PathBuf,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PlanStep {
    pub id: usize,
    pub target: usize,
    pub depends_on: Vec<usize>,
    pub action: PlanAction,
}

impl PlannedTarget {
    pub fn completion(&self) -> CompletionCondition {
        match &self.operation {
            Operation::File | Operation::Tree => CompletionCondition::PathAbsent {
                path: self.path.clone(),
            },
            Operation::Contents => CompletionCondition::ContentsEmpty {
                path: self.path.clone(),
            },
            Operation::GitWorktree { registration } => {
                CompletionCondition::WorktreeAndRegistrationAbsent {
                    checkout: self.path.clone(),
                    registration: registration.clone(),
                }
            }
            Operation::Docker { reference } => CompletionCondition::DockerReferenceAbsent {
                reference: reference.clone(),
            },
            Operation::Snapshot { name } => {
                CompletionCondition::SnapshotAbsent { name: name.clone() }
            }
            Operation::Brew => CompletionCondition::BrewPreviewEmpty,
            Operation::Go => CompletionCondition::GoCacheAbsent {
                path: self.path.clone(),
            },
            Operation::Pnpm => CompletionCondition::PnpmStoreHealthy {
                path: self.path.clone(),
            },
            Operation::Trash => CompletionCondition::TrashInventoryEmpty {
                root: self.path.clone(),
            },
            Operation::Registration | Operation::Native { .. } => {
                CompletionCondition::RegistrationAbsent {
                    identifier: self.path.clone(),
                }
            }
            Operation::OfficialUninstall => {
                CompletionCondition::InstallationArtifactsAndRegistrationsAbsent {
                    root: self.path.clone(),
                }
            }
        }
    }

    fn scope(&self) -> TargetScope {
        match &self.operation {
            Operation::File => TargetScope::File {
                path: self.path.clone(),
            },
            Operation::Tree => TargetScope::OwnedTree {
                path: self.path.clone(),
            },
            Operation::Contents | Operation::Go | Operation::Pnpm | Operation::Trash => {
                TargetScope::Contents {
                    path: self.path.clone(),
                }
            }
            Operation::GitWorktree { registration } => TargetScope::Worktree {
                checkout: self.path.clone(),
                registration: registration.clone(),
            },
            Operation::Docker { .. } | Operation::Snapshot { .. } | Operation::Brew => {
                TargetScope::NativeResource {
                    key: self.operation.target_key(&self.path),
                }
            }
            Operation::Native { .. } => TargetScope::NativeResource {
                key: self.operation.target_key(&self.path),
            },
            Operation::Registration => TargetScope::Registration {
                identifier: self.path.clone(),
            },
            Operation::OfficialUninstall => TargetScope::Installation {
                root: self.path.clone(),
            },
        }
    }
}

#[derive(Clone, Debug)]
struct InstallationArtifact {
    path: PathBuf,
    identity: Option<TargetIdentity>,
    #[cfg(windows)]
    object_id: Option<(u64, u64)>,
}
impl InstallationArtifact {
    fn unchanged(&self) -> bool {
        #[cfg(windows)]
        {
            self.identity.is_some()
                && self.object_id.is_some()
                && crate::platform::windows::identity::object_id(&self.path) == self.object_id
        }
        #[cfg(not(windows))]
        {
            self.identity
                .is_some_and(|identity| identity.recheck(&self.path))
        }
    }
}

#[derive(Clone, Debug)]
pub struct InstallationInstance {
    pub root: PathBuf,
    artifacts: Vec<InstallationArtifact>,
}
impl InstallationInstance {
    pub fn artifact_count(&self) -> usize {
        self.artifacts.len()
    }
    /// Windows 发现通道在计划固化前观察快捷方式；其他平台没有这条调用链。
    #[cfg(windows)]
    pub(crate) fn observe_scan_paths(&mut self, paths: &[PathBuf]) -> Result<(), String> {
        if paths.is_empty() {
            return Ok(());
        }
        let additions = Self::capture(self.root.clone(), paths)?;
        for artifact in additions.artifacts {
            if !self
                .artifacts
                .iter()
                .any(|seen| norm(&seen.path) == norm(&artifact.path))
            {
                if self.artifacts.len() == 4096 {
                    return Err("Installation artifact budget exceeded".into());
                }
                self.artifacts.push(artifact);
            }
        }
        Ok(())
    }
    pub(crate) fn scanned_identity(
        &self,
        path: &std::path::Path,
    ) -> Result<Option<TargetIdentity>, String> {
        let artifact = self
            .artifacts
            .iter()
            .find(|artifact| norm(&artifact.path) == norm(path))
            .ok_or("Path is outside the scanned installation evidence")?;
        match std::fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Ok(_) if artifact.unchanged() => Ok(artifact.identity),
            _ => Err("Scanned installation artifact changed or is unconfirmed".into()),
        }
    }
    pub fn capture(root: PathBuf, paths: &[PathBuf]) -> Result<Self, String> {
        if paths.is_empty() || paths.len() > 4096 {
            return Err("Installation artifact budget invalid".into());
        }
        let mut artifacts: Vec<InstallationArtifact> = Vec::new();
        for path in paths {
            let identity = match std::fs::symlink_metadata(path) {
                Ok(metadata) if super::facts::is_link(&metadata) => {
                    return Err("Redirected installation artifact".into())
                }
                Ok(_) => Some(
                    crate::core::model::capture_identity(path)
                        .ok_or("Installation artifact identity unavailable")?,
                ),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(format!("Installation artifact unreadable: {error}")),
            };
            #[cfg(windows)]
            let object_id = if identity.is_some() {
                Some(
                    crate::platform::windows::identity::object_id(path)
                        .ok_or("Installation stable identity unavailable")?,
                )
            } else {
                None
            };
            if !artifacts.iter().any(|seen| norm(&seen.path) == norm(path)) {
                artifacts.push(InstallationArtifact {
                    path: path.clone(),
                    identity,
                    #[cfg(windows)]
                    object_id,
                });
            }
        }
        Ok(Self { root, artifacts })
    }
    pub fn validate_live_paths(
        &self,
        root: &std::path::Path,
        paths: &[PathBuf],
    ) -> Result<(), String> {
        if norm(root) != norm(&self.root)
            || paths.iter().any(|path| {
                !self
                    .artifacts
                    .iter()
                    .any(|observed| norm(&observed.path) == norm(path))
            })
        {
            return Err("Installation scope expanded or instance changed since scan".into());
        }
        self.revalidate()
    }
    fn revalidate(&self) -> Result<(), String> {
        for artifact in &self.artifacts {
            match std::fs::symlink_metadata(&artifact.path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Ok(_) if artifact.unchanged() => {}
                _ => {
                    return Err(
                        "Installation artifact changed, appeared or became unreadable since scan"
                            .into(),
                    )
                }
            }
        }
        Ok(())
    }
    fn explanation(&self) -> serde_json::Value {
        serde_json::json!({"root": self.root, "artifacts": self.artifacts.iter().map(|artifact|
            serde_json::json!({"path": artifact.path, "identity_captured": artifact.identity.is_some(), "absent_at_scan": artifact.identity.is_none()})
        ).collect::<Vec<_>>()})
    }
}

/// The scanned official operation freezes its command and evidence; execution never re-derives
/// either, so a replaced interpreter or a runtime appearing later cannot hijack the plan.
#[derive(Clone, Debug)]
pub struct OfficialOperation {
    uninstaller: OfficialUninstaller,
    evidence: Vec<InstallationArtifact>,
    working_directory: InstallationInstance,
}
impl OfficialOperation {
    /// `evidence` lists extra files the frozen command consumes (the declared module file);
    /// the executable itself is always captured. Every entry must be a present regular file.
    pub fn capture(
        uninstaller: &OfficialUninstaller,
        evidence: &[PathBuf],
    ) -> Result<Self, String> {
        if !std::fs::symlink_metadata(&uninstaller.working_directory)
            .is_ok_and(|metadata| metadata.is_dir() && !super::facts::is_link(&metadata))
        {
            return Err("Official working directory is missing or redirected".into());
        }
        let mut artifacts = Vec::with_capacity(evidence.len() + 1);
        for path in std::iter::once(&uninstaller.executable).chain(evidence.iter()) {
            match std::fs::symlink_metadata(path) {
                Ok(metadata) if metadata.is_file() && !super::facts::is_link(&metadata) => {}
                _ => {
                    return Err(
                        "Official operation evidence is missing, redirected or not a file".into(),
                    )
                }
            }
            artifacts.push(InstallationArtifact {
                path: path.clone(),
                identity: Some(
                    crate::core::model::capture_identity(path)
                        .ok_or("Official operation identity unavailable")?,
                ),
                #[cfg(windows)]
                object_id: Some(
                    crate::platform::windows::identity::object_id(path)
                        .ok_or("Official operation stable identity unavailable")?,
                ),
            });
        }
        Ok(Self {
            uninstaller: uninstaller.clone(),
            evidence: artifacts,
            working_directory: InstallationInstance::capture(
                uninstaller.working_directory.clone(),
                std::slice::from_ref(&uninstaller.working_directory),
            )?,
        })
    }

    /// Returns the frozen command only while every scanned piece of evidence is unchanged.
    pub fn command(&self) -> Result<&OfficialUninstaller, String> {
        if self
            .working_directory
            .scanned_identity(&self.uninstaller.working_directory)?
            .is_none()
        {
            return Err("Official working directory disappeared since scan".into());
        }
        for artifact in &self.evidence {
            match std::fs::symlink_metadata(&artifact.path) {
                Ok(_) if artifact.unchanged() => {}
                _ => {
                    return Err(
                        "Official operation evidence changed or unavailable since scan".into(),
                    )
                }
            }
        }
        Ok(&self.uninstaller)
    }

    fn explanation(&self) -> serde_json::Value {
        serde_json::json!({
            "provider": self.uninstaller.provider,
            "executable": self.uninstaller.executable,
            "arguments": self.uninstaller.arguments.len(),
            "working_directory": self.uninstaller.working_directory,
            "evidence": self.evidence.iter().map(|artifact| &artifact.path).collect::<Vec<_>>(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct CleanupPlan {
    pub rule: RuleRef,
    pub targets: Vec<PlannedTarget>,
    pub preserve: Vec<PathBuf>,
    pub blocked: Vec<String>,
    pub observations: Vec<Arc<RuleObservation>>,
    pub installation: Option<InstallationInstance>,
    pub official: Option<OfficialOperation>,
}
impl CleanupPlan {
    pub fn scope(&self, index: usize) -> Option<TargetScope> {
        let target = self.targets.get(index)?;
        let extent = target.scope();
        let manifest = self.rule.manifest_membership();
        Some(if manifest && !target.operation.is_native_resource() {
            TargetScope::ManifestMember {
                path: target.path.clone(),
                extent: Box::new(extent),
            }
        } else {
            extent
        })
    }

    /// Dependencies are capability-owned; rules cannot reorder verification before mutation.
    pub fn steps(&self) -> Vec<PlanStep> {
        let mut steps = Vec::new();
        for (target, planned) in self.targets.iter().enumerate() {
            let actions = if planned.operation == Operation::OfficialUninstall
                && self.rule.snapshot.definition(&self.rule.id).app.is_some()
            {
                super::execution::SOURCE_ACTIONS
                    .into_iter()
                    .map(|action| PlanAction::SourceLifecycle { action })
                    .collect()
            } else {
                let mut actions = vec![
                    PlanAction::Revalidate,
                    PlanAction::Apply {
                        operation: planned.operation.clone(),
                    },
                ];
                if let Operation::GitWorktree { registration } = &planned.operation {
                    actions.push(PlanAction::Verify {
                        condition: CompletionCondition::PathAbsent {
                            path: planned.path.clone(),
                        },
                    });
                    actions.push(PlanAction::UnregisterWorktree {
                        registration: registration.clone(),
                    });
                }
                actions.push(PlanAction::Verify {
                    condition: planned.completion(),
                });
                actions
            };
            let first = steps.len();
            for action in actions {
                let id = steps.len();
                steps.push(PlanStep {
                    id,
                    target,
                    depends_on: if id == first { vec![] } else { vec![id - 1] },
                    action,
                });
            }
        }
        steps
    }

    pub fn new(rule: RuleRef, targets: Vec<PlannedTarget>) -> Self {
        let rule = rule.observed();
        let preserve = rule.preserved();
        let mut observations = Vec::new();
        rule.observations(&mut observations);
        let mut blocked = Vec::new();
        rule.blocked_reasons(&mut blocked);
        blocked.extend(
            observations
                .iter()
                .filter(|observation| observation.detected != super::facts::Evidence::Confirmed)
                .map(|observation| {
                    format!(
                        "Scan ownership evidence absent or unknown: {}",
                        observation.rule_id
                    )
                }),
        );
        Self {
            rule,
            targets,
            preserve,
            blocked,
            observations,
            installation: None,
            official: None,
        }
    }

    pub fn validate_binding(
        &self,
        path: &std::path::Path,
        operation: &Operation,
        disposal: crate::core::cleaner::Disposal,
        identity: Option<TargetIdentity>,
    ) -> Result<(), String> {
        if !self.targets.iter().any(|target| {
            target.operation.target_key(&target.path) == operation.target_key(path)
                && &target.operation == operation
                && target.disposal == disposal
                && target.identity == identity
        }) {
            return Err("Selection does not match the scanned plan".into());
        }
        self.validate()
    }

    /// Export is diagnostic only; it cannot be deserialized into execution authority.
    pub fn explanation(&self) -> serde_json::Value {
        serde_json::json!({
            "observations": self.observations,
            "installation": self.installation.as_ref().map(InstallationInstance::explanation),
            "official": self.official.as_ref().map(OfficialOperation::explanation),
            "targets": self.targets.iter().enumerate().map(|(index, target)| serde_json::json!({
                "path": target.path, "operation": target.operation, "disposal": target.disposal,
                "identity_captured": target.identity.is_some(), "scope": self.scope(index),
                "completion": target.completion()
            })).collect::<Vec<_>>(),
            "steps": self.steps(),
            "preserve": self.preserve, "blocked": self.blocked
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        self.rule.revalidate()?;
        if !self.blocked.is_empty() {
            return Err(self.blocked.join("; "));
        }
        if let Some(instance) = &self.installation {
            instance.revalidate()?;
        }
        if let Some(official) = &self.official {
            official.command()?;
        }
        let mut preserve = self.preserve.clone();
        preserve.extend(self.rule.preserved_live());
        for (index, target) in self.targets.iter().enumerate() {
            self.rule.authorizes(&target.path)?;
            if matches!(
                target.operation,
                Operation::File
                    | Operation::Tree
                    | Operation::Contents
                    | Operation::Go
                    | Operation::Pnpm
                    | Operation::GitWorktree { .. }
            ) {
                let protected = if target.operation == Operation::Contents {
                    crate::core::safety::is_contents_protected(&target.path)
                } else {
                    crate::core::safety::is_protected(&target.path)
                };
                if protected {
                    return Err("Protected target".into());
                }
                if target.identity.is_none() {
                    return Err("Missing target identity".into());
                }
                if !target
                    .identity
                    .is_some_and(|identity| identity.recheck(&target.path))
                {
                    return Err("Target identity changed or unavailable".into());
                }
                let target_path = norm(&target.path);
                // Two different refusals: a target inside a preserved path must not be
                // narrowed into it, and a target covering a preserved path cannot be
                // split here — both keep the target out of the plan with its reason.
                if let Some(keep) = preserve
                    .iter()
                    .find(|keep| at_or_under(&target_path, &norm(keep)))
                {
                    return Err(format!(
                        "Target is inside a preserved path: {}",
                        keep.display()
                    ));
                }
                if let Some(keep) = preserve
                    .iter()
                    .find(|keep| at_or_under(&norm(keep), &target_path))
                {
                    return Err(format!(
                        "Target covers a preserved path: {}",
                        keep.display()
                    ));
                }
            }
            for other in &self.targets[..index] {
                if norm(&target.path) == norm(&other.path) && target.operation != other.operation {
                    return Err("Conflicting target operations".into());
                }
            }
        }
        Ok(())
    }
}
