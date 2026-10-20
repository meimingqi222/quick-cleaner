//! Explicit consent to close cache owners never grants broader deletion authority.
use crate::core::{
    categories::CategoryId,
    cleaner::{CleanProgress, CleanReport, CleanTarget, Disposal},
    rules::Operation,
    scanner::ScanItem,
};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Occupant {
    pub pid: u32,
    /// Darwin uniqueid or the exact Windows creation FILETIME, never a PID alone.
    pub identity: u64,
    pub executable: PathBuf,
    pub name: String,
    pub evidence: PathBuf,
}

#[derive(Clone, Debug)]
pub struct ReleasePlan {
    pub item: ScanItem,
    pub occupants: Vec<Occupant>,
}

pub enum ReleaseOutcome {
    Cleaned(CleanReport),
    StillBusy,
}

pub fn eligible(item: &ScanItem) -> bool {
    matches!(
        item.category,
        CategoryId::UserCache | CategoryId::BrowserCache | CategoryId::Logs
    ) && (item.recommended
        || item
            .busy
            .as_ref()
            .is_some_and(|busy| busy.recommended_before_busy))
        && !item.busy.as_ref().is_some_and(|busy| busy.unknown)
        && item.operation == Operation::Contents
        && item.disposal == Disposal::Permanent
        && item.identity.is_some()
        && !item.plans.is_empty()
        && item.rule.blocked.is_none()
}

fn validate(item: &ScanItem) -> Result<(), String> {
    if !eligible(item) || crate::core::safety::is_contents_protected(&item.path) {
        return Err("cache-release-not-authorized".into());
    }
    if !item
        .identity
        .is_some_and(|identity| identity.recheck(&item.path))
    {
        return Err("cache-target-changed; rescan-required".into());
    }
    item.rule.revalidate()?;
    for plan in &item.plans {
        plan.validate_binding(&item.path, &item.operation, item.disposal, item.identity)?;
    }
    Ok(())
}

pub fn prepare(item: ScanItem) -> Result<ReleasePlan, String> {
    validate(&item)?;
    let occupants = crate::platform::cache_release::occupants(&item.path)?;
    if occupants.is_empty() || occupants.len() > 16 {
        return Err("no-confirmed-cache-owners; retry-or-rescan".into());
    }
    Ok(ReleasePlan { item, occupants })
}

fn consent_covers(consented: &[Occupant], current: &[Occupant]) -> bool {
    current.iter().all(|owner| {
        consented.iter().any(|approved| {
            approved.pid == owner.pid
                && approved.identity == owner.identity
                && approved.executable == owner.executable
        })
    })
}

pub fn execute(
    plan: &ReleasePlan,
    force: bool,
    progress: &CleanProgress,
) -> Result<ReleaseOutcome, String> {
    execute_with(
        plan,
        force,
        progress,
        crate::platform::cache_release::occupants,
        crate::platform::cache_release::request_exit,
        Duration::from_secs(8),
        |target, p| crate::core::cleaner::clean_targets(&[target], p),
    )
}

