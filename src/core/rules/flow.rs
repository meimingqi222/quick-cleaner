//! Capability-owned dependencies and completion checks; reports are diagnostic, not authority.
use super::facts::Evidence;
use super::plan::PlanAction;
use super::{CleanupPlan, CompletionCondition, Operation, PlanStep, PlannedTarget};
use crate::core::cleaner::{CleanProgress, CleanReport, CleanResult};
use crate::core::inuse::SpotCheck;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Succeeded,
    Failed,
    Blocked,
    Unknown,
    Cancelled,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct StepResult {
    pub step: PlanStep,
    pub status: StepStatus,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct PlanExecution {
    pub rule_id: String,
    pub rule_version: u32,
    pub schema: u32,
    pub sequence: u64,
    pub target: usize,
    pub steps: Vec<StepResult>,
}

/// Only NotFound confirms absence. A failed stat is not a completion proof.
pub fn filesystem_completion(condition: &CompletionCondition) -> Evidence {
    match condition {
        CompletionCondition::PathAbsent { path } => match std::fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Evidence::Confirmed,
            Err(_) => Evidence::Unknown,
            Ok(_) => Evidence::Absent,
        },
        CompletionCondition::ContentsEmpty { path } => {
            let Ok(metadata) = std::fs::symlink_metadata(path) else {
                return Evidence::Unknown;
            };
            if !metadata.is_dir()
                || metadata.file_type().is_symlink()
                || crate::core::model::capture_identity(path).is_none()
            {
                return Evidence::Unknown;
            }
            match std::fs::read_dir(path) {
                Ok(mut entries) => match entries.next() {
                    None => Evidence::Confirmed,
                    Some(Ok(_)) => Evidence::Absent,
                    Some(Err(_)) => Evidence::Unknown,
                },
                Err(_) => Evidence::Unknown,
            }
        }
        _ => Evidence::Unknown,
    }
}

pub(crate) fn execute_filesystem(
    plan: &CleanupPlan,
    target: usize,
    progress: &CleanProgress,
    occupancy: SpotCheck,
) -> CleanReport {
    execute_filesystem_with(
        plan,
        target,
        progress,
        occupancy,
        |path, operation, disposal| {
            if *operation == Operation::Contents {
                crate::core::cleaner::clean_dir_contents(path, progress)
            } else {
                let mut report = CleanReport::default();
                report.record(
                    path,
                    crate::core::cleaner::dispose(path, disposal, progress),
                );
                report
            }
        },
    )
}

fn execute_filesystem_with(
    plan: &CleanupPlan,
    index: usize,
    progress: &CleanProgress,
    occupancy: SpotCheck,
    mut apply: impl FnMut(&Path, &Operation, crate::core::cleaner::Disposal) -> CleanReport,
) -> CleanReport {
    execute_capability(
        plan,
        index,
        progress,
        occupancy,
        &mut FileExecutor { apply: &mut apply },
    )
}

trait CapabilityExecutor {
    fn supports(&self, target: &PlannedTarget) -> bool;
    fn requires_file_occupancy(&self) -> bool {
        true
    }
    fn prepare(&mut self, _target: &PlannedTarget) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn effective_action(&self, action: PlanAction, _target: &PlannedTarget) -> PlanAction {
        action
    }
    fn apply(&mut self, target: &PlannedTarget) -> CleanReport;
    fn verify(&mut self, condition: &CompletionCondition) -> Evidence;
    fn failure_reason(&self) -> Option<String> {
        None
    }
    fn unregister_worktree(
        &mut self,
        _target: &PlannedTarget,
        _registration: &Path,
    ) -> Result<(), String> {
        Err("Step requires its worktree capability executor".into())
    }
    fn source_action(&mut self, _action: super::execution::SourceAction) -> Result<(), String> {
        Err("Step requires its source capability executor".into())
    }
}

#[cfg(any(windows, test))]
pub(crate) fn execute_source(
    plan: &CleanupPlan,
    index: usize,
    progress: &CleanProgress,
    run: impl FnMut(super::execution::SourceAction) -> Result<(), String>,
) -> CleanReport {
    execute_capability(
        plan,
        index,
        progress,
        SpotCheck::Unknown,
        &mut SourceExecutor(run),
    )
}
#[cfg(any(windows, test))]
struct SourceExecutor<F>(F);
#[cfg(any(windows, test))]
impl<F: FnMut(super::execution::SourceAction) -> Result<(), String>> CapabilityExecutor
    for SourceExecutor<F>
{
    fn supports(&self, target: &PlannedTarget) -> bool {
        target.operation == Operation::OfficialUninstall
            && target.disposal == crate::core::cleaner::Disposal::Permanent
    }
    fn requires_file_occupancy(&self) -> bool {
        false
    }
    fn apply(&mut self, target: &PlannedTarget) -> CleanReport {
        let mut report = CleanReport::default();
        report.record(&target.path, CleanResult::Failed);
        report
    }
    fn verify(&mut self, _: &CompletionCondition) -> Evidence {
        Evidence::Unknown
    }
    fn source_action(&mut self, action: super::execution::SourceAction) -> Result<(), String> {
        (self.0)(action)
    }
}

#[cfg(any(windows, test))]
/// Run a command-based uninstall (a registered app's official uninstaller) through the
/// shared capability runner, so the UI gets the same step evidence as source installs.
///
/// `run` performs the official operation; `verify` answers the completion condition
/// (registration/artifacts gone). The plan's target is `OfficialUninstall`, whose
/// `Revalidate` step validates the plan and then the frozen command is run.
pub(crate) fn execute_registered(
    plan: &CleanupPlan,
    index: usize,
    progress: &CleanProgress,
    run: impl FnMut() -> Result<(), String>,
    verify: impl FnMut(&CompletionCondition) -> Evidence,
) -> CleanReport {
    execute_capability(
        plan,
        index,
        progress,
        SpotCheck::Unknown,
        &mut RegisteredExecutor {
            run,
            verify,
            error: None,
        },
    )
}

