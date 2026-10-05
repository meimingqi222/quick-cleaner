//! Non-ARP discovery. Launch evidence never grants ownership of its parent directory.

use crate::core::apps::{AppDiscovery, AppRegRoot, InstalledApp, OfficialUninstaller};
use crate::core::safety::{at_or_under, is_protected_residual_path, is_system_root_dir, norm};
use std::collections::HashMap;
use std::path::{Path, PathBuf, Prefix};
use std::sync::atomic::{AtomicBool, Ordering};
use winapi::Interface;

pub(super) fn discover_apps(registered: &[InstalledApp], live: &AtomicBool) -> Vec<InstalledApp> {
    let mut candidates = Vec::new();
    if let Some(com) = ShortcutReader::new() {
        for base in shortcut_roots() {
            for entry in walkdir::WalkDir::new(base)
                .min_depth(1)
                .max_depth(4)
                .follow_links(false)
                .into_iter()
                .flatten()
                .take(4000)
            {
                if !live.load(Ordering::Relaxed) {
                    return Vec::new();
                }
                let path = entry.path();
                if !entry.file_type().is_file()
                    || !path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
                {
                    continue;
                }
                if let Some(target) = com.target(path) {
                    candidates.push((target, Some(path.to_path_buf())));
                }
            }
        }
    }
    // Adapters can also supply well-known layouts, including CLI-only installs with no shortcuts.
    if let Some(local) = super::user_env::real_user_local_appdata() {
        let snapshot = crate::core::rules::current();
        for (_, rule) in snapshot.applications() {
            let home = local.join(&rule.default_home);
            for relative in &rule.targets {
                let target = home.join(relative);
                if target.is_file() || inspect_adapter(&target).is_some() {
                    candidates.push((target, None));
                }
            }
        }
    }
    if !live.load(Ordering::Relaxed) {
        return Vec::new();
    }
    merge_candidates(registered, candidates)
}

fn shortcut_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = super::user_env::real_user_home() {
        roots.push(home.join("Desktop"));
        // Includes redirected desktops without assuming the current process user's profile.
        if let Some(desktop) = super::user_env::real_user_desktop() {
            roots.push(desktop);
        }
    }
    if let Some(roaming) = super::user_env::real_user_roaming_appdata() {
        roots.push(roaming.join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Some(pd) = std::env::var_os("ProgramData") {
        roots.push(PathBuf::from(pd).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Some(public) = std::env::var_os("PUBLIC") {
        roots.push(PathBuf::from(public).join("Desktop"));
    }
    roots.sort();
    roots.dedup();
    roots
}

fn local_executable(path: &Path) -> Option<PathBuf> {
    match path.components().next()? {
        std::path::Component::Prefix(p)
            if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)) => {}
        _ => return None,
    }
    if !path.is_absolute()
        || !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        || is_protected_residual_path(path)
        || path.parent().is_some_and(is_system_root_dir)
        || !path.is_file()
    {
        return None;
    }
    let canonical = canonical_local(path)?;
    (!is_protected_residual_path(&canonical)).then_some(canonical)
}

pub(super) fn canonical_local(path: &Path) -> Option<PathBuf> {
    let canonical = path.canonicalize().ok()?;
    let text = canonical.to_str()?;
    // Safety rules and identity keys use ordinary drive paths, not Win32 verbatim prefixes.
    let path = PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(text));
    matches!(path.components().next(), Some(std::path::Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_)))
        .then_some(path)
}

