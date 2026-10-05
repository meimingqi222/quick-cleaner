//! Linked worktree discovery reads metadata only; Git commands run at cleanup time.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registration {
    pub admin: PathBuf,
    common: PathBuf,
    target: PathBuf,
    backlink: String,
}

fn small_text(path: &Path) -> io::Result<String> {
    let md = std::fs::symlink_metadata(path)?;
    if !md.is_file() || md.file_type().is_symlink() || md.len() > 8192 {
        return Err(io::Error::other("Invalid worktree metadata"));
    }
    let mut text = String::new();
    std::fs::File::open(path)?
        .take(8193)
        .read_to_string(&mut text)?;
    if text.len() > 8192 {
        return Err(io::Error::other("Worktree metadata too large"));
    }
    Ok(text.trim_end_matches(['\r', '\n']).to_owned())
}

fn resolve(base: &Path, value: &str) -> io::Result<PathBuf> {
    if value.is_empty() || value.contains(['\r', '\n', '\0']) {
        return Err(io::Error::other("Invalid worktree path"));
    }
    canonical(&base.join(value))
}

fn canonical(path: &Path) -> io::Result<PathBuf> {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        if !matches!(path.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
        {
            return Err(io::Error::other("Worktree must be on a local drive"));
        }
        let canonical = std::fs::canonicalize(path)?;
        let text = canonical
            .to_str()
            .ok_or_else(|| io::Error::other("Invalid worktree path encoding"))?;
        // Git and safety's path keys require ordinary drive paths, not Win32 verbatim prefixes.
        Ok(PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(text)))
    }
    #[cfg(not(windows))]
    std::fs::canonicalize(path)
}

pub fn inspect(target: &Path) -> io::Result<Registration> {
    let target = canonical(target)?;
    let marker = target.join(".git");
    let text = small_text(&marker)?;
    let value = text
        .strip_prefix("gitdir: ")
        .ok_or_else(|| io::Error::other("Not a linked worktree"))?;
    let admin = resolve(&target, value)?;
    let common = resolve(&admin, &small_text(&admin.join("commondir"))?)?;
    // Both links must agree, and the admin entry must be an immediate worktrees child.
    let backlink = small_text(&admin.join("gitdir"))?;
    if admin.parent() != Some(common.join("worktrees").as_path())
        || resolve(&admin, &backlink)? != marker
        || !common.join("HEAD").is_file()
        || !admin.join("HEAD").is_file()
    {
        return Err(io::Error::other("Worktree registration mismatch"));
    }
    Ok(Registration {
        admin,
        common,
        target,
        backlink,
    })
}

/// Agent containers have at most two wrapper directories (Codex id/project, Maka workspace).
pub fn discover(container: &Path) -> Vec<PathBuf> {
    fn visit(path: &Path, depth: usize, budget: &mut usize, found: &mut Vec<PathBuf>) {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let Ok(md) = std::fs::symlink_metadata(path) else {
            return;
        };
        if !md.is_dir() || md.file_type().is_symlink() {
            return;
        }
        match std::fs::symlink_metadata(path.join(".git")) {
            Ok(_) => {
                if inspect(path).is_ok() {
                    found.push(path.to_owned());
                }
                return;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return,
        }
        if depth == 0 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            if *budget == 0 {
                break;
            }
            *budget -= 1;
            if entry
                .file_type()
                .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
            {
                visit(&entry.path(), depth - 1, budget, found);
            }
        }
    }
    let mut found = Vec::new();
    visit(container, 3, &mut 4096, &mut found);
    found
}

fn git(
    common: &Path,
    args: &[&std::ffi::OsStr],
    timeout: Duration,
) -> Option<crate::core::proc::ProcRun> {
    let mut argv = vec![
        std::ffi::OsStr::new("--no-optional-locks"),
        std::ffi::OsStr::new("-c"),
        std::ffi::OsStr::new("core.fsmonitor=false"),
        std::ffi::OsStr::new("--git-dir"),
        common.as_os_str(),
    ];
    argv.extend_from_slice(args);
    crate::core::proc::run_with_timeout("git", &argv, timeout)
}