fn execute_with(
    plan: &ReleasePlan,
    force: bool,
    progress: &CleanProgress,
    mut probe: impl FnMut(&std::path::Path) -> Result<Vec<Occupant>, String>,
    mut exit: impl FnMut(&Occupant, bool) -> Result<(), String>,
    wait: Duration,
    clean: impl FnOnce(CleanTarget, &CleanProgress) -> CleanReport,
) -> Result<ReleaseOutcome, String> {
    validate(&plan.item)?;
    let current = probe(&plan.item.path)?;
    if !consent_covers(&plan.occupants, &current) {
        return Err("cache-owners-changed; review-required".into());
    }
    for owner in &current {
        if progress.cancelled() {
            return Err("cancelled".into());
        }
        validate(&plan.item)?;
        exit(owner, force)?;
    }
    let deadline = Instant::now() + wait;
    loop {
        if progress.cancelled() {
            return Err("cancelled".into());
        }
        let current = probe(&plan.item.path)?;
        if current.is_empty() {
            break;
        }
        if !consent_covers(&plan.occupants, &current) {
            return Err("cache-owners-changed; review-required".into());
        }
        if Instant::now() >= deadline {
            return Ok(ReleaseOutcome::StillBusy);
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    validate(&plan.item)?;
    let item = &plan.item;
    let target = CleanTarget {
        path: item.path.clone(),
        plans: item.plans.clone(),
        rule: Some(item.rule.clone()),
        operation: item.operation.clone(),
        remove_dir: false,
        size_hint: None,
        identity: item.identity,
        disposal: item.disposal,
    };
    // The ordinary cleaner performs a fresh occupancy probe and retains all database/link guards.
    Ok(ReleaseOutcome::Cleaned(clean(target, progress)))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn owner(pid: u32, identity: u64) -> Occupant {
        Occupant {
            pid,
            identity,
            executable: "/Applications/Example.app/Contents/MacOS/Example".into(),
            name: "Example".into(),
            evidence: "/cache/file".into(),
        }
    }
    #[test]
    fn consent_never_covers_reused_pids_or_new_owners() {
        let approved = vec![owner(40, 1)];
        assert!(consent_covers(&approved, &approved));
        assert!(consent_covers(&approved, &[]));
        assert!(!consent_covers(&approved, &[owner(40, 2)]));
        assert!(!consent_covers(&approved, &[owner(41, 1)]));
        let mut changed = owner(40, 1);
        changed.executable = "/other".into();
        assert!(!consent_covers(&approved, &[changed]));
    }
    fn fixture(name: &str) -> ReleasePlan {
        let path = crate::core::testing::fixture(name);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("cached"), b"keep until safe").unwrap();
        let identity = crate::core::model::capture_identity(&path);
        let rule = crate::core::rules::RuleRef::engine();
        let plan = crate::core::rules::CleanupPlan::new(
            rule.clone(),
            vec![crate::core::rules::PlannedTarget {
                path: path.clone(),
                operation: Operation::Contents,
                identity,
                disposal: Disposal::Permanent,
            }],
        );
        ReleasePlan {
            item: ScanItem {
                path,
                plans: vec![std::sync::Arc::new(plan)],
                operation: Operation::Contents,
                rule,
                disposal: Disposal::Permanent,
                label: crate::core::i18n::Text::same("cache"),
                size: 15,
                file_count: 1,
                category: CategoryId::UserCache,
                last_modified: 0,
                recommended: true,
                busy: None,
                identity,
            },
            occupants: vec![owner(40, 1)],
        }
    }

    #[test]
    fn cache_release_waits_for_vacancy_and_preserves_the_scanned_plan() {
        let plan = fixture("qc_release_clean");
        let calls = std::cell::Cell::new(0);
        let exits = std::cell::Cell::new(0);
        let result = execute_with(
            &plan,
            false,
            &CleanProgress::default(),
            |_| {
                let count = calls.get();
                calls.set(count + 1);
                Ok(if count == 0 {
                    plan.occupants.clone()
                } else {
                    vec![]
                })
            },
            |_, force| {
                assert!(!force);
                exits.set(exits.get() + 1);
                Ok(())
            },
            Duration::ZERO,
            |target, _| {
                assert_eq!(target.path, plan.item.path);
                assert_eq!(target.identity, plan.item.identity);
                assert!(std::sync::Arc::ptr_eq(
                    &target.plans[0],
                    &plan.item.plans[0]
                ));
                assert!(!target.remove_dir);
                CleanReport::default()
            },
        )
        .unwrap();
        assert!(matches!(result, ReleaseOutcome::Cleaned(_)));
        assert_eq!(exits.get(), 1);
        std::fs::remove_dir_all(plan.item.path).unwrap();
    }

    #[test]
    fn timeout_unknown_new_owners_and_cancel_never_delete_or_auto_force() {
        let plan = fixture("qc_release_refusals");
        let progress = CleanProgress::default();
        let result = execute_with(
            &plan,
            false,
            &progress,
            |_| Ok(plan.occupants.clone()),
            |_, force| {
                assert!(!force);
                Ok(())
            },
            Duration::ZERO,
            |_, _| panic!("a sent exit request is not vacancy"),
        )
        .unwrap();
        assert!(matches!(result, ReleaseOutcome::StillBusy));
        assert!(execute_with(
            &plan,
            true,
            &progress,
            |_| Ok(vec![owner(40, 2)]),
            |_, _| panic!("reused PID"),
            Duration::ZERO,
            |_, _| panic!("reused PID")
        )
        .is_err());
        assert!(execute_with(
            &plan,
            false,
            &progress,
            |_| Err("probe-incomplete".into()),
            |_, _| panic!("unknown"),
            Duration::ZERO,
            |_, _| panic!("unknown")
        )
        .is_err());
        progress.request_cancel();
        assert!(execute_with(
            &plan,
            true,
            &progress,
            |_| Ok(plan.occupants.clone()),
            |_, _| panic!("cancelled"),
            Duration::ZERO,
            |_, _| panic!("cancelled")
        )
        .is_err());
        assert!(plan.item.path.join("cached").is_file());
        std::fs::remove_dir_all(plan.item.path).unwrap();
    }

    #[test]
    fn only_rule_recommended_cache_contents_can_close_owners() {
        let mut plan = fixture("qc_release_scope");
        assert!(eligible(&plan.item));
        plan.item.recommended = false;
        assert!(!eligible(&plan.item));
        plan.item.busy = Some(crate::core::inuse::Busy {
            recommended_before_busy: true,
            open: true,
            ..Default::default()
        });
        assert!(eligible(&plan.item));
        plan.item.busy.as_mut().unwrap().unknown = true;
        assert!(!eligible(&plan.item));
        plan.item.busy = None;
        plan.item.recommended = true;
        for category in [
            CategoryId::AiAgents,
            CategoryId::DevBuild,
            CategoryId::PackageCache,
            CategoryId::DevWorktrees,
            CategoryId::OldIdeData,
        ] {
            plan.item.category = category;
            assert!(!eligible(&plan.item));
        }
        plan.item.category = CategoryId::UserCache;
        plan.item.operation = Operation::Tree;
        assert!(!eligible(&plan.item));
        std::fs::remove_dir_all(plan.item.path).unwrap();
    }

    #[test]
    fn changed_cache_root_is_refused_before_any_exit() {
        let plan = fixture("qc_release_identity");
        let moved = plan.item.path.with_extension("original");
        std::fs::rename(&plan.item.path, &moved).unwrap();
        std::fs::create_dir_all(&plan.item.path).unwrap();
        assert!(execute_with(
            &plan,
            true,
            &CleanProgress::default(),
            |_| panic!("changed root"),
            |_, _| panic!("changed root"),
            Duration::ZERO,
            |_, _| panic!("changed root")
        )
        .is_err());
        std::fs::remove_dir_all(plan.item.path).unwrap();
        std::fs::remove_dir_all(moved).unwrap();
    }
}