pub(super) fn canonical_target(path: &Path) -> Option<PathBuf> {
    if let Some(path) = canonical_local(path) {
        return Some(path);
    }
    if !matches!(std::fs::symlink_metadata(path), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
    {
        return None;
    }
    let name = path.file_name()?;
    if name == "." || name == ".." {
        return None;
    }
    Some(canonical_target(path.parent()?)?.join(name))
}

fn covered_by_registry(target: &Path, paths: &[PathBuf]) -> bool {
    let key = norm(target);
    paths.iter().any(|p| at_or_under(&key, &norm(p)))
}

fn merge_candidates(
    registered: &[InstalledApp],
    candidates: Vec<(PathBuf, Option<PathBuf>)>,
) -> Vec<InstalledApp> {
    let mut apps: HashMap<String, InstalledApp> = HashMap::new();
    let registered_paths: Vec<_> = registered
        .iter()
        .flat_map(|app| {
            [
                app.icon_cache_key(),
                super::apps::deduce_install_location(app),
            ]
        })
        .flatten()
        .filter(|p| !is_protected_residual_path(p))
        .filter_map(|p| canonical_local(&p))
        .collect();
    for (target, shortcut) in candidates {
        let Some(target) = local_executable(&target).or_else(|| {
            let target = canonical_target(&target)?;
            if is_protected_residual_path(&target) {
                return None;
            }
            inspect_adapter(&target)?;
            Some(target)
        }) else {
            continue;
        };
        if covered_by_registry(&target, &registered_paths) {
            continue;
        }
        let adapter = inspect_adapter(&target);
        let identity = adapter
            .as_ref()
            .map(|a| norm(&a.code))
            .unwrap_or_else(|| norm(&target));
        let id = format!("discovered:{identity}");
        let app = apps.entry(id.clone()).or_insert_with(|| {
            let name = adapter.as_ref().map(|a| a.name.clone()).unwrap_or_else(|| {
                shortcut
                    .as_ref()
                    .unwrap_or(&target)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            });
            let version = adapter
                .as_ref()
                .map(|a| a.version.clone())
                .unwrap_or_default();
            let uninstaller = adapter.as_ref().and_then(|a| a.uninstaller.clone());
            let location = adapter
                .as_ref()
                .map(|a| a.code.clone())
                .unwrap_or_else(|| target.clone());
            InstalledApp {
                discovery: Some(AppDiscovery {
                    plan: adapter.as_ref().and_then(|a| a.cleanup.clone()),
                    rule: adapter.as_ref().map(|a| a.rule.clone()),
                    executable: target.clone(),
                    shortcuts: Vec::new(),
                    program_paths: adapter
                        .as_ref()
                        .map(|a| a.paths.clone())
                        .unwrap_or_else(|| vec![target.clone()]),
                    uninstaller,
                }),
                id,
                name,
                version,
                publisher: String::new(),
                last_used_date: None,
                last_used_raw: 0,
                install_date: None,
                install_date_raw: 0,
                install_location: Some(location),
                display_icon: Some(target.to_string_lossy().into_owned()),
                uninstall_string: None,
                quiet_uninstall_string: None,
                estimated_size: 0,
                registry_root: AppRegRoot::Unregistered,
                registry_subpath: String::new(),
                is_system_component: false,
                uninstaller_missing: false,
            }
        });
        if let Some(shortcut) = shortcut {
            let paths = &mut app.discovery.as_mut().unwrap().shortcuts;
            if !paths.contains(&shortcut) {
                paths.push(shortcut);
            }
        }
    }
    let mut apps: Vec<_> = apps.into_values().collect();
    for app in &mut apps {
        let discovery = app.discovery.as_mut().unwrap();
        discovery.shortcuts.sort();
        discovery.shortcuts.dedup();
        if let Some(frozen) = &discovery.plan {
            let mut plan = (**frozen).clone();
            if let Some(instance) = &mut plan.installation {
                if let Err(reason) = instance.observe_scan_paths(&discovery.shortcuts) {
                    plan.blocked.push(reason);
                }
            }
            discovery.plan = Some(std::sync::Arc::new(plan));
        }
    }
    apps.sort_by(|a, b| a.id.cmp(&b.id));
    apps
}

struct AdapterMatch {
    cleanup: Option<std::sync::Arc<crate::core::rules::CleanupPlan>>,
    rule: crate::core::rules::RuleRef,
    name: String,
    code: PathBuf,
    version: String,
    uninstaller: Option<OfficialUninstaller>,
    paths: Vec<PathBuf>,
}

fn inspect_adapter(target: &Path) -> Option<AdapterMatch> {
    let snapshot = crate::core::rules::current();
    let found = snapshot.applications().find_map(|(definition, rule)| {
        source_adapter(
            target,
            rule,
            crate::core::rules::RuleRef {
                snapshot: snapshot.clone(),
                id: definition.id.clone(),
                scope: None,
                contributors: Vec::new(),
                blocked: None,
                observation: None,
            },
        )
    });
    found
}

pub fn explain_source_install(root: &Path, id: &str) -> Result<serde_json::Value, String> {
    let snapshot = crate::core::rules::current();
    let definition = snapshot
        .bundle
        .rules
        .iter()
        .find(|definition| definition.id == id)
        .ok_or("Unknown rule")?;
    let rule = definition
        .app
        .as_ref()
        .ok_or("Missing source installation layout")?;
    let adapter = rule
        .targets
        .iter()
        .find_map(|target| {
            source_adapter(
                &root.join(target),
                rule,
                crate::core::rules::RuleRef::new(id, Some(root.to_path_buf())),
            )
        })
        .ok_or("Installation ownership cannot be confirmed")?;
    Ok(serde_json::json!({
        "instance": adapter.code, "targets": adapter.paths,
        "plan": adapter.cleanup.as_ref().map(|plan| plan.explanation()),
        "official_operation_available": adapter.uninstaller.is_some(),
        "actions": rule.actions, "completion": rule.completion,
        "preserve": rule.preserve.iter().map(|path| root.join(path)).collect::<Vec<_>>()
    }))
}

fn source_adapter(
    target: &Path,
    rule: &crate::core::rules::SourceInstallRule,
    mut rule_ref: crate::core::rules::RuleRef,
) -> Option<AdapterMatch> {
    let home = target.ancestors().take(12).find(|home| {
        rule.targets
            .iter()
            .any(|p| norm(&home.join(p)) == norm(target))
    })?;
    rule_ref.scope = Some(home.to_path_buf());
    rule_ref.revalidate().ok()?;
    let code = canonical_target(&home.join(&rule.source))?;
    let home = canonical_local(home)?;
    // Content/layout signatures, not the shortcut's display name, select an adapter.
    let source_file = code.join(&rule.source_file);
    let project = crate::core::rules::facts::confined_file(&code, &source_file)
        .ok()
        .filter(|present| *present)
        .and_then(|_| crate::core::rules::facts::read(&source_file).ok())
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default();
    let source_signature = project.lines().any(|l| l.trim() == rule.source_signature)
        && code.join(&rule.constants).is_file();
    let source_owned = source_signature
        && std::fs::symlink_metadata(code.join(&rule.source_owner))
            .is_ok_and(|md| !crate::core::rules::facts::is_link(&md));
    let plan = if source_signature && !source_owned {
        None
    } else {
        super::source_install::SourceInstallPlan::build_with_rule(&code, source_owned, rule.clone())
            .ok()
    };
    if (!source_signature && plan.is_none())
        || is_protected_residual_path(&code)
        || is_protected_residual_path(&home)
    {
        return None;
    }
    let version = std::fs::read_to_string(code.join(&rule.stamp))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.pointer(&rule.version_pointer)?
                .as_str()
                .map(str::to_owned)
        })
        .unwrap_or_default();
    let mut cleanup = if let Some(installation) = &plan {
        let mut cleanup = crate::core::rules::CleanupPlan::new(
            rule_ref.clone(),
            vec![crate::core::rules::PlannedTarget {
                path: code.clone(),
                operation: crate::core::rules::Operation::OfficialUninstall,
                identity: crate::core::model::capture_identity(&code),
                disposal: crate::core::cleaner::Disposal::Permanent,
            }],
        );
        cleanup.installation = Some(
            crate::core::rules::InstallationInstance::capture(
                code.clone(),
                &installation.observed_paths(),
            )
            .ok()?,
        );
        Some(cleanup)
    } else {
        None
    };
    // Code removal is supported by Hermes only for source installs, never sealed/MSIX bundles.
    let uninstaller = if let Some(plan) = &plan {
        // A module file that is absent at scan must not authorize the interpreter route: the
        // frozen launcher would runpy a file dropped after scanning. Missing or redirected
        // module evidence freezes the no-op recovery route instead.
        let module = code.join(&rule.module_file);
        let module_ok = std::fs::symlink_metadata(&module)
            .is_ok_and(|md| md.is_file() && !crate::core::rules::facts::is_link(&md));
        let found = module_ok
            .then(|| {
                external_python(&home, &code, rule).map(|python| (python, vec![module.clone()]))
            })
            .flatten();
        found
            .map(|(python, evidence)| {
                let script = "import os,runpy,sys;root,home,variable,module,relative=sys.argv[1:6];args=sys.argv[6:];sys.path.insert(0,root);os.environ[variable]=home;os.environ.pop('PYTHONPATH',None);sys.argv=[module]+args;os.path.isfile(os.path.join(root,relative)) and runpy.run_module(module,run_name='__main__')";
                let mut arguments=vec!["-I".into(),"-c".into(),script.into(),code.to_string_lossy().into_owned(),home.to_string_lossy().into_owned(),rule.home_variable.clone(),rule.module.clone(),rule.module_file.clone()];
                arguments.extend(rule.module_args.clone());
                (
                    OfficialUninstaller {
                        provider: rule.name.clone(), executable: python,
                        arguments,
                        // The preserved home is outside the source tree that the module removes.
                        working_directory: home.clone(),
                        installed_artifacts: plan.paths.clone(),
                    },
                    evidence,
                )
            })
            .or_else(|| {
                Some((
                    OfficialUninstaller {
                        provider: format!("{} recovery",rule.name),
                        executable: PathBuf::from(std::env::var_os("SystemRoot")?).join(r"System32\WindowsPowerShell\v1.0\powershell.exe"),
                        arguments: vec!["-NoProfile".into(), "-NonInteractive".into(), "-Command".into(), "exit 0".into()],
                        working_directory: home.clone(), installed_artifacts: plan.paths.clone(),
                    },
                    Vec::new(),
                ))
            })
            .and_then(|(command, evidence)| {
                if let Some(plan) = &mut cleanup {
                    plan.official = Some(
                        crate::core::rules::OfficialOperation::capture(&command, &evidence).ok()?,
                    );
                }
                Some(command)
            })
    } else {
        None
    };
    Some(AdapterMatch {
        cleanup: cleanup.map(std::sync::Arc::new),
        rule: rule_ref,
        name: rule.name.clone(),
        code: if plan.is_some() {
            code
        } else {
            target.to_path_buf()
        },
        version,
        uninstaller,
        paths: plan
            .map(|p| p.paths)
            .unwrap_or_else(|| vec![target.to_path_buf()]),
    })
}

