//! Installation records grant narrow program ownership; user data is never a cleanup target.

use super::app_discovery::{canonical_local, canonical_target};
use crate::core::apps::OfficialUninstaller;
use crate::core::cleaner::{clean_arbitrary_items, ArbitraryTarget, CleanProgress, Disposal};
use crate::core::safety::{at_or_under, is_protected_residual_path, norm};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

struct ExecutionContext<'a> {
    scanned: &'a crate::core::rules::CleanupPlan,
    executions: &'a mut Vec<crate::core::rules::flow::PlanExecution>,
    settle: Option<(&'a OfficialUninstaller, std::time::Duration)>,
}

pub(super) struct SourceInstallPlan {
    pub rule: crate::core::rules::SourceInstallRule,
    pub home: PathBuf,
    pub paths: Vec<PathBuf>,
    pub state: Option<PathBuf>,
    pub shared_tools: bool,
    startup: Vec<(PathBuf, crate::core::model::TargetIdentity)>,
}

pub(super) fn read_json(path: &Path) -> Option<Value> {
    use std::os::windows::fs::MetadataExt;
    let md = std::fs::symlink_metadata(path).ok()?;
    if !md.is_file()
        || md.file_attributes() & winapi::um::winnt::FILE_ATTRIBUTE_REPARSE_POINT != 0
        || md.len() > 4 * 1024 * 1024
    {
        return None;
    }
    serde_json::from_str(
        std::fs::read_to_string(path)
            .ok()?
            .trim_start_matches('\u{feff}'),
    )
    .ok()
}

fn install_key(code: &Path) -> String {
    format!("{:x}", Sha256::digest(code.to_string_lossy().as_bytes()))[..16].to_owned()
}

/// Existing components must not be redirected outside (or within) the claimed installation.
fn owned_path(home: &Path, path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    let Ok(relative) = path.strip_prefix(home) else {
        return false;
    };
    if relative.as_os_str().is_empty() || is_protected_residual_path(path) {
        return false;
    }
    let mut current = home.to_path_buf();
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return false;
        }
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(md)
                if md.file_attributes() & winapi::um::winnt::FILE_ATTRIBUTE_REPARSE_POINT != 0 =>
            {
                return false
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return false,
        }
    }
    true
}

fn has_other_entries(root: &Path, own: Option<&Path>) -> bool {
    use std::os::windows::fs::MetadataExt;
    match std::fs::symlink_metadata(root) {
        Ok(md) if md.file_attributes() & winapi::um::winnt::FILE_ATTRIBUTE_REPARSE_POINT != 0 => {
            return true
        }
        Ok(md) if !md.is_dir() => return true,
        Ok(_) => {}
        Err(e) => return e.kind() != std::io::ErrorKind::NotFound,
    }
    match std::fs::read_dir(root) {
        Ok(entries) => {
            for (count, entry) in entries.enumerate() {
                if count >= 256 {
                    return true;
                }
                let Ok(entry) = entry else { return true };
                if own.is_some_and(|p| norm(p) == norm(&entry.path())) {
                    continue;
                }
                let Ok(ft) = entry.file_type() else {
                    return true;
                };
                if ft.is_dir() || ft.is_symlink() {
                    return true;
                }
            }
            false
        }
        Err(e) => e.kind() != std::io::ErrorKind::NotFound,
    }
}

#[cfg(test)]
type HermesPlan = SourceInstallPlan;

impl SourceInstallPlan {
    pub(super) fn observed_paths(&self) -> Vec<PathBuf> {
        let mut paths = self.paths.clone();
        paths.extend(self.startup.iter().map(|(path, _)| path.clone()));
        paths
    }