fn metadata_removable(path: &Path, budget: &mut usize) -> bool {
    if *budget == 0 || crate::core::safety::is_protected(path) {
        return false;
    }
    *budget -= 1;
    let Ok(md) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if md.file_type().is_symlink() {
        return false;
    }
    if md.is_file() {
        return path.extension().is_none_or(|ext| ext != "lock");
    }
    std::fs::read_dir(path).is_ok_and(|entries| {
        entries
            .into_iter()
            .all(|entry| entry.is_ok_and(|entry| metadata_removable(&entry.path(), budget)))
    })
}

impl Registration {
    pub fn check_ready(&self) -> Result<(), crate::core::cleaner::FailReason> {
        use crate::core::cleaner::FailReason;
        if crate::core::safety::is_managed_agent_worktree(&self.target) {
            return Err(FailReason::WorktreeManaged);
        }
        match std::fs::symlink_metadata(self.admin.join("locked")) {
            Ok(_) => return Err(FailReason::WorktreeLocked),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err(FailReason::WorktreeUnverified),
        }
        if crate::core::safety::is_protected(&self.target)
            || !metadata_removable(&self.admin, &mut 1024)
            || inspect(&self.target).as_ref().ok() != Some(self)
            || !matches!(std::fs::symlink_metadata(self.admin.join("locked")), Err(e) if e.kind() == io::ErrorKind::NotFound)
        {
            return Err(FailReason::WorktreeUnverified);
        }
        // Dirty (including ignored) files stay intact. Git availability is checked before deletion.
        let args = [
            std::ffi::OsStr::new("--work-tree"),
            self.target.as_os_str(),
            std::ffi::OsStr::new("status"),
            std::ffi::OsStr::new("--porcelain"),
            std::ffi::OsStr::new("--untracked-files=all"),
            std::ffi::OsStr::new("--ignored"),
            std::ffi::OsStr::new("--ignore-submodules=none"),
        ];
        let status = git(&self.admin, &args, Duration::from_secs(10))
            .filter(|run| run.ok)
            .ok_or(FailReason::WorktreeUnverified)?;
        if !status.stdout.is_empty() {
            return Err(FailReason::WorktreeDirty);
        }
        let no_submodules = git(
            &self.admin,
            &[
                std::ffi::OsStr::new("ls-files"),
                std::ffi::OsStr::new("--stage"),
                std::ffi::OsStr::new("-z"),
            ],
            Duration::from_secs(10),
        )
        .is_some_and(|run| {
            run.ok
                && !run
                    .stdout
                    .split(|b| *b == 0)
                    .any(|entry| entry.starts_with(b"160000 "))
        });
        if no_submodules {
            Ok(())
        } else {
            Err(FailReason::WorktreeUnverified)
        }
    }

    /// Only remove the selected missing checkout's registration, never repository-wide prune.
    pub fn unregister(&self) -> bool {
        self.unregister_action().is_ok()
            && self.completion() == crate::core::rules::facts::Evidence::Confirmed
    }

    pub(crate) fn unregister_action(&self) -> Result<(), String> {
        if !matches!(std::fs::symlink_metadata(&self.target), Err(e) if e.kind() == io::ErrorKind::NotFound)
            || !metadata_removable(&self.admin, &mut 1024)
            || small_text(&self.admin.join("gitdir")).ok().as_ref() != Some(&self.backlink)
            || small_text(&self.admin.join("commondir"))
                .ok()
                .and_then(|p| resolve(&self.admin, &p).ok())
                .as_ref()
                != Some(&self.common)
        {
            return Err("Worktree checkout or exact registration could not be revalidated".into());
        }
        let args = [
            std::ffi::OsStr::new("worktree"),
            std::ffi::OsStr::new("remove"),
            std::ffi::OsStr::new("--"),
            self.target.as_os_str(),
        ];
        let result = git(&self.common, &args, Duration::from_secs(30))
            .ok_or("Exact worktree unregister unavailable or timed out")?;
        if result.ok {
            Ok(())
        } else {
            Err(format!(
                "Exact worktree unregister failed: {}",
                String::from_utf8_lossy(&result.stderr).trim()
            ))
        }
    }