#[cfg(any(windows, test))]
struct RegisteredExecutor<R, V> {
    run: R,
    verify: V,
    error: Option<String>,
}
#[cfg(any(windows, test))]
impl<R: FnMut() -> Result<(), String>, V: FnMut(&CompletionCondition) -> Evidence>
    CapabilityExecutor for RegisteredExecutor<R, V>
{
    fn supports(&self, target: &PlannedTarget) -> bool {
        target.operation == Operation::OfficialUninstall
            && target.disposal == crate::core::cleaner::Disposal::Permanent
    }
    fn requires_file_occupancy(&self) -> bool {
        false
    }
    fn apply(&mut self, target: &PlannedTarget) -> CleanReport {
        let mut report = CleanReport::default();
        match (self.run)() {
            Ok(()) => report.record(&target.path, CleanResult::Ok),
            Err(reason) => {
                self.error = Some(reason);
                report.record(&target.path, CleanResult::Failed);
            }
        }
        report
    }
    fn verify(&mut self, condition: &CompletionCondition) -> Evidence {
        (self.verify)(condition)
    }
    fn failure_reason(&self) -> Option<String> {
        self.error.clone()
    }
}

#[cfg(any(windows, test))]
/// Run a native-residue cleanup (registry key/value, scheduled task or system
/// extension) through the shared capability runner, so the residual channel gets the
/// same `Revalidate → Apply → Verify` step evidence as every other entry.
///
/// `apply` performs the kind-specific native deletion for a `Operation::Native` target;
/// `verify` answers its completion condition (`RegistrationAbsent`, whose identifier is
/// the target's native id).
pub(crate) fn execute_native_residual(
    plan: &CleanupPlan,
    index: usize,
    progress: &CleanProgress,
    apply: impl FnMut(&PlannedTarget) -> Result<(), String>,
    verify: impl FnMut(&CompletionCondition) -> Evidence,
) -> CleanReport {
    execute_capability(
        plan,
        index,
        progress,
        SpotCheck::Unknown,
        &mut NativeResidualExecutor {
            apply,
            verify,
            error: None,
        },
    )
}

#[cfg(any(windows, test))]
struct NativeResidualExecutor<A, V> {
    apply: A,
    verify: V,
    error: Option<String>,
}
#[cfg(any(windows, test))]
impl<A, V> CapabilityExecutor for NativeResidualExecutor<A, V>
where
    A: FnMut(&PlannedTarget) -> Result<(), String>,
    V: FnMut(&CompletionCondition) -> Evidence,
{
    fn supports(&self, target: &PlannedTarget) -> bool {
        matches!(target.operation, Operation::Native { .. })
    }
    fn requires_file_occupancy(&self) -> bool {
        false
    }
    fn apply(&mut self, target: &PlannedTarget) -> CleanReport {
        let mut report = CleanReport::default();
        match (self.apply)(target) {
            Ok(()) => report.record(&target.path, CleanResult::Ok),
            Err(reason) => {
                self.error = Some(reason);
                report.record(&target.path, CleanResult::Failed);
            }
        }
        report
    }
    fn verify(&mut self, condition: &CompletionCondition) -> Evidence {
        (self.verify)(condition)
    }
    fn failure_reason(&self) -> Option<String> {
        self.error.clone()
    }
}

#[cfg(any(windows, test))]
pub(crate) fn execution_result(report: &CleanReport) -> Result<(), String> {
    let steps: Vec<_> = report
        .plan_executions
        .iter()
        .flat_map(|execution| &execution.steps)
        .collect();
    if steps.is_empty() {
        return Err("No capability execution was reported".into());
    }
    if let Some(step) = steps
        .iter()
        .find(|step| step.status != StepStatus::Succeeded)
    {
        return Err(step
            .reason
            .clone()
            .unwrap_or_else(|| format!("Capability step {:?}", step.status)));
    }
    if !report.failed.is_empty() || !report.skipped_items.is_empty() || !report.manual.is_empty() {
        return Err("Capability reported incomplete cleanup".into());
    }
    Ok(())
}

pub(crate) fn execute_owner(
    plan: &CleanupPlan,
    index: usize,
    progress: &CleanProgress,
    occupancy: SpotCheck,
    size_hint: Option<u64>,
) -> CleanReport {
    execute_capability(
        plan,
        index,
        progress,
        occupancy,
        &mut OwnerExecutor {
            progress,
            size_hint,
            resources: SystemOwner(None),
            fallback: false,
            error: None,
        },
    )
}

