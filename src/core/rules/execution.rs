//! Fixed lifecycle dependencies cannot be weakened by a rule update.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceAction {
    Revalidate,
    OfficialOperation,
    SupplementCleanup,
    VerifyCompletion,
    RemoveRecoveryRecords,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceCompletion {
    ArtifactsAndRegistrationsAbsent,
}
pub const SOURCE_ACTIONS: [SourceAction; 5] = [
    SourceAction::Revalidate,
    SourceAction::OfficialOperation,
    SourceAction::SupplementCleanup,
    SourceAction::VerifyCompletion,
    SourceAction::RemoveRecoveryRecords,
];
pub fn validate(actions: &[SourceAction]) -> Result<(), String> {
    if actions != SOURCE_ACTIONS {
        return Err("Unsafe or incomplete installation lifecycle".into());
    }
    Ok(())
}
pub fn remove_snapshot(name: &str) -> bool {
    remove_snapshot_with(name, |args| {
        crate::core::proc::run_with_timeout("tmutil", args, std::time::Duration::from_secs(60))
    })
}
fn remove_snapshot_with(
    name: &str,
    mut run: impl FnMut(&[&str]) -> Option<crate::core::proc::ProcRun>,
) -> bool {
    remove_snapshot_action_with(name, &mut run)
        && snapshot_absence_with(name, run) == super::facts::Evidence::Confirmed
}

pub(crate) fn remove_snapshot_action(name: &str) -> bool {
    remove_snapshot_action_with(name, |args| {
        crate::core::proc::run_with_timeout("tmutil", args, std::time::Duration::from_secs(60))
    })
}

fn remove_snapshot_action_with(
    name: &str,
    mut run: impl FnMut(&[&str]) -> Option<crate::core::proc::ProcRun>,
) -> bool {
    let Some(date) = snapshot_date(name) else {
        return false;
    };
    run(&["deletelocalsnapshots", date]).is_some_and(|run| run.ok)
}

pub(crate) fn snapshot_absence(name: &str) -> super::facts::Evidence {
    snapshot_absence_with(name, |args| {
        crate::core::proc::run_with_timeout("tmutil", args, std::time::Duration::from_secs(60))
    })
}

fn snapshot_absence_with(
    name: &str,
    mut run: impl FnMut(&[&str]) -> Option<crate::core::proc::ProcRun>,
) -> super::facts::Evidence {
    use super::facts::Evidence;
    let Some(date) = snapshot_date(name) else {
        return Evidence::Unknown;
    };
    let Some(inventory) = run(&["listlocalsnapshots", "/"]) else {
        return Evidence::Unknown;
    };
    if !inventory.ok {
        return Evidence::Unknown;
    }
    let Ok(stdout) = std::str::from_utf8(&inventory.stdout) else {
        return Evidence::Unknown;
    };
    for line in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if line.starts_with("Snapshots for disk") {
            continue;
        }
        if !line.starts_with("com.apple.") || line.contains(char::is_whitespace) {
            return Evidence::Unknown;
        }
        if line == format!("com.apple.TimeMachine.{date}.local") {
            return Evidence::Absent;
        }
    }
    Evidence::Confirmed
}
pub(crate) fn snapshot_date(name: &str) -> Option<&str> {
    let date = name
        .strip_prefix("com.apple.TimeMachine.")
        .and_then(|name| name.strip_suffix(".local"))
        .unwrap_or(name);
    if date.len() != 17 || chrono::NaiveDateTime::parse_from_str(date, "%Y-%m-%d-%H%M%S").is_err() {
        None
    } else {
        Some(date)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_completion_probe_preserves_unknown_and_only_queries_inventory() {
        use super::super::facts::Evidence;
        let name = "com.apple.TimeMachine.2026-10-04-120000.local";
        for (text, expected) in [
            ("Snapshots for disk (/):\n", Evidence::Confirmed),
            (name, Evidence::Absent),
            ("broken inventory", Evidence::Unknown),
        ] {
            assert_eq!(
                snapshot_absence_with(name, |args| {
                    assert_eq!(args, ["listlocalsnapshots", "/"]);
                    Some(crate::core::proc::ProcRun {
                        stdout: text.as_bytes().to_vec(),
                        stderr: vec![],
                        exit_code: Some(0),
                        ok: true,
                    })
                }),
                expected
            );
        }
        assert_eq!(snapshot_absence_with(name, |_| None), Evidence::Unknown);
        assert_eq!(
            snapshot_absence_with("--delete-all", |_| panic!("invalid snapshot queried")),
            Evidence::Unknown
        );
    }

    #[test]
    fn snapshot_uses_date_argument_and_checks_native_inventory() {
        let response = |text: &str| crate::core::proc::ProcRun {
            stdout: text.as_bytes().to_vec(),
            stderr: vec![],
            exit_code: Some(0),
            ok: true,
        };
        let name = "com.apple.TimeMachine.2026-10-04-120000.local";
        assert!(remove_snapshot_with(name, |args| {
            if args[0] == "deletelocalsnapshots" {
                assert_eq!(args[1], "2026-10-04-120000");
            }
            Some(response("Snapshots for disk (/):\n"))
        }));
        assert!(!remove_snapshot_with(name, |_| Some(response(&format!(
            "Snapshots for disk (/):\n{name}\n"
        )))));
        assert!(!remove_snapshot_with(name, |args| {
            if args[0] == "listlocalsnapshots" {
                None
            } else {
                Some(response(""))
            }
        }));
        assert!(!remove_snapshot_with("--delete-all", |_| panic!(
            "invalid name must not execute"
        )));
    }
    #[test]
    fn lifecycle_keeps_retry_records_after_any_dependency_failure() {
        use crate::core::cleaner::{CleanProgress, Disposal};
        use crate::core::rules::{
            CleanupPlan, InstallationInstance, Operation, PlannedTarget, RuleRef,
        };
        let root = crate::core::testing::fixture("source_retry_dependencies");
        let code = root.join("hermes-agent");
        std::fs::create_dir(&code).unwrap();
        let record = root.join("retry");
        std::fs::write(&record, b"retry evidence").unwrap();
        let mut plan = CleanupPlan::new(
            RuleRef::new("hermes", Some(root.clone())),
            vec![PlannedTarget {
                path: code.clone(),
                operation: Operation::OfficialUninstall,
                identity: crate::core::model::capture_identity(&code),
                disposal: Disposal::Permanent,
            }],
        );
        plan.installation =
            Some(InstallationInstance::capture(code, std::slice::from_ref(&record)).unwrap());
        for failed in &SOURCE_ACTIONS[..4] {
            let mut visited = Vec::new();
            let report = crate::core::rules::flow::execute_source(
                &plan,
                0,
                &CleanProgress::default(),
                |action| {
                    visited.push(action);
                    if action == *failed {
                        Err("fixture failure".into())
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(crate::core::rules::flow::execution_result(&report).is_err());
            assert!(!visited.contains(&SourceAction::RemoveRecoveryRecords));
            assert!(record.is_file());
        }
        let mut reordered = SOURCE_ACTIONS;
        reordered.swap(3, 4);
        assert!(validate(&reordered).is_err());
        let mut bundle = crate::core::rules::current().bundle.clone();
        bundle
            .rules
            .iter_mut()
            .find(|rule| rule.id == "hermes")
            .unwrap()
            .app
            .as_mut()
            .unwrap()
            .actions = reordered.to_vec();
        assert!(
            bundle.validate().is_err(),
            "unsafe lifecycle cannot be loaded from a rule package"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