    pub(crate) fn completion(&self) -> crate::core::rules::facts::Evidence {
        use crate::core::rules::facts::Evidence;
        let checkout = crate::core::rules::flow::filesystem_completion(
            &crate::core::rules::CompletionCondition::PathAbsent {
                path: self.target.clone(),
            },
        );
        let registration = crate::core::rules::flow::filesystem_completion(
            &crate::core::rules::CompletionCondition::PathAbsent {
                path: self.admin.clone(),
            },
        );
        match (checkout, registration) {
            (Evidence::Confirmed, Evidence::Confirmed) => Evidence::Confirmed,
            (Evidence::Unknown, _) | (_, Evidence::Unknown) => Evidence::Unknown,
            _ => Evidence::Absent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cleaner::{clean_targets, CleanProgress, CleanTarget};
    use crate::core::model::capture_identity;

    fn command(root: &Path, args: &[&str]) -> crate::core::proc::ProcRun {
        let root = root.to_str().unwrap();
        let mut argv = vec!["-C", root];
        argv.extend_from_slice(args);
        let run = crate::core::proc::run_with_timeout("git", &argv, Duration::from_secs(15))
            .expect("Git must be installed for worktree tests");
        assert!(run.ok, "{}", String::from_utf8_lossy(&run.stderr));
        run
    }

    fn repository(tag: &str) -> (PathBuf, PathBuf) {
        let root = crate::core::testing::fixture(tag);
        let main = root.join("main repo");
        std::fs::create_dir_all(&main).unwrap();
        command(&main, &["init"]);
        std::fs::write(main.join("tracked.txt"), b"committed data").unwrap();
        command(&main, &["add", "."]);
        command(
            &main,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "fixture",
            ],
        );
        (root, main)
    }

    fn add(main: &Path, path: &Path) {
        command(
            main,
            &[
                "worktree",
                "add",
                "--detach",
                path.to_str().unwrap(),
                "HEAD",
            ],
        );
    }

    fn clean(path: &Path) -> crate::core::cleaner::CleanReport {
        let mut target = CleanTarget::remove(path.to_owned());
        target.identity = capture_identity(path);
        target.rule = Some(crate::core::rules::RuleRef::engine());
        clean_targets(&[target], &CleanProgress::default())
    }

    #[test]
    fn managed_clean_checkout_cannot_be_deleted_without_reference_retirement() {
        let (root, main) = repository("worktrees_managed_reference");
        // Ownership follows workspace evidence, independent of the project/app install path.
        let workspace = root.join("relocated owner/custom workspace");
        let tree = workspace.join("subagent-worktrees/child");
        add(&main, &tree);
        let registration = inspect(&tree).unwrap();
        assert!(registration.check_ready().is_ok());
        std::fs::write(workspace.join("runtime.sqlite"), b"owner metadata").unwrap();
        use crate::core::cleaner::FailReason;
        assert_eq!(registration.check_ready(), Err(FailReason::WorktreeManaged));
        let report = clean(&tree);
        assert!(!report.failed.is_empty());
        assert!(report
            .fail_info
            .iter()
            .any(|info| info.reason == FailReason::WorktreeManaged));
        assert!(tree.join("tracked.txt").is_file());
        assert!(registration.admin.is_dir());
        // Removing .git or selecting a single file cannot bypass owner policy.
        std::fs::remove_file(tree.join(".git")).unwrap();
        assert!(!clean(&tree.join("tracked.txt")).failed.is_empty());
        assert!(!clean(&tree).failed.is_empty());
        assert!(tree.join("tracked.txt").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cleanup_removes_exact_registration_and_preserves_other_stale_entries() {
        let (root, main) = repository("worktrees_exact");
        let selected = root.join("selected tree");
        let other = root.join("other");
        let stale = root.join("stale");
        add(&main, &selected);
        add(&main, &other);
        add(&main, &stale);
        let selected_entry = inspect(&selected).unwrap();
        assert!(
            selected_entry.check_ready().is_ok(),
            "clean checkout must pass preflight: {selected_entry:?}"
        );
        let stale_entry = inspect(&stale).unwrap();
        std::fs::remove_dir_all(&stale).unwrap();
        let report = clean(&selected);
        let execution = &report.plan_executions[0];
        assert_eq!(execution.steps.len(), 5);
        assert!(execution
            .steps
            .iter()
            .all(|step| step.status == crate::core::rules::flow::StepStatus::Succeeded));
        assert!(matches!(
            execution.steps[2].step.action,
            crate::core::rules::PlanAction::Verify {
                condition: crate::core::rules::CompletionCondition::PathAbsent { .. }
            }
        ));
        assert!(matches!(
            execution.steps[3].step.action,
            crate::core::rules::PlanAction::UnregisterWorktree { .. }
        ));
        assert!(
            report.failed.is_empty(),
            "{:?}, {:?}, target remains {}",
            report.failed,
            report.fail_info,
            selected.exists()
        );
        assert!(!selected.exists());
        assert!(
            !selected_entry.admin.exists(),
            "selected Git registration must be gone"
        );
        assert!(
            stale_entry.admin.is_dir(),
            "never prune unrelated stale registrations"
        );
        assert!(other.join("tracked.txt").is_file());
        assert!(main.join("tracked.txt").is_file());
        let list = command(&main, &["worktree", "list", "--porcelain"]);
        let list = String::from_utf8_lossy(&list.stdout);
        assert!(!list.contains("selected tree"));
        assert!(list.contains("other"));
        assert!(list.contains("stale"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dirty_locked_and_changed_registration_never_fall_back_to_deletion() {
        let (root, main) = repository("worktrees_guard");
        for mode in [
            "dirty",
            "untracked",
            "ignored",
            "locked",
            "index-lock",
            "forged",
            "recycle",
        ] {
            let path = root.join(mode);
            add(&main, &path);
            let registration = inspect(&path).unwrap();
            match mode {
                "dirty" => std::fs::write(path.join("tracked.txt"), b"keep edits").unwrap(),
                "untracked" => std::fs::write(path.join("new.txt"), b"keep file").unwrap(),
                "ignored" => {
                    std::fs::write(registration.common.join("info/exclude"), b"ignored.txt\n")
                        .unwrap();
                    std::fs::write(path.join("ignored.txt"), b"keep ignored data").unwrap();
                }
                "locked" => {
                    command(&main, &["worktree", "lock", path.to_str().unwrap()]);
                }
                "index-lock" => {
                    std::fs::write(registration.admin.join("index.lock"), b"busy").unwrap()
                }
                "recycle" => {}
                "forged" => std::fs::write(
                    registration.admin.join("gitdir"),
                    main.join(".git").to_str().unwrap(),
                )
                .unwrap(),
                _ => unreachable!(),
            }
            let report = if mode == "recycle" {
                let mut target = CleanTarget::remove(path.clone());
                target.identity = capture_identity(&path);
                target.disposal = crate::core::cleaner::Disposal::RecycleBin;
                clean_targets(&[target], &CleanProgress::default())
            } else {
                clean(&path)
            };
            assert!(!report.failed.is_empty(), "{mode} must block cleanup");
            let expected_reason = match mode {
                "dirty" | "untracked" | "ignored" => {
                    Some(crate::core::cleaner::FailReason::WorktreeDirty)
                }
                "locked" => Some(crate::core::cleaner::FailReason::WorktreeLocked),
                "index-lock" | "forged" => {
                    Some(crate::core::cleaner::FailReason::WorktreeUnverified)
                }
                "recycle" => {
                    assert!(registration.check_ready().is_ok());
                    None
                }
                _ => unreachable!(),
            };
            if let Some(reason) = expected_reason {
                assert_eq!(registration.check_ready(), Err(reason), "{mode}");
            }
            assert!(
                path.join("tracked.txt").is_file(),
                "{mode} must retain data"
            );
            assert!(registration.admin.is_dir());
        }
        let path = root.join("changed");
        add(&main, &path);
        let mut target = CleanTarget::remove(path.clone());
        target.identity = capture_identity(&path);
        std::fs::remove_file(path.join(".git")).unwrap();
        target.identity = capture_identity(&path);
        assert!(!clean_targets(&[target], &CleanProgress::default())
            .failed
            .is_empty());
        assert!(path.join("tracked.txt").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discovery_only_lists_linked_checkouts_and_stops_at_repository_roots() {
        let (root, main) = repository("worktrees_discovery");
        let container = root.join("Maka/workspaces");
        let tree = container.join("default/subagent-worktrees/id");
        add(&main, &tree);
        let codex = root.join("codex");
        let nested = codex.join("hash/project");
        add(&main, &nested);
        std::fs::create_dir_all(container.join("default/other-data")).unwrap();
        assert_eq!(discover(&container), vec![tree.clone()]);
        assert_eq!(discover(&codex), vec![nested]);
        assert!(discover(&main).is_empty());
        std::fs::write(tree.join(".git"), b"gitdir: broken\n").unwrap();
        assert!(discover(&container).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unregister_requires_missing_checkout_and_unchanged_backlink() {
        let (root, main) = repository("worktrees_unregister_guard");
        let tree = root.join("tree");
        add(&main, &tree);
        let entry = inspect(&tree).unwrap();
        assert!(!entry.unregister());
        assert!(tree.join("tracked.txt").is_file());
        std::fs::remove_dir_all(&tree).unwrap();
        std::fs::write(entry.admin.join("gitdir"), b"changed registration").unwrap();
        assert!(!entry.unregister());
        assert!(entry.admin.is_dir());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn worktree_runner_retains_registration_after_failed_or_unverified_checkout_cleanup() {
        use crate::core::cleaner::{CleanReport, CleanResult, Disposal};
        use crate::core::rules::flow::StepStatus;
        use crate::core::rules::{CleanupPlan, Operation, PlannedTarget, RuleRef};
        let (root, main) = repository("worktree_plan_dependencies");
        for mode in [
            "failed",
            "no-op",
            "backlink-changed",
            "cancelled",
            "unknown-occupancy",
        ] {
            let path = root.join(mode);
            add(&main, &path);
            let entry = inspect(&path).unwrap();
            let plan = CleanupPlan::new(
                RuleRef::engine(),
                vec![PlannedTarget {
                    path: path.clone(),
                    operation: Operation::GitWorktree {
                        registration: entry.admin.clone(),
                    },
                    identity: capture_identity(&path),
                    disposal: Disposal::Permanent,
                }],
            );
            let progress = CleanProgress::default();
            let report = crate::core::rules::flow::execute_worktree_with(
                &plan,
                0,
                &progress,
                if mode == "unknown-occupancy" {
                    crate::core::inuse::SpotCheck::Unknown
                } else {
                    crate::core::inuse::SpotCheck::Clear
                },
                |target| {
                    assert_ne!(
                        mode, "unknown-occupancy",
                        "unknown occupancy must block apply"
                    );
                    let mut report = CleanReport::default();
                    let result = if mode == "backlink-changed" {
                        let result =
                            crate::core::cleaner::dispose(&target.path, target.disposal, &progress);
                        std::fs::write(entry.admin.join("gitdir"), b"changed after cleanup")
                            .unwrap();
                        result
                    } else if mode == "cancelled" {
                        let result =
                            crate::core::cleaner::dispose(&target.path, target.disposal, &progress);
                        progress.request_cancel();
                        result
                    } else if mode == "failed" {
                        CleanResult::Failed
                    } else {
                        CleanResult::Ok
                    };
                    report.record(&path, result);
                    report
                },
            );
            assert!(
                !report.failed.is_empty() || !report.skipped_items.is_empty(),
                "{mode}"
            );
            let steps = &report.plan_executions[0].steps;
            assert_eq!(steps.len(), 5);
            assert_eq!(steps[4].status, StepStatus::Blocked);
            match mode {
                "failed" => {
                    assert_eq!(steps[1].status, StepStatus::Failed);
                    assert_eq!(steps[2].status, StepStatus::Blocked);
                    assert_eq!(steps[3].status, StepStatus::Blocked);
                }
                "no-op" => {
                    assert_eq!(steps[2].status, StepStatus::Failed);
                    assert_eq!(steps[3].status, StepStatus::Blocked);
                }
                "cancelled" => {
                    assert_eq!(steps[2].status, StepStatus::Cancelled);
                    assert_eq!(steps[3].status, StepStatus::Blocked);
                }
                "unknown-occupancy" => {
                    assert_eq!(steps[0].status, StepStatus::Unknown);
                    assert!(steps[1..]
                        .iter()
                        .all(|step| step.status == StepStatus::Blocked));
                }
                _ => {
                    assert_eq!(steps[2].status, StepStatus::Succeeded);
                    assert_eq!(steps[3].status, StepStatus::Failed);
                    assert!(steps[3].reason.as_ref().unwrap().contains("registration"));
                }
            }
            assert!(
                entry.admin.is_dir(),
                "{mode} retains exact retry registration"
            );
            if !matches!(mode, "backlink-changed" | "cancelled") {
                assert!(path.join("tracked.txt").is_file());
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