    pub fn build_with_rule(
        code: &Path,
        source_owned: bool,
        rule: crate::core::rules::SourceInstallRule,
    ) -> Result<Self, String> {
        let code = canonical_target(code).ok_or("Invalid Hermes source path")?;
        let home = canonical_local(code.parent().ok_or("Missing Hermes home")?)
            .ok_or("Invalid Hermes home")?;
        if is_protected_residual_path(&home) || !owned_path(&home, &code) {
            return Err("Redirected/protected Hermes source".into());
        }
        let r = &rule;
        let state_path = home.join(&r.state_dir).join(install_key(&code));
        let state_exists = match std::fs::symlink_metadata(&state_path) {
            Ok(_) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(format!("Unreadable Hermes install record: {e}")),
        };
        let state = if state_exists {
            if !owned_path(&home, &state_path) {
                return Err("Redirected Hermes install record".into());
            }
            if !owned_path(&home, &state_path.join(&r.state_facts)) {
                return Err("Redirected install facts".into());
            }
            let facts = read_json(&state_path.join(&r.state_facts))
                .ok_or("Unreadable Hermes install record")?;
            let environment = facts
                .pointer(&r.environment_pointer)
                .and_then(Value::as_str)
                .ok_or("Missing Hermes environment record")?;
            let environment = canonical_target(Path::new(environment))
                .ok_or("Invalid Hermes environment path")?;
            if !at_or_under(
                &norm(&environment),
                &norm(&state_path.join(&r.environments)),
            ) || !owned_path(&home, &environment)
                || environment
                    .file_name()
                    .is_none_or(|n| n != r.environment_name.as_str())
            {
                return Err("Hermes environment escapes its installation".into());
            }
            Some(state_path)
        } else {
            None
        };
        if !source_owned && state.is_none() {
            return Err("No source or dependency ownership evidence".into());
        }
        // A named profile can use this same checkout. Removing it would break that profile.
        if has_other_entries(&home.join(&r.profiles), None) {
            return Err("Hermes source is shared by named profiles".into());
        }
        let shared_tools = has_other_entries(&home.join(&r.state_dir), state.as_deref());
        let mut paths = vec![code.clone()];
        if let Some(state) = &state {
            paths.push(state.clone());
        }
        // Only recorded command names, never the entire bin (which may also contain uv or user tools).
        for name in &r.launchers {
            if !shared_tools {
                paths.push(home.join(name));
            }
        }
        if !shared_tools {
            let tool_facts = home.join(&r.tools_dir).join(&r.tool_facts);
            if !owned_path(&home, &tool_facts) {
                return Err("Redirected Hermes tools".into());
            }
            let facts = match std::fs::symlink_metadata(&tool_facts) {
                Ok(_) => {
                    Some(read_json(&tool_facts).ok_or("Unreadable Hermes tool ownership record")?)
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(format!("Unreadable Hermes tools: {e}")),
            };
            if let Some(facts) = facts {
                let packages = facts
                    .get("packages")
                    .and_then(Value::as_object)
                    .ok_or("Invalid tool facts")?;
                for (package, fact) in packages {
                    let entry = fact
                        .get("entry")
                        .and_then(Value::as_str)
                        .ok_or("Missing tool entry")?;
                    let digest = fact.get("digest").and_then(Value::as_str).unwrap_or("");
                    if Path::new(entry).components().count() != 1
                        || entry == "."
                        || entry == ".."
                        || !entry.starts_with(&format!("{package}-"))
                        || digest.len() != 64
                        || !digest.bytes().all(|c| c.is_ascii_hexdigit())
                    {
                        return Err("Invalid tool ownership record".into());
                    }
                    paths.push(home.join(&r.tools_dir).join(entry));
                }
                paths.push(home.join(&r.tools_dir).join(&r.tool_facts));
                paths.push(home.join(&r.tools_dir).join(&r.tool_lock));
            }
            let setup = home.join(&r.installer);
            if !owned_path(&home, &setup) || !owned_path(&home, &home.join(&r.bootstrap_dir)) {
                return Err("Redirected Hermes bootstrap files".into());
            }
            if std::fs::metadata(&setup).is_ok_and(|m| m.len() <= 16 * 1024 * 1024) {
                if let Ok(bytes) = std::fs::read(&setup) {
                    if bytes.starts_with(b"MZ")
                        && bytes
                            .windows(r.installer_signature.len())
                            .any(|w| w == r.installer_signature.as_bytes())
                    {
                        paths.push(setup);
                    }
                }
            }
            if let Ok(entries) = std::fs::read_dir(home.join(&r.bootstrap_dir)) {
                for entry in entries.take(256) {
                    let entry = entry.map_err(|e| e.to_string())?;
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with(&r.bootstrap_prefix)
                        && r.bootstrap_extensions
                            .iter()
                            .any(|ext| name.ends_with(&format!(".{ext}")))
                    {
                        if !owned_path(&home, &entry.path())
                            || !entry
                                .metadata()
                                .is_ok_and(|m| m.is_file() && m.len() <= 4 * 1024 * 1024)
                        {
                            return Err("Invalid bootstrap script".into());
                        }
                        let text =
                            std::fs::read_to_string(entry.path()).map_err(|e| e.to_string())?;
                        if text.contains(&r.source) {
                            paths.push(entry.path());
                        }
                    }
                }
            }
        }
        let gateway = home.join(&r.gateway_dir);
        if !owned_path(&home, &gateway) {
            return Err("Redirected Hermes Gateway".into());
        }
        if !shared_tools && gateway.is_dir() {
            let proof = r.gateway_files.iter().any(|name| {
                std::fs::read_to_string(gateway.join(name))
                    .is_ok_and(|text| gateway_reference_with_rule(&text, &home, r))
            });
            if proof {
                paths.push(gateway);
            }
        }
        let mut startup = Vec::new();
        if !shared_tools {
            if let Some(roaming) = super::user_env::real_user_roaming_appdata() {
                let root = roaming.join(r"Microsoft\Windows\Start Menu\Programs\Startup");
                for name in &r.gateway_files {
                    let path = root.join(name);
                    if std::fs::read_to_string(&path)
                        .is_ok_and(|text| gateway_reference_with_rule(&text, &home, r))
                    {
                        let identity = crate::core::model::capture_identity(&path)
                            .ok_or("Unverified Gateway Startup entry")?;
                        startup.push((path, identity));
                    }
                }
            }
        }
        if !paths.iter().all(|p| owned_path(&home, p)) {
            return Err("Redirected Hermes program artifact".into());
        }
        paths.sort();
        paths.dedup();
        if paths.iter().any(|p| {
            r.preserve.iter().any(|keep| {
                let keep = norm(&home.join(keep));
                at_or_under(&norm(p), &keep) || at_or_under(&keep, &norm(p))
            })
        }) {
            return Err("Preserved installation target overlap".into());
        }
        Ok(Self {
            rule,
            home,
            paths,
            state,
            shared_tools,
            startup,
        })
    }

    #[cfg(test)]
    pub fn build(code: &Path, source_owned: bool) -> Result<Self, String> {
        Self::build_with_rule(
            code,
            source_owned,
            crate::core::rules::current()
                .definition("hermes")
                .app
                .clone()
                .unwrap(),
        )
    }

    pub fn ensure_idle(&self) -> Result<(), String> {
        let processes = super::process::try_list_processes()?;
        if let Some(p) = busy_process_with_rule(&self.home, &processes, &self.rule) {
            return Err(format!(
                "Hermes install/application still running: {} (PID {})",
                p.exe_name, p.pid
            ));
        }
        Ok(())
    }

    pub fn execute(
        &self,
        scanned: &crate::core::rules::CleanupPlan,
        executions: &mut Vec<crate::core::rules::flow::PlanExecution>,
        shortcuts: &[(PathBuf, crate::core::model::TargetIdentity)],
        executable: &Path,
        command: &OfficialUninstaller,
        settle: std::time::Duration,
    ) -> Result<(), String> {
        self.execute_with(
            ExecutionContext {
                scanned,
                executions,
                settle: Some((command, settle)),
            },
            shortcuts,
            executable,
            || self.ensure_idle(),
            || {
                // Official modules may remove shared launchers; the owned supplement is still safe.
                if self.shared_tools {
                    return Ok(());
                }
                let frozen = scanned
                    .official
                    .as_ref()
                    .ok_or("Missing scanned official operation")?;
                let command = frozen.command()?;
                let exit = super::process::run_official_command(command)?;
                if exit != 0 {
                    return Err(format!("Official uninstall failed (exit {exit:#x})"));
                }
                Ok(())
            },
            || self.clean_registrations(),
        )
    }

    #[cfg(test)]
    fn scanned_plan(
        &self,
        shortcuts: &[PathBuf],
    ) -> Result<crate::core::rules::CleanupPlan, String> {
        let snapshot = crate::core::rules::current();
        let (definition, _) = snapshot
            .applications()
            .find(|(_, rule)| rule.source == self.rule.source && rule.module == self.rule.module)
            .ok_or("Missing fixture installation rule")?;
        let reference = crate::core::rules::RuleRef::new(&definition.id, Some(self.home.clone()));
        let root = self.home.join(&self.rule.source);
        let mut scanned = crate::core::rules::CleanupPlan::new(
            reference,
            vec![crate::core::rules::PlannedTarget {
                path: root.clone(),
                operation: crate::core::rules::Operation::OfficialUninstall,
                identity: crate::core::model::capture_identity(&root),
                disposal: Disposal::Permanent,
            }],
        );
        scanned.installation = Some(crate::core::rules::InstallationInstance::capture(
            root,
            &self.observed_paths(),
        )?);
        scanned
            .installation
            .as_mut()
            .unwrap()
            .observe_scan_paths(shortcuts)?;
        Ok(scanned)
    }

    #[cfg(test)]
    pub(super) fn finish_with(
        &self,
        shortcuts: &[(PathBuf, crate::core::model::TargetIdentity)],
        executable: &Path,
        idle: impl Fn() -> Result<(), String>,
        registrations: impl Fn() -> Result<(), String>,
    ) -> Result<(), String> {
        let scanned = self.scanned_plan(
            &shortcuts
                .iter()
                .map(|(path, _)| path.clone())
                .collect::<Vec<_>>(),
        )?;
        self.execute_with(
            ExecutionContext {
                scanned: &scanned,
                executions: &mut vec![],
                settle: None,
            },
            shortcuts,
            executable,
            idle,
            || Ok(()),
            registrations,
        )
    }

    fn execute_with(
        &self,
        context: ExecutionContext<'_>,
        shortcuts: &[(PathBuf, crate::core::model::TargetIdentity)],
        executable: &Path,
        idle: impl Fn() -> Result<(), String>,
        official: impl Fn() -> Result<(), String>,
        registrations: impl Fn() -> Result<(), String>,
    ) -> Result<(), String> {
        use crate::core::rules::execution::SourceAction;
        let ExecutionContext {
            scanned,
            executions,
            settle,
        } = context;
        let progress = CleanProgress::default();
        let report = crate::core::rules::flow::execute_source(scanned, 0, &progress, |action| {
            crate::log!("Installation {}: {action:?}", self.rule.name);
            match action {
                SourceAction::Revalidate => {
                    idle()?;
                    self.recheck_shared()
                }
                SourceAction::OfficialOperation => official(),
                SourceAction::SupplementCleanup => {
                    let instance = scanned
                        .installation
                        .as_ref()
                        .ok_or("Missing scanned installation instance")?;
                    instance.validate_live_paths(
                        &self.home.join(&self.rule.source),
                        &self.observed_paths(),
                    )?;
                    self.supplement_with(instance, shortcuts, executable, &idle, &registrations)
                }
                SourceAction::VerifyCompletion => {
                    self.verify_with(&idle, &registrations)?;
                    if let Some((command, budget)) = settle {
                        // Stability is part of completion: a resurrecting artifact keeps the
                        // step failed and recovery records alive.
                        self.await_stable(&idle, command, budget)
                    } else {
                        Ok(())
                    }
                }
                SourceAction::RemoveRecoveryRecords => {
                    idle()?;
                    self.recheck_shared()?;
                    self.remove_recovery()
                }
            }
        });
        let result = crate::core::rules::flow::execution_result(&report);
        executions.extend(report.plan_executions);
        result
    }

    fn supplement_with(
        &self,
        instance: &crate::core::rules::InstallationInstance,
        shortcuts: &[(PathBuf, crate::core::model::TargetIdentity)],
        executable: &Path,
        idle: impl Fn() -> Result<(), String>,
        registrations: impl Fn() -> Result<(), String>,
    ) -> Result<(), String> {
        idle()?;
        self.recheck_shared()?;
        self.remove_state_contents()?;
        for (path, identity) in &self.startup {
            if instance.scanned_identity(path)?.is_none() {
                continue;
            }
            if !identity.recheck(path)
                || !std::fs::read_to_string(path)
                    .is_ok_and(|text| gateway_reference_with_rule(&text, &self.home, &self.rule))
            {
                return Err("Gateway Startup entry changed".into());
            }
            self.remove_paths(std::slice::from_ref(path), false)?;
        }
        registrations()?;
        for (path, identity) in shortcuts {
            if instance.scanned_identity(path)?.is_none() {
                continue;
            }
            if !identity.recheck(path) || !super::app_discovery::shortcut_targets(path, executable)
            {
                return Err(format!("Shortcut changed: {}", path.display()));
            }
            self.remove_paths(std::slice::from_ref(path), false)?;
        }
        // Keep the install record until every other operation succeeds, enabling retry after source removal.
        let paths: Vec<_> = self
            .paths
            .iter()
            .filter(|p| {
                Some(p.as_path()) != self.state.as_deref()
                    && *p
                        != &self
                            .home
                            .join(&self.rule.tools_dir)
                            .join(&self.rule.tool_facts)
            })
            .cloned()
            .collect();
        self.remove_paths(&paths, true)?;
        Ok(())
    }

    /// Polls the frozen artifact list until it stays absent and the install stays idle for
    /// several consecutive samples, or the settle budget expires.
    fn await_stable(
        &self,
        idle: &impl Fn() -> Result<(), String>,
        command: &OfficialUninstaller,
        budget: std::time::Duration,
    ) -> Result<(), String> {
        let artifacts = self.before_recovery_artifacts(&command.installed_artifacts);
        let deadline = std::time::Instant::now() + budget;
        let mut stable = 0;
        loop {
            if super::app_discovery::artifacts_removed(&artifacts)
                && idle().is_ok()
                && self.recheck_shared().is_ok()
            {
                stable += 1;
                if stable >= 4 {
                    return Ok(());
                }
            } else {
                stable = 0;
            }
            if std::time::Instant::now() >= deadline {
                return Err(format!(
                    "Official uninstall incomplete: {}",
                    command.provider
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    fn verify_with(
        &self,
        idle: impl Fn() -> Result<(), String>,
        registrations: impl Fn() -> Result<(), String>,
    ) -> Result<(), String> {
        idle()?;
        self.recheck_shared()?;
        registrations()?;
        let artifacts = self.before_recovery_artifacts(&self.paths);
        if !super::app_discovery::artifacts_removed(&artifacts) {
            return Err("Program artifacts remain before recovery cleanup".into());
        }
        Ok(())
    }

    // These exact records are deliberately retained until the final lifecycle step.
    fn before_recovery_artifacts(&self, paths: &[PathBuf]) -> Vec<PathBuf> {
        let tool_facts = self
            .home
            .join(&self.rule.tools_dir)
            .join(&self.rule.tool_facts);
        paths
            .iter()
            .filter(|path| Some(path.as_path()) != self.state.as_deref() && **path != tool_facts)
            .cloned()
            .collect()
    }

    fn recheck_shared(&self) -> Result<(), String> {
        if has_other_entries(&self.home.join(&self.rule.profiles), None)
            || (!self.shared_tools
                && has_other_entries(&self.home.join(&self.rule.state_dir), self.state.as_deref()))
        {
            return Err("Installation dependencies gained another owner".into());
        }
        Ok(())
    }

    fn remove_state_contents(&self) -> Result<(), String> {
        let Some(state) = &self.state else {
            return Ok(());
        };
        if super::app_discovery::artifacts_removed(std::slice::from_ref(state)) {
            return Ok(());
        }
        if !owned_path(&self.home, state) {
            return Err("Install record redirected".into());
        }
        let mut paths = Vec::new();
        let record = norm(&state.join(&self.rule.state_facts));
        let mut ancestors = vec![state.clone()];
        let mut count = 0;
        while let Some(directory) = ancestors.pop() {
            for entry in std::fs::read_dir(&directory).map_err(|e| e.to_string())? {
                count += 1;
                if count > 256 {
                    return Err("Install record enumeration limit".into());
                }
                let entry = entry.map_err(|e| e.to_string())?;
                let path = entry.path();
                if !owned_path(&self.home, &path) {
                    return Err("Install state child redirected".into());
                }
                if norm(&path) == record {
                    continue;
                }
                if at_or_under(&record, &norm(&path)) {
                    if path
                        .strip_prefix(state)
                        .map_err(|e| e.to_string())?
                        .components()
                        .count()
                        > 32
                    {
                        return Err("Install record ancestor depth limit".into());
                    }
                    ancestors.push(path);
                } else {
                    paths.push(path);
                }
            }
        }
        self.remove_paths(&paths, true)
    }

    fn remove_recovery(&self) -> Result<(), String> {
        let tool_facts = self
            .home
            .join(&self.rule.tools_dir)
            .join(&self.rule.tool_facts);
        if self.paths.contains(&tool_facts) {
            self.remove_paths(std::slice::from_ref(&tool_facts), true)?;
        }
        if let Some(state) = &self.state {
            self.remove_paths(std::slice::from_ref(state), true)?;
        }
        if !super::app_discovery::artifacts_removed(&self.paths) {
            return Err("Hermes program artifacts remain".into());
        }
        if self.shared_tools {
            crate::log!("Hermes: shared tools preserved for other installations");
        }
        Ok(())
    }

    fn remove_paths(&self, paths: &[PathBuf], require_owned: bool) -> Result<(), String> {
        let mut targets = Vec::new();
        for path in paths {
            if super::app_discovery::artifacts_removed(std::slice::from_ref(path)) {
                continue;
            }
            if require_owned && !owned_path(&self.home, path) {
                return Err(format!("Artifact changed: {}", path.display()));
            }
            let target = ArbitraryTarget::capture(path.clone());
            if target.identity.is_none() {
                return Err(format!("Unverified artifact: {}", path.display()));
            }
            targets.push(target);
        }
        let report =
            clean_arbitrary_items(&targets, Disposal::Permanent, &CleanProgress::default());
        if !report.failed.is_empty()
            || !report.skipped_items.is_empty()
            || !report.manual.is_empty()
            || targets
                .iter()
                .any(|t| !super::app_discovery::artifacts_removed(std::slice::from_ref(&t.path)))
        {
            return Err("Hermes program cleanup incomplete; see cleanup log".into());
        }
        Ok(())
    }

    fn clean_registrations(&self) -> Result<(), String> {
        let system = std::env::var_os("SystemRoot").ok_or("Missing SystemRoot")?;
        let command = OfficialUninstaller {
            provider: "Hermes registration cleanup".into(),
            executable: PathBuf::from(system)
                .join(r"System32\WindowsPowerShell\v1.0\powershell.exe"),
            arguments: vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-Command".into(),
                registration_script_with_rule(
                    &self.home,
                    "Environment",
                    !self.shared_tools,
                    true,
                    &self.rule,
                ),
            ],
            working_directory: self.home.clone(),
            installed_artifacts: vec![],
        };
        let exit = super::process::run_official_command(&command)?;
        if exit != 0 {
            return Err(format!(
                "Hermes registration cleanup/verification failed ({exit})"
            ));
        }
        Ok(())
    }
}

fn ps_literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn gateway_reference_with_rule(
    text: &str,
    home: &Path,
    rule: &crate::core::rules::SourceInstallRule,
) -> bool {
    let text = text.to_lowercase().replace('/', "\\");
    [&rule.gateway_dir, &rule.source]
        .iter()
        .any(|dir| text.contains(&format!("{}\\", norm(&home.join(dir)))))
}

pub(super) fn busy_process_with_rule<'a>(
    home: &Path,
    processes: &'a [super::process::RunningProcess],
    rule: &crate::core::rules::SourceInstallRule,
) -> Option<&'a super::process::RunningProcess> {
    processes.iter().find(|p| {
        p.exe_name.eq_ignore_ascii_case(&rule.installer_process)
            || at_or_under(&p.image_path, &norm(home))
            || (p.image_path.is_empty()
                && rule
                    .unknown_processes
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&p.exe_name)))
    })
}

#[cfg(test)]
fn busy_process<'a>(
    home: &Path,
    processes: &'a [super::process::RunningProcess],
) -> Option<&'a super::process::RunningProcess> {
    busy_process_with_rule(
        home,
        processes,
        &crate::core::rules::current()
            .definition("hermes")
            .app
            .clone()
            .unwrap(),
    )
}
#[cfg(test)]
fn registration_script(home: &Path, environment_key: &str, exclusive: bool, tasks: bool) -> String {
    registration_script_with_rule(
        home,
        environment_key,
        exclusive,
        tasks,
        &crate::core::rules::current()
            .definition("hermes")
            .app
            .clone()
            .unwrap(),
    )
}