fn external_python(
    home: &Path,
    code: &Path,
    rule: &crate::core::rules::SourceInstallRule,
) -> Option<PathBuf> {
    let mut candidates: Vec<_> = std::fs::read_dir(home.join(&rule.tools_dir))
        .ok()?
        .flatten()
        .take(100)
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(&rule.python_prefix)
        })
        .map(|e| e.path().join("python.exe"))
        .collect();
    candidates.sort();
    candidates
        .into_iter()
        .filter_map(|p| canonical_local(&p))
        .find(|p| {
            p.is_file() && !at_or_under(&norm(p), &norm(code)) && !is_protected_residual_path(p)
        })
}

pub(super) fn artifacts_removed(paths: &[PathBuf]) -> bool {
    !paths.is_empty() && paths.iter().all(|p| {
        matches!(std::fs::symlink_metadata(p), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
    })
}

pub(super) fn run_uninstaller(app: &InstalledApp) -> Result<(), String> {
    run_uninstaller_reported(app).result
}

pub(super) fn run_uninstaller_reported(app: &InstalledApp) -> crate::core::apps::UninstallOutcome {
    let snapshot = app
        .discovery
        .as_ref()
        .and_then(|d| d.rule.as_ref())
        .map(|r| r.snapshot.clone())
        .unwrap_or_else(crate::core::rules::current);
    let mut plan_executions = Vec::new();
    let result = crate::core::rules::with_snapshot(snapshot, || {
        run_uninstaller_with_timeout(
            app,
            std::time::Duration::from_secs(120),
            &mut plan_executions,
        )
    });
    if let Err(reason) = &result {
        if plan_executions.is_empty() {
            if let Some(plan) = app.discovery.as_ref().and_then(|d| d.plan.as_ref()) {
                plan_executions.push(crate::core::rules::flow::blocked_execution(plan, 0, reason));
            }
        }
    }
    crate::core::apps::UninstallOutcome {
        result,
        plan_executions,
    }
}

pub(super) fn validate_residual_clean(
    app_id: &str,
    items: &[crate::core::apps::ResidualItem],
) -> Result<(), String> {
    let Some(code) = app_id.strip_prefix("discovered:").map(Path::new) else {
        return Ok(());
    };
    let home = canonical_target(code.parent().ok_or("Missing install home")?)
        .ok_or("Invalid install home")?;
    let snapshot = crate::core::rules::current();
    let Some((_, rule)) = snapshot
        .applications()
        .find(|(_, r)| norm(&home.join(&r.source)) == norm(code))
    else {
        return Ok(());
    };
    let adapter = inspect_adapter(&home.join(&rule.targets[0]));
    for item in items {
        if item.source != crate::core::apps::ResidualSource::InstallDir {
            continue;
        }
        if let crate::core::apps::ResidualKind::File(path, _)
        | crate::core::apps::ResidualKind::Directory(path, _) = &item.kind
        {
            if !adapter
                .as_ref()
                .is_some_and(|a| a.paths.iter().any(|p| norm(p) == norm(path)))
            {
                return Err("Program ownership changed since residual scan".into());
            }
        }
    }
    let processes = super::process::try_list_processes()?;
    if super::source_install::busy_process_with_rule(&home, &processes, rule).is_some() {
        return Err("Hermes install/application still running".into());
    }
    Ok(())
}

fn run_uninstaller_with_timeout(
    app: &InstalledApp,
    timeout: std::time::Duration,
    executions: &mut Vec<crate::core::rules::flow::PlanExecution>,
) -> Result<(), String> {
    let discovery = app.discovery.as_ref().ok_or("Missing discovery evidence")?;
    let scanned = discovery
        .plan
        .as_ref()
        .ok_or("Missing scanned installation plan")?;
    scanned.validate()?;
    // Live layout revalidates the scanned instance; it cannot add new deletion targets.
    let adapter =
        inspect_adapter(&discovery.executable).ok_or("Official installation evidence changed")?;
    if scanned.rule.id != adapter.rule.id
        || !std::sync::Arc::ptr_eq(&scanned.rule.snapshot, &adapter.rule.snapshot)
    {
        return Err("Installation rule differs from the scanned plan".into());
    }
    // The scanned command is authoritative: a replaced interpreter or a runtime that appears
    // later must not redirect execution to a different executable.
    let command = scanned
        .official
        .as_ref()
        .ok_or("Missing scanned official operation")?
        .command()?;
    let rule = adapter
        .rule
        .snapshot
        .definition(&adapter.rule.id)
        .app
        .clone()
        .ok_or("Missing install rule")?;
    let plan = super::source_install::SourceInstallPlan::build_with_rule(
        &adapter.code,
        adapter.code.join(&rule.source_owner).exists(),
        rule,
    )?;
    scanned
        .installation
        .as_ref()
        .ok_or("Missing scanned installation instance")?
        .validate_live_paths(&adapter.code, &plan.observed_paths())?;
    plan.ensure_idle()?;
    let mut shortcuts = Vec::new();
    for path in &discovery.shortcuts {
        if shortcut_targets(path, &discovery.executable) {
            if let Some(identity) = scanned
                .installation
                .as_ref()
                .ok_or("Missing scanned shortcut evidence")?
                .scanned_identity(path)?
            {
                shortcuts.push((path.clone(), identity));
            }
        }
    }
    crate::log!("Official uninstall: {} / {}", app.name, command.provider);
    plan.execute(
        scanned,
        executions,
        &shortcuts,
        &discovery.executable,
        command,
        timeout,
    )
}

struct ShortcutReader {
    initialized: bool,
}

pub(super) fn shortcut_targets(path: &Path, executable: &Path) -> bool {
    ShortcutReader::new()
        .and_then(|reader| reader.target(path))
        .and_then(|target| canonical_target(&target))
        .is_some_and(|target| norm(&target) == norm(executable))
}

impl ShortcutReader {
    fn new() -> Option<Self> {
        // This scanner runs on a background thread. Pair each successful COM initialization.
        let hr = unsafe {
            winapi::um::combaseapi::CoInitializeEx(
                std::ptr::null_mut(),
                winapi::um::objbase::COINIT_APARTMENTTHREADED,
            )
        };
        if hr >= 0 {
            Some(Self { initialized: true })
        } else if hr == winapi::shared::winerror::RPC_E_CHANGED_MODE {
            Some(Self { initialized: false })
        } else {
            None
        }
    }

    fn target(&self, path: &Path) -> Option<PathBuf> {
        use std::os::windows::fs::MetadataExt;
        use winapi::um::combaseapi::CoCreateInstance;
        use winapi::um::objidl::IPersistFile;
        const CLSID_SHELL_LINK: winapi::shared::guiddef::GUID = winapi::shared::guiddef::GUID {
            Data1: 0x00021401,
            Data2: 0,
            Data3: 0,
            Data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
        };
        use winapi::um::shobjidl_core::IShellLinkW;
        use winapi::um::winnt::FILE_ATTRIBUTE_REPARSE_POINT;
        if std::fs::symlink_metadata(path).ok()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT
            != 0
        {
            return None;
        }
        let mut link: *mut IShellLinkW = std::ptr::null_mut();
        // Only Load/GetPath: Resolve can contact networks, search disks or update the shortcut.
        unsafe {
            let hr = CoCreateInstance(
                &CLSID_SHELL_LINK,
                std::ptr::null_mut(),
                winapi::shared::wtypesbase::CLSCTX_INPROC_SERVER,
                &IShellLinkW::uuidof(),
                &mut link as *mut _ as *mut _,
            );
            if hr < 0 || link.is_null() {
                return None;
            }
            let mut persist: *mut IPersistFile = std::ptr::null_mut();
            let hr =
                (*link).QueryInterface(&IPersistFile::uuidof(), &mut persist as *mut _ as *mut _);
            if hr < 0 || persist.is_null() {
                (*link).Release();
                return None;
            }
            let wide = super::registry::to_wide(&path.to_string_lossy());
            let loaded = (*persist).Load(wide.as_ptr(), 0) >= 0;
            (*persist).Release();
            let mut buf = vec![0u16; 32768];
            let hr = if loaded {
                (*link).GetPath(buf.as_mut_ptr(), buf.len() as i32, std::ptr::null_mut(), 0)
            } else {
                -1
            };
            (*link).Release();
            (hr >= 0).then(|| PathBuf::from(super::registry::from_wide(&buf)))
        }
    }
}

impl Drop for ShortcutReader {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                winapi::um::combaseapi::CoUninitialize();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::apps::{
        AppFilterPreset, Confidence, ResidualItem, ResidualKind, ResidualSource,
    };

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "qc-discovery-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(canonical_local(&path).unwrap())
        }
        fn file(&self, path: &str, content: &str) -> PathBuf {
            let path = self.0.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, content).unwrap();
            canonical_local(&path).unwrap()
        }
        fn shortcut(&self, name: &str, target: &Path) -> PathBuf {
            use std::os::windows::process::CommandExt;
            let path = self.0.join(name);
            let status = std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command", "$link = (New-Object -ComObject WScript.Shell).CreateShortcut($env:QC_LINK_PATH); $link.TargetPath = $env:QC_LINK_TARGET; $link.Save()"])
                .env("QC_LINK_PATH", &path).env("QC_LINK_TARGET", target)
                .creation_flags(winapi::um::winbase::CREATE_NO_WINDOW)
                .status().unwrap();
            assert!(status.success());
            path
        }
        fn hermes(&self) -> PathBuf {
            self.file(
                "hermes-agent/pyproject.toml",
                "[project]\nname = \"hermes-agent\"\n",
            );
            self.file(
                "hermes-agent/hermes_cli/uninstall.py",
                "# official module fixture",
            );
            self.file("hermes-agent/hermes_constants.py", "");
            self.file("hermes-agent/.git/HEAD", "ref: refs/heads/main");
            self.file(
                "hermes-agent/install-stamp.json",
                r#"{"displayVersion":"0.21.5"}"#,
            );
            self.file("tools/python-fixture/python.exe", "runtime outside source");
            self.file("bin/hermes.exe", "launcher");
            self.file(
                "hermes-agent/apps/desktop/release/win-unpacked/Hermes.exe",
                "GUI",
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn another_layout_is_discovered_and_cleaned_using_only_a_rule() {
        let f = Fixture::new();
        let definition: crate::core::rules::RuleDefinition =
            toml::from_str(include_str!("../../../rules/fixtures/atlas.toml")).unwrap();
        let mut bundle = crate::core::rules::current().bundle.clone();
        bundle.rules.push(definition);
        bundle.validate().unwrap();
        let snapshot = std::sync::Arc::new(crate::core::rules::RuleSnapshot { bundle });
        f.file("checkout/project.txt", "owner = atlas-fixture");
        f.file("checkout/atlas_constants.py", "");
        f.file("checkout/.owner", "fixture ownership");
        f.file("checkout/atlas_cli/remove.py", "# fixture official module");
        f.file("preferences.json", "keep");
        f.file("history/keep.txt", "keep");
        f.file(".secrets", "keep");
        f.file("dependencies/untracked.txt", "keep");
        let executable = f.file("commands/atlas.exe", "fixture launcher");
        crate::core::rules::with_snapshot(snapshot, || {
            let adapter = inspect_adapter(&executable).expect("rule identifies a different layout");
            assert_eq!(adapter.name, "Atlas Fixture");
            let explanation = explain_source_install(&f.0, "atlas-fixture").unwrap();
            assert_eq!(
                explanation["targets"],
                serde_json::to_value(&adapter.paths).unwrap()
            );
            assert_eq!(explanation["actions"].as_array().unwrap().len(), 5);
            assert_eq!(
                explanation["completion"],
                "artifacts_and_registrations_absent"
            );
            let rule = adapter
                .rule
                .snapshot
                .definition(&adapter.rule.id)
                .app
                .clone()
                .unwrap();
            let plan = super::super::source_install::SourceInstallPlan::build_with_rule(
                &adapter.code,
                true,
                rule,
            )
            .unwrap();
            assert!(plan.paths.contains(&executable));
            assert!(!plan.paths.contains(&f.0));
            plan.finish_with(&[], &adapter.code.join("absent.exe"), || Ok(()), || Ok(()))
                .unwrap();
            assert!(!executable.exists());
            assert!(!adapter.code.exists());
            for path in [
                "preferences.json",
                "history/keep.txt",
                ".secrets",
                "dependencies/untracked.txt",
            ] {
                assert!(f.0.join(path).is_file(), "{path}");
            }
        });
    }

    #[test]
    fn discovered_installation_plan_rejects_new_artifacts_and_keeps_snapshot() {
        let f = Fixture::new();
        let executable = f.hermes();
        let apps = merge_candidates(&[], vec![(executable.clone(), None)]);
        let discovery = apps[0].discovery.as_ref().unwrap();
        let frozen = discovery.plan.as_ref().unwrap();
        assert!(frozen.validate().is_ok());
        let cloned = apps[0].clone();
        assert!(std::sync::Arc::ptr_eq(
            frozen,
            cloned.discovery.as_ref().unwrap().plan.as_ref().unwrap()
        ));
        let snapshot = frozen.rule.snapshot.clone();
        let mut bundle = snapshot.bundle.clone();
        bundle.sequence += 1;
        bundle
            .rules
            .iter_mut()
            .find(|rule| rule.id == "hermes")
            .unwrap()
            .version += 1;
        let newer = std::sync::Arc::new(crate::core::rules::RuleSnapshot { bundle });
        crate::core::rules::with_snapshot(newer, || {
            assert!(frozen.validate().is_ok());
            assert!(std::sync::Arc::ptr_eq(&snapshot, &frozen.rule.snapshot));
        });
        f.file(
            "bootstrap-cache/install-later.ps1",
            "# hermes-agent newly claimed installer",
        );
        let live = inspect_adapter(&executable).unwrap();
        let rule = live
            .rule
            .snapshot
            .definition(&live.rule.id)
            .app
            .clone()
            .unwrap();
        let current = super::super::source_install::SourceInstallPlan::build_with_rule(
            &live.code, true, rule,
        )
        .unwrap();
        assert!(frozen
            .installation
            .as_ref()
            .unwrap()
            .validate_live_paths(&live.code, &current.observed_paths())
            .is_err());
        assert!(f.0.join("bootstrap-cache/install-later.ps1").is_file());
        let outcome = run_uninstaller_reported(&apps[0]);
        let reason = outcome.result.unwrap_err();
        assert!(reason.contains("scope expanded"), "{reason}");
        let execution = &outcome.plan_executions[0];
        assert_eq!(execution.sequence, frozen.rule.snapshot.bundle.sequence);
        assert_eq!(
            execution.rule_version,
            frozen.rule.snapshot.definition("hermes").version
        );
        assert_eq!(execution.steps.len(), 5);
        assert!(execution
            .steps
            .iter()
            .all(|step| step.status == crate::core::rules::flow::StepStatus::Blocked));
        assert_eq!(execution.steps[0].reason.as_deref(), Some(reason.as_str()));
        assert!(
            executable.is_file(),
            "early failure cannot execute the fixture runtime"
        );
    }

    #[test]
    fn discovered_shortcut_identity_is_frozen_and_missing_is_recoverable() {
        let f = Fixture::new();
        let executable = f.hermes();
        let shortcut = f.file("fixture.lnk", "original");
        let apps = merge_candidates(&[], vec![(executable, Some(shortcut.clone()))]);
        let cloned = apps[0].clone();
        let plan = apps[0].discovery.as_ref().unwrap().plan.as_ref().unwrap();
        assert!(std::sync::Arc::ptr_eq(
            plan,
            cloned.discovery.as_ref().unwrap().plan.as_ref().unwrap()
        ));
        let instance = plan.installation.as_ref().unwrap();
        assert!(instance.scanned_identity(&shortcut).unwrap().is_some());
        assert!(instance
            .scanned_identity(&f.0.join("unobserved.lnk"))
            .is_err());
        let old = f.0.join("original.lnk");
        std::fs::rename(&shortcut, &old).unwrap();
        std::fs::write(&shortcut, "replaced").unwrap();
        assert!(instance.scanned_identity(&shortcut).is_err());
        assert_eq!(std::fs::read_to_string(&shortcut).unwrap(), "replaced");
        std::fs::remove_file(&shortcut).unwrap();
        assert!(instance.scanned_identity(&shortcut).unwrap().is_none());
        assert!(old.is_file());
    }

    #[test]
    fn half_uninstalled_hermes_and_dead_shortcuts_remain_discoverable() {
        let f = Fixture::new();
        let exe = f.hermes();
        let shortcut = f.shortcut("dead.lnk", &exe);
        std::fs::remove_file(&exe).unwrap();
        std::fs::remove_file(f.0.join("bin/hermes.exe")).unwrap();
        let apps = merge_candidates(&[], vec![(exe.clone(), Some(shortcut.clone()))]);
        assert_eq!(
            apps.len(),
            1,
            "source evidence must survive missing launchers"
        );
        assert!(apps[0].can_uninstall());
        assert!(apps[0]
            .discovery
            .as_ref()
            .unwrap()
            .plan
            .as_ref()
            .unwrap()
            .validate()
            .is_ok());
        assert!(shortcut_targets(&shortcut, &exe));
        let command = apps[0]
            .discovery
            .as_ref()
            .unwrap()
            .uninstaller
            .as_ref()
            .unwrap();
        assert!(
            command
                .installed_artifacts
                .contains(&f.0.join("hermes-agent")),
            "a missing module is not proof the whole source tree is gone"
        );
    }

    #[test]
    fn dependency_record_recovers_without_source_or_runtime() {
        use sha2::{Digest, Sha256};
        let f = Fixture::new();
        let code = f.0.join("hermes-agent");
        let key =
            format!("{:x}", Sha256::digest(code.to_string_lossy().as_bytes()))[..16].to_owned();
        let environment =
            f.0.join(format!("installs/{key}/environments/generation/venv"));
        f.file(
            &format!("installs/{key}/facts.json"),
            &serde_json::json!({"schema":1,"packages":{"venv":{"environment":environment}}})
                .to_string(),
        );
        let exe = code.join(r"apps\desktop\release\win-unpacked\Hermes.exe");
        let apps = merge_candidates(&[], vec![(exe, None)]);
        assert_eq!(apps.len(), 1);
        assert!(apps[0].can_uninstall());
        assert!(apps[0]
            .discovery
            .as_ref()
            .unwrap()
            .plan
            .as_ref()
            .unwrap()
            .validate()
            .is_ok());
        assert_eq!(
            apps[0]
                .discovery
                .as_ref()
                .unwrap()
                .uninstaller
                .as_ref()
                .unwrap()
                .provider,
            "Hermes recovery"
        );
        let unrelated =
            f.0.join(r"unrelated\hermes-agent\apps\desktop\release\win-unpacked\Hermes.exe");
        assert!(merge_candidates(&[], vec![(unrelated, None)]).is_empty());
    }

    #[test]
    fn frozen_official_command_rejects_replaced_interpreter() {
        let f = Fixture::new();
        let exe = f.hermes();
        let apps = merge_candidates(&[], vec![(exe.clone(), None)]);
        let plan = apps[0].discovery.as_ref().unwrap().plan.as_ref().unwrap();
        let official = plan.official.as_ref().unwrap();
        let interpreter = official.command().unwrap().executable.clone();
        assert_eq!(
            norm(&interpreter),
            norm(&f.0.join("tools/python-fixture/python.exe"))
        );
        // Same-length content keeps weak identity plausible; the file index must still differ.
        std::fs::remove_file(&interpreter).unwrap();
        std::fs::write(&interpreter, "runtime outside-source").unwrap();
        assert!(official.command().is_err());
        assert!(plan.validate().is_err());
        let outcome = run_uninstaller_reported(&apps[0]);
        assert!(outcome.result.unwrap_err().contains("Official operation"));
        assert_eq!(
            outcome.plan_executions[0].steps[0].status,
            crate::core::rules::flow::StepStatus::Blocked
        );
        assert!(exe.is_file(), "a blocked scan must not delete artifacts");
        assert!(interpreter.is_file());
    }

    #[test]
    fn frozen_recovery_route_never_switches_to_late_runtime() {
        let f = Fixture::new();
        let exe = f.hermes();
        // Runtime absent at scan: the plan freezes the recovery command.
        std::fs::remove_dir_all(f.0.join("tools/python-fixture")).unwrap();
        let apps = merge_candidates(&[], vec![(exe, None)]);
        let plan = apps[0].discovery.as_ref().unwrap().plan.as_ref().unwrap();
        let command = plan.official.as_ref().unwrap().command().unwrap();
        assert_eq!(command.provider, "Hermes recovery");
        assert_eq!(command.executable.file_name().unwrap(), "powershell.exe");
        // A runtime appearing later must not redirect the frozen plan.
        f.file("tools/python-later/python.exe", "dropped after scan");
        let command = plan.official.as_ref().unwrap().command().unwrap();
        assert_eq!(command.provider, "Hermes recovery");
        assert!(plan.validate().is_ok());
    }

    #[test]
    fn frozen_official_command_rejects_replaced_module() {
        let f = Fixture::new();
        let exe = f.hermes();
        let apps = merge_candidates(&[], vec![(exe.clone(), None)]);
        let plan = apps[0].discovery.as_ref().unwrap().plan.as_ref().unwrap();
        let module = f.0.join("hermes-agent/hermes_cli/uninstall.py");
        std::fs::remove_file(&module).unwrap();
        std::fs::write(&module, "# official module swapped!").unwrap();
        assert!(plan.official.as_ref().unwrap().command().is_err());
        let outcome = run_uninstaller_reported(&apps[0]);
        let reason = outcome.result.unwrap_err();
        assert!(reason.contains("Official operation"), "{reason}");
        assert!(exe.is_file(), "tampered module evidence blocks execution");
        assert!(module.is_file());
    }

    #[test]
    fn missing_module_freezes_recovery_even_with_runtime() {
        let f = Fixture::new();
        let exe = f.hermes();
        std::fs::remove_file(f.0.join("hermes-agent/hermes_cli/uninstall.py")).unwrap();
        let apps = merge_candidates(&[], vec![(exe, None)]);
        let plan = apps[0].discovery.as_ref().unwrap().plan.as_ref().unwrap();
        let command = plan.official.as_ref().unwrap().command().unwrap();
        assert_eq!(
            command.provider, "Hermes recovery",
            "module absent at scan must not authorize the interpreter route"
        );
    }

    #[test]
    fn stale_residual_scan_cannot_remove_newly_shared_tools() {
        let f = Fixture::new();
        let exe = f.hermes();
        f.file("tools/facts.json", &serde_json::json!({"packages":{"python":{"entry":"python-fixture","digest":"a".repeat(64)}}}).to_string());
        let app = merge_candidates(&[], vec![(exe, None)]).remove(0);
        let mut items = Vec::new();
        super::super::residuals::scan_install_dir(&app, &mut items);
        let selected: Vec<_> = items.into_iter().filter(|item| matches!(&item.kind, ResidualKind::Directory(p, _) if p == &f.0.join("tools/python-fixture"))).collect();
        assert_eq!(selected.len(), 1);
        f.file("installs/new-owner/facts.json", "{}");
        assert_eq!(
            validate_residual_clean(&app.id, &selected).unwrap_err(),
            "Program ownership changed since residual scan"
        );
        assert!(f.0.join("tools/python-fixture/python.exe").is_file());
    }

    #[test]
    fn hermes_without_arp_is_discovered_with_official_uninstall() {
        let f = Fixture::new();
        let exe = f.hermes();
        let shortcut = f.file("Renamed assistant.lnk", "");
        let apps = merge_candidates(
            &[],
            vec![
                (exe.clone(), Some(shortcut.clone())),
                (exe, None),
                (f.0.join("bin/hermes.exe"), None),
            ],
        );
        assert_eq!(apps.len(), 1);
        let app = &apps[0];
        assert_eq!(app.name, "Hermes");
        assert_eq!(app.version, "0.21.5");
        assert!(app.registry_subpath.is_empty());
        assert!(app.can_uninstall());
        assert!(!AppFilterPreset::Orphan.matches(app, 0));
        let discovery = app.discovery.as_ref().unwrap();
        assert_eq!(discovery.shortcuts, vec![shortcut]);
        let command = discovery.uninstaller.as_ref().unwrap();
        assert!(command.arguments.contains(&"lite".to_owned()));
        assert!(!command.arguments.contains(&"full".to_owned()));
        assert!(!at_or_under(
            &norm(&command.executable),
            &norm(app.install_location.as_ref().unwrap())
        ));
        assert_eq!(command.working_directory, f.0);
    }

    #[test]
    fn portable_shortcuts_do_not_claim_parent_directory_or_uninstall_authority() {
        let f = Fixture::new();
        let exe = f.file("shared/Portable.exe", "program");
        let apps = merge_candidates(&[], vec![(exe.clone(), Some(f.file("Hermes.lnk", "")))]);
        assert_eq!(apps.len(), 1);
        assert!(
            !apps[0].can_uninstall(),
            "a Hermes display name is insufficient"
        );
        assert_eq!(apps[0].install_location.as_ref(), Some(&exe));
        assert!(!AppFilterPreset::Orphan.matches(&apps[0], 0));
        let mut residuals = Vec::new();
        super::super::residuals::scan_install_dir(&apps[0], &mut residuals);
        assert!(matches!(&residuals[0].kind, ResidualKind::File(p, _) if p == &exe));
        assert!(!residuals
            .iter()
            .any(|i| matches!(&i.kind, ResidualKind::Directory(_, _))));
    }

    #[test]
    fn registry_wins_by_path_but_same_name_different_install_survives() {
        let f = Fixture::new();
        let exe = f.file("registered/App.exe", "");
        let mut registered = merge_candidates(&[], vec![(exe.clone(), None)]).remove(0);
        registered.discovery = None;
        registered.registry_root = AppRegRoot::Hkcu;
        registered.install_location = Some(f.0.join("registered"));
        let other = f.file("registered-other/App.exe", "");
        let apps = merge_candidates(&[registered], vec![(exe, None), (other.clone(), None)]);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].discovery.as_ref().unwrap().executable, other);
    }

    #[test]
    fn sealed_hermes_is_protected_but_missing_runtime_can_recover() {
        let f = Fixture::new();
        let exe = f.hermes();
        std::fs::remove_dir_all(f.0.join("hermes-agent/.git")).unwrap();
        assert!(!merge_candidates(&[], vec![(exe.clone(), None)])[0].can_uninstall());
        f.file("hermes-agent/.git/HEAD", "ref: refs/heads/main");
        std::fs::remove_file(f.0.join("tools/python-fixture/python.exe")).unwrap();
        let app = merge_candidates(&[], vec![(exe, None)]).remove(0);
        assert!(app.can_uninstall());
        assert_eq!(
            app.discovery.unwrap().uninstaller.unwrap().provider,
            "Hermes recovery"
        );
    }

    #[test]
    fn missing_or_protected_or_network_targets_are_rejected() {
        let f = Fixture::new();
        let directory = f.0.join("Directory.exe");
        std::fs::create_dir(&directory).unwrap();
        let sys = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/cmd.exe");
        assert!(merge_candidates(
            &[],
            vec![
                (directory, None),
                (f.0.join("missing.exe"), None),
                (sys.clone(), None),
                (PathBuf::from(r"\\server\share\app.exe"), None)
            ]
        )
        .is_empty());
        assert!(local_executable(&sys.canonicalize().unwrap()).is_none());
    }

    #[test]
    fn success_requires_all_artifacts_gone_not_missing_arp_or_exit_zero() {
        let f = Fixture::new();
        let code = f.file("source/module.py", "");
        let exe = f.file("program.exe", "");
        let paths = vec![code.clone(), exe.clone()];
        assert!(!artifacts_removed(&[]));
        assert!(!artifacts_removed(&paths));
        std::fs::remove_file(exe).unwrap();
        assert!(!artifacts_removed(&paths));
        std::fs::remove_file(code).unwrap();
        assert!(artifacts_removed(&paths));
    }

    #[test]
    fn discovered_user_data_and_name_only_shortcuts_are_never_preselected() {
        let f = Fixture::new();
        let exe = f.hermes();
        let shortcut = f.shortcut("Exact.lnk", &exe);
        assert!(shortcut_targets(&shortcut, &exe));
        let app = merge_candidates(&[], vec![(exe, Some(shortcut.clone()))]).remove(0);
        let mut items = vec![
            ResidualItem::certain(
                ResidualKind::Directory(f.0.clone(), 0),
                ResidualSource::AppDataDir,
            ),
            ResidualItem::certain(ResidualKind::File(shortcut, 0), ResidualSource::Shortcut),
            ResidualItem::certain(
                ResidualKind::File(f.0.join("Other Hermes.lnk"), 0),
                ResidualSource::Shortcut,
            ),
        ];
        super::super::residuals::limit_discovered_residuals(&app, &mut items);
        assert_eq!(items[0].confidence, Confidence::Possible);
        assert_eq!(items[1].confidence, Confidence::Certain);
        assert_eq!(items[2].confidence, Confidence::Possible);
        let other = f.file("Other.exe", "unrelated program");
        f.shortcut("Exact.lnk", &other);
        super::super::residuals::limit_discovered_residuals(&app, &mut items);
        assert_eq!(
            items[1].confidence,
            Confidence::Possible,
            "retargeted shortcut is no longer certain"
        );
    }

    #[test]
    #[ignore = "read-only live installation probe"]
    fn probe_live_hermes_discovery() {
        let apps = super::super::apps::list_installed_apps(&AtomicBool::new(true));
        let matches: Vec<_> = apps.iter().filter(|a| a.name == "Hermes").collect();
        assert_eq!(matches.len(), 1);
        let app = matches[0];
        assert!(app.can_uninstall());
        println!("{} {} {:?}", app.name, app.version, app.discovery);
    }

    #[test]
    #[ignore = "requires QC_TEST_RUNTIME_DIR pointing to an external Python directory"]
    fn official_uninstall_process_preserves_data_and_completes_noop_with_verified_fallback() {
        let runtime = PathBuf::from(
            std::env::var_os("QC_TEST_RUNTIME_DIR").expect("QC_TEST_RUNTIME_DIR required"),
        );
        assert!(runtime.join("python.exe").is_file());
        for success in [false, true] {
            let f = Fixture::new();
            let exe = f.hermes();
            let userdata = f.file("sessions/keep.txt", "user data");
            let config = f.file("config.yaml", "keep configuration");
            let dummy = f.0.join("tools/python-fixture");
            std::fs::remove_dir_all(dummy).unwrap();
            let link = f.0.join("tools/python-live");
            let status = std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command", "New-Item -ItemType Junction -Path $env:QC_FIXTURE_LINK -Target $env:QC_FIXTURE_RUNTIME -ErrorAction Stop | Out-Null"])
                .env("QC_FIXTURE_LINK", &link).env("QC_FIXTURE_RUNTIME", &runtime)
                .status().unwrap();
            assert!(status.success());
            let script = if success {
                "import os,shutil,sys\nfrom pathlib import Path\nhome=Path(os.environ['HERMES_HOME'])\nassert home.name.startswith('qc-discovery-')\nassert sys.argv[1:]==['--mode','lite']\n(home/'bin/hermes.exe').unlink()\nshutil.rmtree(Path(__file__).parent.parent)\n"
            } else {
                "# Deliberately exits zero without removing artifacts.\n"
            };
            f.file("hermes-agent/hermes_cli/uninstall.py", script);
            let app = merge_candidates(&[], vec![(exe.clone(), None)]).remove(0);
            let command = app
                .discovery
                .as_ref()
                .unwrap()
                .uninstaller
                .as_ref()
                .unwrap();
            let plan = super::super::source_install::SourceInstallPlan::build(
                app.install_location.as_ref().unwrap(),
                true,
            )
            .unwrap();
            // Isolated installation: exercise the real-user runner, then inject only fixture registrations/idle.
            assert!(!command.provider.ends_with(" recovery"),
                "QC_TEST_RUNTIME_DIR must contain an independent Python runtime; Store aliases cannot exercise the official module");
            assert_eq!(
                super::super::process::run_official_command(command).unwrap(),
                0
            );
            assert_eq!(exe.exists(), !success);
            plan.finish_with(&[], &exe, || Ok(()), || Ok(())).unwrap();
            assert!(artifacts_removed(&plan.paths));
            assert_eq!(std::fs::read_to_string(userdata).unwrap(), "user data");
            assert_eq!(
                std::fs::read_to_string(config).unwrap(),
                "keep configuration"
            );
            assert!(!exe.exists());
            // Remove the junction itself before the fixture tree; the external runtime is untouched.
            std::fs::remove_dir(link).unwrap();
            assert!(runtime.join("python.exe").is_file());
        }
    }
}