trait OwnerResources {
    fn prepare(&mut self, target: &PlannedTarget) -> bool;
    fn apply(&mut self) -> Result<(), String>;
    fn completion(&mut self) -> Evidence;
}
struct SystemOwner(Option<crate::core::owner::PreparedOwner>);
impl OwnerResources for SystemOwner {
    fn prepare(&mut self, target: &PlannedTarget) -> bool {
        self.0 = crate::core::owner::PreparedOwner::prepare(&target.path, &target.operation);
        self.0.is_some()
    }
    fn apply(&mut self) -> Result<(), String> {
        self.0
            .as_ref()
            .ok_or_else(|| "Owner route was not prepared".to_string())?
            .apply()
    }
    fn completion(&mut self) -> Evidence {
        self.0
            .as_ref()
            .map_or(Evidence::Unknown, |owner| owner.completion())
    }
}
struct OwnerExecutor<'a, R> {
    progress: &'a CleanProgress,
    size_hint: Option<u64>,
    resources: R,
    fallback: bool,
    error: Option<String>,
}
impl<R: OwnerResources> CapabilityExecutor for OwnerExecutor<'_, R> {
    fn supports(&self, target: &PlannedTarget) -> bool {
        matches!(target.operation, Operation::Go | Operation::Pnpm)
            && target.disposal == crate::core::cleaner::Disposal::Permanent
    }
    fn prepare(&mut self, target: &PlannedTarget) -> Result<Option<String>, String> {
        self.fallback = !self.resources.prepare(target);
        Ok(self.fallback.then(|| {
            "Owner preflight unavailable or scope mismatched; using authorized contents cleanup"
                .into()
        }))
    }
    fn effective_action(&self, action: PlanAction, target: &PlannedTarget) -> PlanAction {
        if !self.fallback {
            return action;
        }
        match action {
            PlanAction::Apply { .. } => PlanAction::Apply {
                operation: Operation::Contents,
            },
            PlanAction::Verify { .. } => PlanAction::Verify {
                condition: CompletionCondition::ContentsEmpty {
                    path: target.path.clone(),
                },
            },
            _ => action,
        }
    }
    fn apply(&mut self, target: &PlannedTarget) -> CleanReport {
        if !crate::core::cleaner::root_identity_holds(&target.path, target.identity)
            || crate::core::safety::is_protected(&target.path)
        {
            self.error =
                Some("Target identity or protection changed during owner preflight".into());
            let mut report = CleanReport::default();
            self.progress
                .failed
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            report.record(&target.path, CleanResult::Failed);
            return report;
        }
        if self.fallback {
            return crate::core::cleaner::clean_dir_contents(&target.path, self.progress);
        }
        let mut report = CleanReport::default();
        let result = match self.resources.apply() {
            Ok(()) => CleanResult::Ok,
            Err(reason) => {
                self.error = Some(reason);
                self.progress
                    .failed
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                CleanResult::Failed
            }
        };
        report.record(&target.path, result);
        report
    }
    fn verify(&mut self, condition: &CompletionCondition) -> Evidence {
        if self.fallback {
            return filesystem_completion(condition);
        }
        let evidence = self.resources.completion();
        if evidence == Evidence::Confirmed {
            self.progress
                .files
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.progress.bytes.fetch_add(
                self.size_hint.unwrap_or(0),
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        evidence
    }
    fn failure_reason(&self) -> Option<String> {
        self.error.clone()
    }
}

struct FileExecutor<F> {
    apply: F,
}

pub(crate) fn execute_worktree(
    plan: &CleanupPlan,
    index: usize,
    progress: &CleanProgress,
    occupancy: SpotCheck,
) -> CleanReport {
    execute_worktree_with(plan, index, progress, occupancy, |target| {
        let mut report = CleanReport::default();
        report.record(
            &target.path,
            crate::core::cleaner::dispose(&target.path, target.disposal, progress),
        );
        report
    })
}

pub(crate) fn execute_worktree_with(
    plan: &CleanupPlan,
    index: usize,
    progress: &CleanProgress,
    occupancy: SpotCheck,
    apply: impl FnMut(&PlannedTarget) -> CleanReport,
) -> CleanReport {
    execute_capability(
        plan,
        index,
        progress,
        occupancy,
        &mut WorktreeExecutor { entry: None, apply },
    )
}

struct WorktreeExecutor<F> {
    entry: Option<crate::core::worktrees::Registration>,
    apply: F,
}
impl<F: FnMut(&PlannedTarget) -> CleanReport> CapabilityExecutor for WorktreeExecutor<F> {
    fn supports(&self, target: &PlannedTarget) -> bool {
        matches!(target.operation, Operation::GitWorktree { .. })
            && target.disposal == crate::core::cleaner::Disposal::Permanent
    }
    fn prepare(&mut self, target: &PlannedTarget) -> Result<Option<String>, String> {
        use crate::core::cleaner::FailReason;
        let Operation::GitWorktree { registration } = &target.operation else {
            return Err("Not a worktree operation".into());
        };
        let entry = crate::core::worktrees::inspect(&target.path)
            .ok()
            .filter(|entry| &entry.admin == registration);
        let readiness = entry
            .as_ref()
            .map_or(Err(FailReason::WorktreeUnverified), |entry| {
                entry.check_ready()
            });
        if let Err(reason) = readiness {
            crate::core::cleaner::record_fail_reason(&target.path, reason);
            return Err(format!("Worktree preflight: {reason:?}"));
        }
        self.entry = entry;
        Ok(None)
    }
    fn apply(&mut self, target: &PlannedTarget) -> CleanReport {
        if !crate::core::cleaner::root_identity_holds(&target.path, target.identity)
            || crate::core::safety::is_protected(&target.path)
            || crate::core::worktrees::inspect(&target.path).ok().as_ref() != self.entry.as_ref()
        {
            crate::core::cleaner::record_fail_reason(
                &target.path,
                crate::core::cleaner::FailReason::WorktreeUnverified,
            );
            let mut report = CleanReport::default();
            report.record(&target.path, CleanResult::Failed);
            return report;
        }
        (self.apply)(target)
    }
    fn unregister_worktree(
        &mut self,
        target: &PlannedTarget,
        registration: &Path,
    ) -> Result<(), String> {
        let entry = self
            .entry
            .as_ref()
            .filter(|entry| entry.admin == registration)
            .ok_or("Exact worktree registration was not prepared")?;
        entry.unregister_action().inspect_err(|_| {
            crate::core::cleaner::record_fail_reason(
                &target.path,
                crate::core::cleaner::FailReason::WorktreeUnverified,
            );
        })
    }
    fn verify(&mut self, condition: &CompletionCondition) -> Evidence {
        match condition {
            CompletionCondition::PathAbsent { .. } => filesystem_completion(condition),
            CompletionCondition::WorktreeAndRegistrationAbsent { registration, .. } => self
                .entry
                .as_ref()
                .filter(|entry| &entry.admin == registration)
                .map_or(Evidence::Unknown, |entry| entry.completion()),
            _ => Evidence::Unknown,
        }
    }
}
impl<F: FnMut(&Path, &Operation, crate::core::cleaner::Disposal) -> CleanReport> CapabilityExecutor
    for FileExecutor<F>
{
    fn supports(&self, target: &PlannedTarget) -> bool {
        matches!(
            target.operation,
            Operation::File | Operation::Tree | Operation::Contents
        ) && (target.operation != Operation::Contents
            || target.disposal == crate::core::cleaner::Disposal::Permanent)
    }
    fn apply(&mut self, target: &PlannedTarget) -> CleanReport {
        (self.apply)(&target.path, &target.operation, target.disposal)
    }
    fn verify(&mut self, condition: &CompletionCondition) -> Evidence {
        filesystem_completion(condition)
    }
}

pub(crate) fn execute_native(
    plan: &CleanupPlan,
    target: usize,
    progress: &CleanProgress,
    size_hint: Option<u64>,
) -> CleanReport {
    execute_capability(
        plan,
        target,
        progress,
        SpotCheck::Unknown,
        &mut NativeExecutor {
            progress,
            size_hint,
            layers_deleted: false,
            error: None,
            resources: SystemResources,
        },
    )
}

trait NativeResources {
    fn mutate(&mut self, operation: &Operation) -> Result<bool, String>;
    fn completion(&mut self, condition: &CompletionCondition) -> Evidence;
    fn completed(&mut self, _condition: &CompletionCondition) {}
}
struct SystemResources;
impl NativeResources for SystemResources {
    fn mutate(&mut self, operation: &Operation) -> Result<bool, String> {
        match operation {
            Operation::Docker { reference } => crate::core::docker::remove_image_action(reference),
            Operation::Brew => crate::core::brew::cleanup_action().map(|()| false),
            Operation::Snapshot { name } if super::execution::remove_snapshot_action(name) => {
                Ok(false)
            }
            _ => Err("Native operation failed or unavailable".into()),
        }
    }
    fn completion(&mut self, condition: &CompletionCondition) -> Evidence {
        match condition {
            CompletionCondition::DockerReferenceAbsent { reference } => {
                crate::core::docker::reference_absence(reference)
            }
            CompletionCondition::SnapshotAbsent { name } => {
                super::execution::snapshot_absence(name)
            }
            CompletionCondition::BrewPreviewEmpty => crate::core::brew::cleanup_completion(),
            _ => Evidence::Unknown,
        }
    }
    fn completed(&mut self, condition: &CompletionCondition) {
        if *condition == CompletionCondition::BrewPreviewEmpty {
            crate::core::brew::record_cleanup();
        }
    }
}
struct NativeExecutor<'a, R> {
    progress: &'a CleanProgress,
    size_hint: Option<u64>,
    layers_deleted: bool,
    error: Option<String>,
    resources: R,
}
impl<R: NativeResources> CapabilityExecutor for NativeExecutor<'_, R> {
    fn supports(&self, target: &PlannedTarget) -> bool {
        matches!(
            target.operation,
            Operation::Docker { .. } | Operation::Snapshot { .. } | Operation::Brew
        ) && target.disposal == crate::core::cleaner::Disposal::Permanent
    }
    fn requires_file_occupancy(&self) -> bool {
        false
    }
    fn apply(&mut self, target: &PlannedTarget) -> CleanReport {
        let mut report = CleanReport::default();
        let succeeded = match self.resources.mutate(&target.operation) {
            Ok(deleted) => {
                self.layers_deleted = deleted;
                true
            }
            Err(reason) => {
                self.error = Some(reason);
                false
            }
        };
        report.record(
            &target.path,
            if succeeded {
                CleanResult::Ok
            } else {
                CleanResult::Failed
            },
        );
        if !succeeded {
            self.progress
                .failed
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        report
    }
    fn verify(&mut self, condition: &CompletionCondition) -> Evidence {
        let evidence = self.resources.completion(condition);
        if evidence == Evidence::Confirmed {
            self.resources.completed(condition);
            self.progress
                .files
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if (self.layers_deleted
                && matches!(condition, CompletionCondition::DockerReferenceAbsent { .. }))
                || *condition == CompletionCondition::BrewPreviewEmpty
            {
                self.progress.bytes.fetch_add(
                    self.size_hint.unwrap_or(0),
                    std::sync::atomic::Ordering::Relaxed,
                );
            }
        }
        evidence
    }
    fn failure_reason(&self) -> Option<String> {
        self.error.clone()
    }
}