fn registration_script_with_rule(
    home: &Path,
    environment_key: &str,
    exclusive: bool,
    tasks: bool,
    rule: &crate::core::rules::SourceInstallRule,
) -> String {
    // Only our generated script runs, in the install user's token. Values never become PowerShell source.
    let array = |values: &[String]| {
        format!(
            "@({})",
            values
                .iter()
                .map(|s| ps_literal(s))
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    let options=format!("$taskPrefix={}; $gatewayDir={}; $gatewayFiles={}; $pathNames={}; $sourceName={}; $environmentNames={};\n",ps_literal(&rule.task_prefix),ps_literal(&rule.gateway_dir),array(&rule.gateway_files),array(&rule.path_prefixes),ps_literal(&rule.source),array(&rule.environment_variables));
    format!(
        "$installHome = {}; $environmentKey = {}; $exclusive = {}; $checkTasks = {};\n{}{}",
        ps_literal(&home.to_string_lossy()),
        ps_literal(environment_key),
        if exclusive { "$true" } else { "$false" },
        if tasks { "$true" } else { "$false" },
        options,
        REGISTRATION_SCRIPT
    )
}

const REGISTRATION_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
function Under([string]$value, [string]$root) {
    $value = [Environment]::ExpandEnvironmentVariables($value.Trim().Trim('"')).Replace('/', '\').TrimEnd('\')
    $root = $root.Replace('/', '\').TrimEnd('\')
    return $value.Equals($root, [StringComparison]::OrdinalIgnoreCase) -or $value.StartsWith($root + '\', [StringComparison]::OrdinalIgnoreCase)
}
try {
    if ($checkTasks) {
        $service = New-Object -ComObject 'Schedule.Service'; $service.Connect()
        if ($exclusive) {
            $folder = $service.GetFolder('\')
            foreach ($task in @($folder.GetTasks(1))) {
                if ($task.Name -notlike ($taskPrefix + '*')) { continue }
                $xml = [xml]$task.Xml; $owned = $false
                foreach ($action in $xml.Task.Actions.Exec) {
                    $text = ([string]$action.Command + ' ' + [string]$action.Arguments).Replace('/', '\')
                    if ($text.IndexOf($installHome.TrimEnd('\') + '\' + $gatewayDir + '\', [StringComparison]::OrdinalIgnoreCase) -ge 0) { $owned = $true }
                }
                if ($owned) {
                    if ($task.GetInstances(0).Count -gt 0) { $task.Stop(0) }
                    $folder.DeleteTask($task.Name, 0)
                    if (@($folder.GetTasks(1) | Where-Object { $_.Name -eq $task.Name }).Count -gt 0) { throw 'Gateway task remains' }
                }
            }
        }
        $startup = [Environment]::GetFolderPath('Startup')
        foreach ($name in $gatewayFiles) {
            $entry = Join-Path $startup $name
            if ($exclusive -and (Test-Path -LiteralPath $entry)) {
                $text = [IO.File]::ReadAllText($entry).Replace('/', '\')
                if ($text.IndexOf($installHome.TrimEnd('\') + '\' + $gatewayDir + '\', [StringComparison]::OrdinalIgnoreCase) -ge 0) { throw 'Gateway Startup entry remains' }
            }
        }
    }
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($environmentKey, $true)
    if ($null -ne $key) {
        try {
            $names = @($sourceName); if ($exclusive) { $names = $pathNames }
            $prefixes = $names | ForEach-Object { Join-Path $installHome $_ }
            $pathValue = $key.GetValue('Path', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            if ($null -ne $pathValue) {
                $kept = @(([string]$pathValue).Split(';') | Where-Object {
                    $entry = $_; -not (@($prefixes | Where-Object { Under $entry $_ }).Count -gt 0)
                })
                $key.SetValue('Path', ($kept -join ';'), $key.GetValueKind('Path'))
                if (@(([string]$key.GetValue('Path')).Split(';') | Where-Object {
                    $entry = $_; @($prefixes | Where-Object { Under $entry $_ }).Count -gt 0
                }).Count -gt 0) { throw 'Hermes PATH remains' }
            }
            foreach ($name in $environmentNames) {
                $value = $key.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
                if ($exclusive -and $null -ne $value -and (Under ([string]$value) $installHome)) { $key.DeleteValue($name) }
            }
        } finally { $key.Dispose() }
    }
    exit 0
} catch { [Console]::Error.WriteLine($_.Exception.Message); exit 1 }
"#;

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let tag = format!(
                "qc_hermes & ' $(audit)_{}",
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            Self(canonical_local(&crate::core::testing::fixture(&tag)).unwrap())
        }
        fn file(&self, relative: &str, content: &str) -> PathBuf {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, content).unwrap();
            path
        }
        fn layout(&self) -> PathBuf {
            self.file("hermes-agent/pyproject.toml", "name = \"hermes-agent\"");
            let code = self.0.join("hermes-agent");
            let relative = format!(
                "installs/{}/environments/generation/venv/pyvenv.cfg",
                install_key(&code)
            );
            let environment = self
                .file(&relative, "home = runtime")
                .parent()
                .unwrap()
                .to_path_buf();
            let facts =
                serde_json::json!({"schema":1,"packages":{"venv":{"environment":environment}}});
            self.file(
                &format!("installs/{}/facts.json", install_key(&code)),
                &facts.to_string(),
            );
            self.file("tools/python-verified/python.exe", "runtime");
            self.file("tools/facts.json", &serde_json::json!({"packages":{"python":{"entry":"python-verified","digest":"a".repeat(64)}}}).to_string());
            self.file("tools/untracked.txt", "not a recorded tool");
            self.file("config.yaml", "preserve configuration");
            self.file("sessions/keep.txt", "preserve sessions");
            self.file(".env", "preserve credentials");
            code
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Err(error) = std::fs::remove_dir_all(&self.0) {
                if !std::thread::panicking() {
                    panic!("Fixture cleanup failed: {error}");
                }
            }
        }
    }

    #[test]
    fn split_runtime_cleanup_preserves_data_and_unrecorded_files() {
        let f = Fixture::new();
        let code = f.layout();
        f.file("hermes-setup.exe", "MZ hermes_bootstrap_lib");
        f.file(
            "bootstrap-cache/install-main.ps1",
            "# hermes-agent installer",
        );
        let plan = HermesPlan::build(&code, true).unwrap();
        assert!(plan.paths.contains(plan.state.as_ref().unwrap()));
        assert!(plan.paths.contains(&f.0.join("tools/python-verified")));
        // A successful no-op official command leaves the program intact; verified core cleanup completes it.
        plan.finish_with(&[], &code.join("missing.exe"), || Ok(()), || Ok(()))
            .unwrap();
        assert!(super::super::app_discovery::artifacts_removed(&plan.paths));
        for (path, text) in [
            ("config.yaml", "preserve configuration"),
            ("sessions/keep.txt", "preserve sessions"),
            (".env", "preserve credentials"),
            ("tools/untracked.txt", "not a recorded tool"),
        ] {
            assert_eq!(std::fs::read_to_string(f.0.join(path)).unwrap(), text);
        }
    }

    #[test]
    fn supplement_rechecks_frozen_shortcut_after_registration_callback() {
        let f = Fixture::new();
        let code = f.layout();
        let shortcut = f.file("fixture.lnk", "original");
        let identity = crate::core::model::capture_identity(&shortcut).unwrap();
        let plan = HermesPlan::build(&code, true).unwrap();
        let mut instance =
            crate::core::rules::InstallationInstance::capture(code.clone(), &plan.observed_paths())
                .unwrap();
        instance
            .observe_scan_paths(std::slice::from_ref(&shortcut))
            .unwrap();
        let error = plan
            .supplement_with(
                &instance,
                &[(shortcut.clone(), identity)],
                &code.join("missing.exe"),
                || Ok(()),
                || {
                    std::fs::rename(&shortcut, f.0.join("original.lnk")).unwrap();
                    std::fs::write(&shortcut, "replaced").unwrap();
                    Ok(())
                },
            )
            .unwrap_err();
        assert!(error.contains("Scanned installation artifact"), "{error}");
        assert_eq!(std::fs::read_to_string(shortcut).unwrap(), "replaced");
        assert!(plan.state.unwrap().join("facts.json").is_file());
        assert!(
            code.is_dir(),
            "dependency failure must stop remaining cleanup"
        );
    }

    #[test]
    fn supplement_retains_same_content_startup_replacement() {
        let f = Fixture::new();
        let code = f.layout();
        let mut plan = HermesPlan::build(&code, true).unwrap();
        let text = format!(
            "@echo off\n{}\\start.cmd",
            norm(&f.0.join(&plan.rule.gateway_dir))
        );
        assert!(gateway_reference_with_rule(&text, &f.0, &plan.rule));
        let startup = f.file("startup.cmd", &text);
        let modified = std::fs::metadata(&startup).unwrap().modified().unwrap();
        let identity = crate::core::model::capture_identity(&startup).unwrap();
        plan.startup.push((startup.clone(), identity));
        let instance =
            crate::core::rules::InstallationInstance::capture(code.clone(), &plan.observed_paths())
                .unwrap();
        let error = plan
            .supplement_with(
                &instance,
                &[],
                &code.join("missing.exe"),
                || {
                    std::fs::rename(&startup, f.0.join("saved-startup.cmd")).unwrap();
                    std::fs::write(&startup, &text).unwrap();
                    std::fs::File::options()
                        .write(true)
                        .open(&startup)
                        .unwrap()
                        .set_modified(modified)
                        .unwrap();
                    assert!(
                        !identity.recheck(&startup),
                        "the replaced startup must no longer match its scanned identity"
                    );
                    Ok(())
                },
                || Ok(()),
            )
            .unwrap_err();
        assert!(error.contains("Scanned installation artifact"), "{error}");
        assert_eq!(std::fs::read_to_string(startup).unwrap(), text);
        assert!(plan.state.unwrap().join("facts.json").is_file());
    }

    #[test]
    fn source_missing_recovers_from_install_identity_not_a_directory_name() {
        let f = Fixture::new();
        let code = f.layout();
        std::fs::remove_dir_all(&code).unwrap();
        let plan = HermesPlan::build(&code, false).unwrap();
        assert!(plan.state.is_some());
        let bad = f.0.join("unrelated/hermes-agent");
        assert!(HermesPlan::build(&bad, false).is_err());
        let facts = plan.state.unwrap().join("facts.json");
        std::fs::write(
            facts,
            serde_json::json!({"packages":{"venv":{"environment":f.0.join("sessions/venv")}}})
                .to_string(),
        )
        .unwrap();
        assert!(HermesPlan::build(&code, false).is_err());
    }

    #[test]
    fn other_installations_and_profiles_do_not_lose_shared_tools() {
        let f = Fixture::new();
        let code = f.layout();
        f.file("installs/other-install/facts.json", "{}");
        f.file("bin/hermes.exe", "shared launcher");
        let plan = HermesPlan::build(&code, true).unwrap();
        assert!(plan.shared_tools);
        assert!(!plan
            .paths
            .iter()
            .any(|p| at_or_under(&norm(p), &norm(&f.0.join("tools")))));
        plan.finish_with(&[], &code.join("missing.exe"), || Ok(()), || Ok(()))
            .unwrap();
        assert!(f.0.join("tools/python-verified/python.exe").is_file());
        assert!(f.0.join("bin/hermes.exe").is_file());
        assert!(f.0.join("installs/other-install/facts.json").is_file());
        f.file("profiles/another/config.yaml", "shared checkout");
        assert!(HermesPlan::build(&code, false).is_err());
    }

    #[test]
    fn failed_registration_or_new_owner_keeps_retry_evidence() {
        let f = Fixture::new();
        let code = f.layout();
        let plan = HermesPlan::build(&code, true).unwrap();
        assert!(plan
            .finish_with(&[], &code, || Ok(()), || Err("registration failed".into()))
            .is_err());
        assert!(code.exists() && plan.state.as_ref().unwrap().exists());
        f.file("installs/new-owner/facts.json", "{}");
        assert!(plan.finish_with(&[], &code, || Ok(()), || Ok(())).is_err());
        assert!(f.0.join("tools/python-verified/python.exe").exists());
    }

    #[test]
    fn late_failure_keeps_install_record_for_recovery() {
        let f = Fixture::new();
        let code = f.layout();
        let plan = HermesPlan::build(&code, true).unwrap();
        let checks = std::cell::Cell::new(0);
        assert!(plan
            .finish_with(
                &[],
                &code,
                || Ok(()),
                || {
                    checks.set(checks.get() + 1);
                    if checks.get() == 1 {
                        Ok(())
                    } else {
                        Err("verification failed".into())
                    }
                }
            )
            .is_err());
        assert!(!code.exists());
        assert!(plan.state.as_ref().unwrap().join("facts.json").is_file());
        assert!(f.0.join("tools/facts.json").is_file());
        let retry = HermesPlan::build(&code, false).unwrap();
        retry.finish_with(&[], &code, || Ok(()), || Ok(())).unwrap();
        assert!(super::super::app_discovery::artifacts_removed(&retry.paths));
        assert!(f.0.join("sessions/keep.txt").is_file());
    }

    #[test]
    fn verification_runs_after_runtime_cleanup_and_before_record_removal() {
        let fixture = Fixture::new();
        let code = fixture.layout();
        let plan = HermesPlan::build(&code, true).unwrap();
        let state = plan.state.as_ref().unwrap();
        assert!(state.join("environments").is_dir());
        plan.finish_with(
            &[],
            &code,
            || Ok(()),
            || {
                assert!(!state.join("environments").exists());
                assert!(state.join("facts.json").is_file());
                Ok(())
            },
        )
        .unwrap();
        assert!(!state.exists());
    }

    #[test]
    fn settle_verification_reports_failure_and_keeps_recovery_records() {
        let fixture = Fixture::new();
        let code = fixture.layout();
        let plan = HermesPlan::build(&code, true).unwrap();
        let state = plan.state.as_ref().unwrap().clone();
        let scanned = plan.scanned_plan(&[]).unwrap();
        let mut executions = Vec::new();
        // The frozen command still claims a preserved artifact: completion must never stabilize.
        let command = OfficialUninstaller {
            provider: "fixture".into(),
            executable: code.join("missing.exe"),
            arguments: vec![],
            working_directory: fixture.0.clone(),
            installed_artifacts: vec![fixture.0.join("config.yaml")],
        };
        let result = plan.execute_with(
            ExecutionContext {
                scanned: &scanned,
                executions: &mut executions,
                settle: Some((&command, std::time::Duration::from_millis(100))),
            },
            &[],
            &code,
            || Ok(()),
            || Ok(()),
            || Ok(()),
        );
        assert_eq!(
            result.unwrap_err(),
            "Official uninstall incomplete: fixture"
        );
        let steps = &executions[0].steps;
        assert_eq!(
            steps[3].status,
            crate::core::rules::flow::StepStatus::Failed
        );
        assert_eq!(
            steps[4].status,
            crate::core::rules::flow::StepStatus::Blocked
        );
        assert!(
            state.join("facts.json").is_file(),
            "an unstable completion cannot retire recovery records"
        );
    }

    #[test]
    fn settle_verification_succeeds_and_retires_recovery_records() {
        let fixture = Fixture::new();
        let code = fixture.layout();
        let plan = HermesPlan::build(&code, true).unwrap();
        let state = plan.state.as_ref().unwrap().clone();
        let scanned = plan.scanned_plan(&[]).unwrap();
        let mut executions = Vec::new();
        let command = OfficialUninstaller {
            provider: "fixture".into(),
            executable: code.join("missing.exe"),
            arguments: vec![],
            working_directory: fixture.0.clone(),
            installed_artifacts: plan.paths.clone(),
        };
        plan.execute_with(
            ExecutionContext {
                scanned: &scanned,
                executions: &mut executions,
                settle: Some((&command, std::time::Duration::from_secs(10))),
            },
            &[],
            &code,
            || Ok(()),
            || Ok(()),
            || Ok(()),
        )
        .unwrap();
        assert!(executions[0]
            .steps
            .iter()
            .all(|step| step.status == crate::core::rules::flow::StepStatus::Succeeded));
        assert!(
            !state.exists(),
            "verified completion retires the install record"
        );
        assert!(fixture.0.join("config.yaml").is_file());
    }

    #[test]
    fn settle_exempts_only_exact_recovery_records_and_rechecks_shared_owners() {
        let fixture = Fixture::new();
        let code = fixture.layout();
        let plan = HermesPlan::build(&code, true).unwrap();
        let state = plan.state.as_ref().unwrap();
        let facts = fixture
            .0
            .join(&plan.rule.tools_dir)
            .join(&plan.rule.tool_facts);
        let child = state.join("environments/remaining");
        let sibling = state.with_extension("other");
        let paths = vec![
            state.clone(),
            facts,
            child.clone(),
            sibling.clone(),
            code.clone(),
        ];
        assert_eq!(
            plan.before_recovery_artifacts(&paths),
            [child, sibling, code]
        );
        let command = OfficialUninstaller {
            provider: "fixture".into(),
            executable: fixture.0.join("missing.exe"),
            arguments: vec![],
            working_directory: fixture.0.clone(),
            installed_artifacts: vec![fixture.0.join("absent")],
        };
        let error = plan
            .await_stable(
                &|| {
                    std::fs::create_dir_all(fixture.0.join(&plan.rule.profiles).join("new-owner"))
                        .unwrap();
                    Ok(())
                },
                &command,
                std::time::Duration::ZERO,
            )
            .unwrap_err();
        assert!(error.contains("Official uninstall incomplete"));
        assert!(state.join("facts.json").is_file());
    }

    #[test]
    fn nested_recovery_record_keeps_only_record_until_verification() {
        let fixture = Fixture::new();
        let code = fixture.layout();
        let initial = HermesPlan::build(&code, true).unwrap();
        let state = initial.state.as_ref().unwrap();
        std::fs::create_dir_all(state.join("records")).unwrap();
        std::fs::rename(state.join("facts.json"), state.join("records/facts.json")).unwrap();
        std::fs::write(state.join("records/dependency"), b"owned state").unwrap();
        let mut rule = initial.rule.clone();
        rule.state_facts = "records/facts.json".into();
        let plan = SourceInstallPlan::build_with_rule(&code, true, rule).unwrap();
        plan.finish_with(
            &[],
            &code,
            || Ok(()),
            || {
                assert!(state.join("records/facts.json").is_file());
                assert!(!state.join("records/dependency").exists());
                assert!(!state.join("environments").exists());
                Ok(())
            },
        )
        .unwrap();
        assert!(!state.exists());
    }

    #[test]
    fn install_and_split_runtime_processes_block_cleanup() {
        use super::super::process::RunningProcess;
        let home = Path::new(r"C:\Users\test\AppData\Local\hermes");
        for image in [
            r"C:\Users\test\AppData\Local\hermes\tools\node\node.exe",
            r"C:\Users\test\AppData\Local\hermes\installs\key\venv\python.exe",
        ] {
            let processes = [RunningProcess {
                pid: 1,
                exe_name: "node.exe".into(),
                image_path: norm(Path::new(image)),
            }];
            assert!(busy_process(home, &processes).is_some());
        }
        let processes = [RunningProcess {
            pid: 2,
            exe_name: "hermes-setup.exe".into(),
            image_path: r"c:\downloads\hermes-setup.exe".into(),
        }];
        assert!(busy_process(home, &processes).is_some());
        let unrelated = [RunningProcess {
            pid: 3,
            exe_name: "node.exe".into(),
            image_path: r"c:\tools\node.exe".into(),
        }];
        assert!(busy_process(home, &unrelated).is_none());
    }

    #[test]
    fn registration_cleanup_handles_quoted_paths_and_preserves_unrelated_values() {
        use std::os::windows::process::CommandExt;
        let f = Fixture::new();
        let key = format!(r"Software\QuickCleaner\Tests\{}", install_key(&f.0));
        let setup = format!("$k=[Microsoft.Win32.Registry]::CurrentUser.CreateSubKey({}); $k.SetValue('Path', {}); $k.SetValue('HERMES_HOME', {}); $k.SetValue('HERMES_GIT_BASH_PATH','C:\\other\\git'); $k.Dispose();", ps_literal(&key), ps_literal(&format!("{};{};C:\\keep", f.0.join("bin").display(), f.0.join("bin-other").display())), ps_literal(&f.0.to_string_lossy()));
        let verify = format!("$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey({}); try {{ if ($k.GetValue('Path') -ne {} -or $null -ne $k.GetValue('HERMES_HOME') -or $k.GetValue('HERMES_GIT_BASH_PATH') -ne 'C:\\other\\git') {{ throw 'Unrelated registration changed' }} }} finally {{ $k.Dispose(); [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree({}) }}", ps_literal(&key), ps_literal(&format!("{};C:\\keep", f.0.join("bin-other").display())), ps_literal(&key));
        // exit inside a script block exits the whole process: use a separate child for each phase.
        let run = |script: String| {
            std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                .creation_flags(winapi::um::winbase::CREATE_NO_WINDOW)
                .output()
                .unwrap()
        };
        let setup_result = run(setup);
        assert!(setup_result.status.success());
        let command = OfficialUninstaller {
            provider: "isolated registration test".into(),
            executable: PathBuf::from(std::env::var_os("SystemRoot").unwrap())
                .join(r"System32\WindowsPowerShell\v1.0\powershell.exe"),
            arguments: vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-Command".into(),
                registration_script(&f.0, &key, true, false),
            ],
            working_directory: f.0.clone(),
            installed_artifacts: vec![],
        };
        let result = super::super::process::run_official_command(&command);
        let verification = run(verify);
        assert_eq!(result.unwrap(), 0);
        assert!(
            verification.status.success(),
            "{}",
            String::from_utf8_lossy(&verification.stderr)
        );
    }

    #[test]
    #[ignore = "creates disabled isolated Task Scheduler entries"]
    fn gateway_task_cleanup_requires_exact_installation_action() {
        use std::os::windows::process::CommandExt;
        let f = Fixture::new();
        let suffix = install_key(&f.0);
        let owned = format!("Hermes_Gateway_QC_{suffix}");
        let unrelated = format!("Hermes_Gateway_QC_{suffix}_other");
        let key = format!(r"Software\QuickCleaner\Tests\{suffix}");
        let prelude = format!("$ErrorActionPreference='Stop'; $s=New-Object -ComObject Schedule.Service; $s.Connect(); $folder=$s.GetFolder('\\'); $owned={}; $other={};", ps_literal(&owned), ps_literal(&unrelated));
        let setup = format!("{prelude} try {{ foreach ($pair in @(@($owned,{}),@($other,{}))) {{ $d=$s.NewTask(0); $d.Settings.Enabled=$false; $a=$d.Actions.Create(0); $a.Path=$env:ComSpec; $a.Arguments=$pair[1]; $null=$folder.RegisterTaskDefinition($pair[0],$d,6,$null,$null,3,$null) }} }} catch {{ foreach ($name in @($owned,$other)) {{ try {{ $folder.DeleteTask($name,0) }} catch {{}} }} throw }}", ps_literal(&format!("/c rem {}", f.0.join(r"gateway-service\Hermes_Gateway.cmd").display())), ps_literal(&format!("/c rem {}", f.0.join(r"other\gateway-service\Hermes_Gateway.cmd").display())));
        let verify = format!("{prelude} try {{ $names=@($folder.GetTasks(1) | ForEach-Object {{$_.Name}}); if ($names -contains $owned -or $names -notcontains $other) {{ throw 'Task ownership verification failed' }} }} finally {{ foreach ($name in @($owned,$other)) {{ try {{ $folder.DeleteTask($name,0) }} catch {{}} }} }}");
        let run = |script: String| {
            std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                .creation_flags(winapi::um::winbase::CREATE_NO_WINDOW)
                .output()
                .unwrap()
        };
        let setup_result = run(setup);
        assert!(
            setup_result.status.success(),
            "{}",
            String::from_utf8_lossy(&setup_result.stderr)
        );
        let result = run(registration_script(&f.0, &key, true, true));
        // Verification always removes both fixture tasks, including on cleanup failure.
        let verification = run(verify);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            verification.status.success(),
            "{}",
            String::from_utf8_lossy(&verification.stderr)
        );
    }
}