fn execution_header(plan: &CleanupPlan, index: usize) -> PlanExecution {
    PlanExecution {
        rule_id: plan.rule.id.clone(),
        rule_version: plan.rule.snapshot.definition(&plan.rule.id).version,
        schema: plan.rule.snapshot.bundle.schema,
        sequence: plan.rule.snapshot.bundle.sequence,
        target: index,
        steps: vec![],
    }
}

#[cfg(windows)]
pub(crate) fn blocked_execution(plan: &CleanupPlan, index: usize, reason: &str) -> PlanExecution {
    let mut execution = execution_header(plan, index);
    for step in plan.steps().into_iter().filter(|step| step.target == index) {
        execution.steps.push(StepResult {
            step,
            status: StepStatus::Blocked,
            reason: Some(if execution.steps.is_empty() {
                reason.into()
            } else {
                "Dependency did not succeed".into()
            }),
        });
    }
    execution
}

fn execute_capability(
    plan: &CleanupPlan,
    index: usize,
    progress: &CleanProgress,
    occupancy: SpotCheck,
    capability: &mut impl CapabilityExecutor,
) -> CleanReport {
    let mut report = CleanReport::default();
    let Some(target) = plan.targets.get(index) else {
        report.record_target(
            crate::core::cleaner::CleanFailure::Id(format!("plan:{}:target:{index}", plan.rule.id)),
            CleanResult::Failed,
        );
        return report;
    };
    let mut execution = execution_header(plan, index);
    let mut dependency_failed = false;
    for mut step in plan.steps().into_iter().filter(|step| step.target == index) {
        step.action = capability.effective_action(step.action, target);
        let (status, reason) = if dependency_failed {
            (
                StepStatus::Blocked,
                Some("Dependency did not succeed".into()),
            )
        } else if progress.cancelled() {
            (StepStatus::Cancelled, Some("Cancelled before step".into()))
        } else {
            match &step.action {
                PlanAction::Revalidate => {
                    // Command operations (official uninstall) have no filesystem scope to
                    // validate; their capability's supports() is the authority. Everything
                    // else must still pass the typed filesystem target check.
                    let filesystem_scoped = target.operation != Operation::OfficialUninstall;
                    let supported = capability.supports(target)
                        && (!filesystem_scoped
                            || target.operation.validate_target(
                                &target.path,
                                target.operation != Operation::Contents,
                            ));
                    if capability.requires_file_occupancy() && occupancy != SpotCheck::Clear {
                        (
                            if occupancy == SpotCheck::Unknown {
                                StepStatus::Unknown
                            } else {
                                StepStatus::Blocked
                            },
                            Some("Target occupancy is busy or unconfirmed".into()),
                        )
                    } else if !supported {
                        (
                            StepStatus::Blocked,
                            Some("Operation requires its native capability executor".into()),
                        )
                    } else if let Err(reason) = plan.validate() {
                        (StepStatus::Blocked, Some(reason))
                    } else {
                        match capability.prepare(target) {
                            Ok(reason) => (StepStatus::Succeeded, reason),
                            Err(reason) => (StepStatus::Blocked, Some(reason)),
                        }
                    }
                }
                PlanAction::Apply { .. } => {
                    report = capability.apply(target);
                    if !report.failed.is_empty() {
                        (
                            StepStatus::Failed,
                            capability
                                .failure_reason()
                                .or_else(|| Some("Cleanup reported a failure".into())),
                        )
                    } else if !report.skipped_items.is_empty() || !report.manual.is_empty() {
                        (
                            StepStatus::Blocked,
                            Some("Cleanup skipped or requires manual action".into()),
                        )
                    } else {
                        (StepStatus::Succeeded, None)
                    }
                }
                PlanAction::Verify { condition } => match capability.verify(condition) {
                    Evidence::Confirmed => (StepStatus::Succeeded, None),
                    Evidence::Absent => (
                        StepStatus::Failed,
                        Some("Cleanup completion condition is not met".into()),
                    ),
                    Evidence::Unknown => (
                        StepStatus::Unknown,
                        Some("Cleanup completion could not be confirmed".into()),
                    ),
                },
                PlanAction::UnregisterWorktree { registration } => {
                    match capability.unregister_worktree(target, registration) {
                        Ok(()) => (StepStatus::Succeeded, None),
                        Err(reason) => (StepStatus::Failed, Some(reason)),
                    }
                }
                PlanAction::SourceLifecycle { action } => {
                    let validation = if *action == super::execution::SourceAction::Revalidate {
                        if !capability.supports(target) {
                            Err("Unsupported source operation".into())
                        } else if plan.installation.is_none() {
                            Err("Missing captured installation instance".into())
                        } else {
                            plan.validate()
                        }
                    } else {
                        Ok(())
                    };
                    match validation {
                        Err(reason) => (StepStatus::Blocked, Some(reason)),
                        Ok(()) => match capability.source_action(*action) {
                            Ok(()) => (StepStatus::Succeeded, None),
                            Err(reason) => (StepStatus::Failed, Some(reason)),
                        },
                    }
                }
            }
        };
        dependency_failed |= status != StepStatus::Succeeded;
        execution.steps.push(StepResult {
            step,
            status,
            reason,
        });
    }
    if dependency_failed
        && report.failed.is_empty()
        && report.skipped_items.is_empty()
        && report.manual.is_empty()
    {
        report.ok = 0;
        if !progress.cancelled() {
            progress
                .failed
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        report.record(
            &target.path,
            if progress.cancelled() {
                CleanResult::Skipped
            } else {
                CleanResult::Failed
            },
        );
    }
    report.plan_executions.push(execution);
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cleaner::Disposal;
    use crate::core::rules::{PlannedTarget, RuleRef};

    /// 注册表应用的卸载走共用执行器：产出 Revalidate → Apply → Verify 三步报告，
    /// 失败原因保留在 Apply 步，完成条件不满足时 Verify 步失败。
    #[test]
    fn registered_runner_reports_revalidate_apply_verify_steps() {
        let plan = CleanupPlan::new(
            RuleRef::engine(),
            vec![PlannedTarget {
                path: std::path::PathBuf::from(r"C:\fixture\registered-app"),
                operation: Operation::OfficialUninstall,
                identity: None,
                disposal: Disposal::Permanent,
            }],
        );
        let progress = CleanProgress::new(1, 0);

        let ok = execute_registered(&plan, 0, &progress, || Ok(()), |_| Evidence::Confirmed);
        let steps = &ok.plan_executions[0].steps;
        assert_eq!(steps.len(), 3);
        assert!(steps
            .iter()
            .all(|step| step.status == StepStatus::Succeeded));
        assert!(execution_result(&ok).is_ok());

        let failed = execute_registered(
            &plan,
            0,
            &progress,
            || Err("boom".into()),
            |_| Evidence::Confirmed,
        );
        assert_eq!(
            failed.plan_executions[0].steps[1].reason.as_deref(),
            Some("boom")
        );
        assert_eq!(
            failed.plan_executions[0].steps[2].status,
            StepStatus::Blocked
        );
        assert_eq!(execution_result(&failed), Err("boom".to_string()));

        let incomplete = execute_registered(&plan, 0, &progress, || Ok(()), |_| Evidence::Absent);
        assert_eq!(
            incomplete.plan_executions[0].steps[2].status,
            StepStatus::Failed
        );
    }

    /// 原生残留目标走共用执行器：Revalidate → Apply → Verify 三步；失败原因保留、
    /// 完成条件不满足时 Verify 失败。
    #[test]
    fn native_residual_runner_reports_revalidate_apply_verify_steps() {
        let target = PlannedTarget {
            path: std::path::PathBuf::from(r"HKCU\Software\Vendor"),
            operation: Operation::Native {
                native: crate::core::rules::NativeKind::RegistryKey,
                identifier: r"HKCU\Software\Vendor".into(),
            },
            identity: None,
            disposal: Disposal::Permanent,
        };
        let plan = CleanupPlan::new(RuleRef::engine(), vec![target]);
        let progress = CleanProgress::new(1, 0);

        let ok = execute_native_residual(&plan, 0, &progress, |_| Ok(()), |_| Evidence::Confirmed);
        let steps = &ok.plan_executions[0].steps;
        assert_eq!(steps.len(), 3);
        assert!(steps
            .iter()
            .all(|step| step.status == StepStatus::Succeeded));
        assert!(execution_result(&ok).is_ok());

        let failed = execute_native_residual(
            &plan,
            0,
            &progress,
            |_| Err("boom".into()),
            |_| Evidence::Confirmed,
        );
        assert_eq!(
            failed.plan_executions[0].steps[1].reason.as_deref(),
            Some("boom")
        );
        assert_eq!(execution_result(&failed), Err("boom".to_string()));

        let absent = execute_native_residual(&plan, 0, &progress, |_| Ok(()), |_| Evidence::Absent);
        assert_eq!(
            absent.plan_executions[0].steps[2].status,
            StepStatus::Failed
        );
    }

    #[test]
    fn source_runner_reports_fixed_dependencies_and_preserves_failure_reasons() {
        use super::super::execution::{SourceAction, SOURCE_ACTIONS};
        let root = crate::core::testing::fixture("source_flow_steps");
        let code = root.join("hermes-agent");
        std::fs::create_dir(&code).unwrap();
        let recovery = root.join("retry-record");
        std::fs::write(&recovery, b"retry").unwrap();
        let mut plan = CleanupPlan::new(
            RuleRef::new("hermes", Some(root.clone())),
            vec![PlannedTarget {
                path: code.clone(),
                operation: Operation::OfficialUninstall,
                identity: crate::core::model::capture_identity(&code),
                disposal: Disposal::Permanent,
            }],
        );
        plan.installation = Some(
            super::super::InstallationInstance::capture(code, std::slice::from_ref(&recovery))
                .unwrap(),
        );
        for failure in 0..=SOURCE_ACTIONS.len() {
            let progress = CleanProgress::default();
            let mut calls = Vec::new();
            let report = execute_source(&plan, 0, &progress, |action| {
                calls.push(action);
                if calls.len() - 1 == failure {
                    Err("fixture source step failure".into())
                } else {
                    Ok(())
                }
            });
            let execution = &report.plan_executions[0];
            assert_eq!(execution.rule_id, "hermes");
            assert_eq!(
                execution.rule_version,
                plan.rule.snapshot.definition("hermes").version
            );
            assert_eq!(execution.sequence, plan.rule.snapshot.bundle.sequence);
            assert_eq!(execution.steps.len(), 5);
            if failure < SOURCE_ACTIONS.len() {
                assert_eq!(calls, SOURCE_ACTIONS[..=failure]);
                assert_eq!(execution.steps[failure].status, StepStatus::Failed);
                assert!(execution.steps[failure + 1..]
                    .iter()
                    .all(|step| step.status == StepStatus::Blocked));
                assert_eq!(
                    execution_result(&report).unwrap_err(),
                    "fixture source step failure"
                );
                assert!(recovery.is_file());
            } else {
                assert_eq!(calls, SOURCE_ACTIONS);
                assert_eq!(execution_result(&report), Ok(()));
            }
        }
        let progress = CleanProgress::default();
        let mut calls = Vec::new();
        let report = execute_source(&plan, 0, &progress, |action| {
            calls.push(action);
            if action == SourceAction::OfficialOperation {
                progress.request_cancel();
            }
            Ok(())
        });
        assert_eq!(calls, SOURCE_ACTIONS[..2]);
        assert_eq!(
            report.plan_executions[0].steps[2].status,
            StepStatus::Cancelled
        );
        assert_eq!(
            report.plan_executions[0].steps[4].status,
            StepStatus::Blocked
        );
        assert!(execution_result(&report).is_err());
        plan.blocked.push("frozen ownership unknown".into());
        let report = execute_source(&plan, 0, &CleanProgress::default(), |_| {
            panic!("blocked plans cannot call native source actions")
        });
        assert_eq!(
            report.plan_executions[0].steps[0].status,
            StepStatus::Blocked
        );
        assert_eq!(
            execution_result(&report).unwrap_err(),
            "frozen ownership unknown"
        );
        plan.blocked.clear();
        plan.installation = None;
        let report = execute_source(&plan, 0, &CleanProgress::default(), |_| {
            panic!("missing scan facts cannot authorize native source actions")
        });
        assert_eq!(
            execution_result(&report).unwrap_err(),
            "Missing captured installation instance"
        );
        assert!(recovery.is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn plan(path: &Path, operation: Operation) -> CleanupPlan {
        CleanupPlan::new(
            RuleRef::engine(),
            vec![PlannedTarget {
                path: path.into(),
                operation,
                identity: crate::core::model::capture_identity(path),
                disposal: Disposal::Permanent,
            }],
        )
    }

    fn statuses(report: &CleanReport) -> Vec<StepStatus> {
        report.plan_executions[0]
            .steps
            .iter()
            .map(|result| result.status)
            .collect()
    }

    struct OwnerFixture {
        prepared: bool,
        fail: bool,
        evidence: Evidence,
        calls: Vec<&'static str>,
    }
    impl OwnerResources for OwnerFixture {
        fn prepare(&mut self, _: &PlannedTarget) -> bool {
            self.calls.push("prepare");
            self.prepared
        }
        fn apply(&mut self) -> Result<(), String> {
            self.calls.push("apply");
            if self.fail {
                Err("fixture timeout after attempt".into())
            } else {
                Ok(())
            }
        }
        fn completion(&mut self) -> Evidence {
            self.calls.push("verify");
            self.evidence
        }
    }
    fn owner_fixture(prepared: bool, fail: bool, evidence: Evidence) -> OwnerFixture {
        OwnerFixture {
            prepared,
            fail,
            evidence,
            calls: vec![],
        }
    }

    #[test]
    fn owner_runner_records_fallback_extent_and_preserves_root() {
        for (operation, suffix) in [
            (Operation::Go, "go/pkg/mod"),
            (Operation::Pnpm, ".pnpm-store"),
        ] {
            let root = crate::core::testing::fixture("owner_flow_fallback");
            let path = root.join(suffix);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("obsolete"), b"cache").unwrap();
            let progress = CleanProgress::default();
            let mut executor = OwnerExecutor {
                progress: &progress,
                size_hint: Some(999),
                resources: owner_fixture(false, false, Evidence::Unknown),
                fallback: false,
                error: None,
            };
            let report = execute_capability(
                &plan(&path, operation),
                0,
                &progress,
                SpotCheck::Clear,
                &mut executor,
            );
            assert_eq!(statuses(&report), vec![StepStatus::Succeeded; 3]);
            assert_eq!(executor.resources.calls, ["prepare"]);
            assert!(report.plan_executions[0].steps[0]
                .reason
                .as_ref()
                .unwrap()
                .contains("contents"));
            assert_eq!(
                report.plan_executions[0].steps[1].step.action,
                PlanAction::Apply {
                    operation: Operation::Contents
                }
            );
            assert_eq!(
                report.plan_executions[0].steps[2].step.action,
                PlanAction::Verify {
                    condition: CompletionCondition::ContentsEmpty { path: path.clone() }
                }
            );
            assert!(path.is_dir());
            // 记账口径按平台：Windows 用逻辑长度，Unix 用分配块（小文件占一个
            // 4 KiB 块）。断言写成两平台各自的事实，而不是让 macOS 将就 Windows。
            #[cfg(windows)]
            assert_eq!(progress.bytes.load(std::sync::atomic::Ordering::Relaxed), 5);
            #[cfg(unix)]
            assert_eq!(
                progress.bytes.load(std::sync::atomic::Ordering::Relaxed),
                4096
            );
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn owner_runner_never_falls_back_after_attempt_and_accounts_only_confirmed() {
        let root = crate::core::testing::fixture("owner_flow_result");
        let path = root.join(".pnpm-store");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("keep"), b"store").unwrap();
        for (fail, evidence, last) in [
            (true, Evidence::Confirmed, StepStatus::Blocked),
            (false, Evidence::Unknown, StepStatus::Unknown),
            (false, Evidence::Absent, StepStatus::Failed),
            (false, Evidence::Confirmed, StepStatus::Succeeded),
        ] {
            let progress = CleanProgress::default();
            let mut executor = OwnerExecutor {
                progress: &progress,
                size_hint: Some(77),
                resources: owner_fixture(true, fail, evidence),
                fallback: false,
                error: None,
            };
            let report = execute_capability(
                &plan(&path, Operation::Pnpm),
                0,
                &progress,
                SpotCheck::Clear,
                &mut executor,
            );
            assert_eq!(statuses(&report)[2], last);
            assert_eq!(
                executor.resources.calls,
                if fail {
                    vec!["prepare", "apply"]
                } else {
                    vec!["prepare", "apply", "verify"]
                }
            );
            assert!(path.join("keep").is_file());
            let confirmed = !fail && evidence == Evidence::Confirmed;
            assert_eq!(
                progress.bytes.load(std::sync::atomic::Ordering::Relaxed),
                if confirmed { 77 } else { 0 }
            );
            assert_eq!(report.ok > 0, confirmed);
            if fail {
                assert_eq!(
                    report.plan_executions[0].steps[1].reason.as_deref(),
                    Some("fixture timeout after attempt")
                );
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn owner_runner_blocks_preflight_on_occupancy_identity_and_preserve() {
        let root = crate::core::testing::fixture("owner_flow_guards");
        let path = root.join("go/pkg/mod");
        std::fs::create_dir_all(&path).unwrap();
        for case in 0..4 {
            let mut plan = plan(&path, Operation::Go);
            let occupancy = match case {
                0 => SpotCheck::Busy,
                1 => SpotCheck::Unknown,
                _ => SpotCheck::Clear,
            };
            if case == 2 {
                plan.targets[0].identity = None;
            }
            if case == 3 {
                plan.preserve.push(path.join("keep"));
            }
            let progress = CleanProgress::default();
            let mut executor = OwnerExecutor {
                progress: &progress,
                size_hint: None,
                resources: owner_fixture(true, false, Evidence::Confirmed),
                fallback: false,
                error: None,
            };
            let report = execute_capability(&plan, 0, &progress, occupancy, &mut executor);
            assert_ne!(statuses(&report)[0], StepStatus::Succeeded);
            assert!(executor.resources.calls.is_empty());
            assert!(path.is_dir());
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn brew_records_throttle_and_estimate_only_after_confirmed_preview() {
        struct BrewFixture {
            evidence: Evidence,
            calls: Vec<&'static str>,
        }
        impl NativeResources for BrewFixture {
            fn mutate(&mut self, operation: &Operation) -> Result<bool, String> {
                assert_eq!(*operation, Operation::Brew);
                self.calls.push("cleanup");
                Ok(false)
            }
            fn completion(&mut self, condition: &CompletionCondition) -> Evidence {
                assert_eq!(*condition, CompletionCondition::BrewPreviewEmpty);
                self.calls.push("preview");
                self.evidence
            }
            fn completed(&mut self, condition: &CompletionCondition) {
                assert_eq!(*condition, CompletionCondition::BrewPreviewEmpty);
                self.calls.push("record");
            }
        }
        for evidence in [Evidence::Confirmed, Evidence::Absent, Evidence::Unknown] {
            let progress = CleanProgress::default();
            let mut executor = NativeExecutor {
                progress: &progress,
                size_hint: Some(100),
                layers_deleted: false,
                error: None,
                resources: BrewFixture {
                    evidence,
                    calls: vec![],
                },
            };
            let report = execute_capability(
                &plan(Path::new("brew://cleanup"), Operation::Brew),
                0,
                &progress,
                SpotCheck::Unknown,
                &mut executor,
            );
            if evidence == Evidence::Confirmed {
                assert_eq!(executor.resources.calls, ["cleanup", "preview", "record"]);
                assert_eq!(statuses(&report), [StepStatus::Succeeded; 3]);
                assert_eq!(progress.snapshot().bytes, 100);
                assert_eq!(report.ok, 1);
            } else {
                assert_eq!(executor.resources.calls, ["cleanup", "preview"]);
                assert_eq!(progress.snapshot().bytes, 0);
                assert_eq!(report.ok, 0);
                assert_eq!(
                    statuses(&report)[2],
                    if evidence == Evidence::Unknown {
                        StepStatus::Unknown
                    } else {
                        StepStatus::Failed
                    }
                );
            }
        }
    }

    #[test]
    fn native_executor_uses_typed_resources_and_reports_verification_and_accounting() {
        struct FixtureResources {
            mutation: Result<bool, String>,
            evidence: Evidence,
            applied: Vec<Operation>,
            verified: Vec<CompletionCondition>,
        }
        impl NativeResources for FixtureResources {
            fn mutate(&mut self, operation: &Operation) -> Result<bool, String> {
                self.applied.push(operation.clone());
                self.mutation.clone()
            }
            fn completion(&mut self, condition: &CompletionCondition) -> Evidence {
                self.verified.push(condition.clone());
                self.evidence
            }
        }
        for operation in [
            Operation::Docker {
                reference: "fixture/app:2".into(),
            },
            Operation::Snapshot {
                name: "com.apple.TimeMachine.2026-10-04-120000.local".into(),
            },
        ] {
            for (mutation, evidence, expected) in [
                (Ok(false), Evidence::Confirmed, StepStatus::Succeeded),
                (Ok(true), Evidence::Confirmed, StepStatus::Succeeded),
                (Ok(true), Evidence::Absent, StepStatus::Failed),
                (Ok(true), Evidence::Unknown, StepStatus::Unknown),
                (
                    Err("fixture mutation timeout".into()),
                    Evidence::Confirmed,
                    StepStatus::Blocked,
                ),
            ] {
                let path = if matches!(operation, Operation::Docker { .. }) {
                    "docker://image/display-alias"
                } else {
                    "tmutil://snapshot/display-alias"
                };
                let planned = plan(Path::new(path), operation.clone());
                let progress = CleanProgress::default();
                let mut executor = NativeExecutor {
                    progress: &progress,
                    size_hint: Some(512),
                    layers_deleted: false,
                    error: None,
                    resources: FixtureResources {
                        mutation: mutation.clone(),
                        evidence,
                        applied: vec![],
                        verified: vec![],
                    },
                };
                let report =
                    execute_capability(&planned, 0, &progress, SpotCheck::Unknown, &mut executor);
                assert_eq!(statuses(&report)[2], expected);
                assert_eq!(
                    executor.resources.applied.as_slice(),
                    std::slice::from_ref(&operation)
                );
                if mutation.is_err() {
                    assert!(executor.resources.verified.is_empty());
                    assert!(report.plan_executions[0].steps[1]
                        .reason
                        .as_ref()
                        .unwrap()
                        .contains("fixture mutation timeout"));
                } else {
                    assert_eq!(
                        executor.resources.verified,
                        [planned.targets[0].completion()]
                    );
                }
                let completed = expected == StepStatus::Succeeded;
                assert_eq!(progress.snapshot().files, u64::from(completed));
                assert_eq!(
                    progress.snapshot().bytes,
                    if completed
                        && mutation == Ok(true)
                        && matches!(operation, Operation::Docker { .. })
                    {
                        512
                    } else {
                        0
                    }
                );
                assert_eq!(report.ok, usize::from(completed));
                if !completed {
                    assert_eq!(report.failed.len(), 1);
                }
            }
        }
    }

    #[test]
    fn successful_action_without_artifact_removal_fails_completion() {
        let root = crate::core::testing::fixture("flow_zero_exit");
        std::fs::write(root.join("sentinel"), b"still here").unwrap();
        let progress = CleanProgress::default();
        let report = execute_filesystem_with(
            &plan(&root, Operation::Tree),
            0,
            &progress,
            SpotCheck::Clear,
            |path, _, _| {
                let mut report = CleanReport::default();
                report.record(path, CleanResult::Ok);
                report
            },
        );
        assert_eq!(
            statuses(&report),
            [
                StepStatus::Succeeded,
                StepStatus::Succeeded,
                StepStatus::Failed
            ]
        );
        assert_eq!(report.ok, 0);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(progress.snapshot().failed, 1);
        assert!(root.join("sentinel").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn real_file_tree_and_contents_follow_plan_dependencies_and_completion() {
        let root = crate::core::testing::fixture("flow_real_cleanup");
        for operation in [Operation::File, Operation::Tree, Operation::Contents] {
            let path = root.join(format!("{operation:?}"));
            if operation == Operation::File {
                std::fs::write(&path, b"file").unwrap();
            } else {
                std::fs::create_dir_all(&path).unwrap();
                std::fs::write(path.join("sentinel"), b"file").unwrap();
            }
            let plan = plan(&path, operation.clone());
            let report = execute_filesystem(&plan, 0, &CleanProgress::default(), SpotCheck::Clear);
            assert!(report.failed.is_empty(), "{operation:?}: {report:?}");
            assert_eq!(statuses(&report), [StepStatus::Succeeded; 3]);
            let steps = &report.plan_executions[0].steps;
            assert_eq!(steps[1].step.depends_on, [steps[0].step.id]);
            assert_eq!(steps[2].step.depends_on, [steps[1].step.id]);
            assert_eq!(
                filesystem_completion(&plan.targets[0].completion()),
                Evidence::Confirmed
            );
            if operation == Operation::Contents {
                assert!(path.is_dir());
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_cancelled_and_native_steps_never_run_dependents() {
        let root = crate::core::testing::fixture("flow_stop_dependencies");
        let planned = plan(&root, Operation::Tree);
        let report = execute_filesystem_with(
            &planned,
            0,
            &CleanProgress::default(),
            SpotCheck::Clear,
            |path, _, _| {
                let mut report = CleanReport::default();
                report.record(path, CleanResult::Failed);
                report
            },
        );
        assert_eq!(
            statuses(&report),
            [
                StepStatus::Succeeded,
                StepStatus::Failed,
                StepStatus::Blocked
            ]
        );
        let progress = CleanProgress::default();
        progress.request_cancel();
        let report =
            execute_filesystem_with(&planned, 0, &progress, SpotCheck::Clear, |_, _, _| {
                panic!("cancelled plan executed")
            });
        assert_eq!(
            statuses(&report),
            [
                StepStatus::Cancelled,
                StepStatus::Blocked,
                StepStatus::Blocked
            ]
        );
        let native = plan(
            &root,
            Operation::Docker {
                reference: "fixture/app:1".into(),
            },
        );
        let report = execute_filesystem_with(
            &native,
            0,
            &CleanProgress::default(),
            SpotCheck::Clear,
            |_, _, _| panic!("native resource fell back to deletion"),
        );
        assert_eq!(statuses(&report), [StepStatus::Blocked; 3]);
        assert!(root.is_dir());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn shared_busy_or_unknown_probe_blocks_apply_without_reprobing() {
        let root = crate::core::testing::fixture("flow_shared_occupancy");
        std::fs::write(root.join("sentinel"), b"protected").unwrap();
        for occupancy in [SpotCheck::Busy, SpotCheck::Unknown] {
            let report = execute_filesystem_with(
                &plan(&root, Operation::Tree),
                0,
                &CleanProgress::default(),
                occupancy,
                |_, _, _| panic!("unconfirmed occupancy granted deletion"),
            );
            assert_eq!(report.ok, 0);
            assert_eq!(statuses(&report)[1..], [StepStatus::Blocked; 2]);
            assert_eq!(
                statuses(&report)[0],
                if occupancy == SpotCheck::Unknown {
                    StepStatus::Unknown
                } else {
                    StepStatus::Blocked
                }
            );
            assert!(root.join("sentinel").is_file());
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unreadable_contents_and_nonfilesystem_conditions_are_unknown() {
        let root = crate::core::testing::fixture("flow_unknown_completion");
        let file = root.join("file");
        std::fs::write(&file, b"not a directory").unwrap();
        assert_eq!(
            filesystem_completion(&CompletionCondition::ContentsEmpty { path: file }),
            Evidence::Unknown
        );
        assert_eq!(
            filesystem_completion(&CompletionCondition::ContentsEmpty {
                path: root.join("missing")
            }),
            Evidence::Unknown
        );
        assert_eq!(
            filesystem_completion(&CompletionCondition::DockerReferenceAbsent {
                reference: "fixture/app:1".into()
            }),
            Evidence::Unknown
        );
        assert_eq!(
            filesystem_completion(&CompletionCondition::PathAbsent {
                path: root.join("missing")
            }),
            Evidence::Confirmed
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
