//! On-demand cache-owner evidence and explicit normal/forced exit adapters.
use crate::core::cache_release::Occupant;
use std::path::Path;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;
#[cfg(target_os = "macos")]
use macos as native;
#[cfg(windows)]
use windows as native;

pub fn occupants(path: &Path) -> Result<Vec<Occupant>, String> {
    native::occupants(path)
}
pub fn request_exit(owner: &Occupant, force: bool) -> Result<(), String> {
    native::request_exit(owner, force)
}

fn process_snapshot(pid: u32) -> Result<(String, std::path::PathBuf), String> {
    use sysinfo::{Pid, System};
    let system = System::new_all();
    let own = Pid::from_u32(std::process::id());
    let process = system
        .process(Pid::from_u32(pid))
        .ok_or("owner-exited; retry")?;
    let me = system
        .process(own)
        .ok_or("current-process-identity-unavailable")?;
    if pid <= 1 || process.user_id().is_none() || process.user_id() != me.user_id() {
        return Err("system-or-other-user-process; manual-action-required".into());
    }
    let mut ancestor = Some(Pid::from_u32(pid));
    let mut depth = 0;
    while let Some(parent) = ancestor {
        if parent == own {
            return Err("cleaner-process-tree; retry-later".into());
        }
        ancestor = system.process(parent).and_then(|process| process.parent());
        depth += 1;
        if depth > 256 {
            return Err("process-ancestry-unknown".into());
        }
    }
    let executable = process
        .exe()
        .ok_or("owner-executable-unknown")?
        .to_path_buf();
    if process.parent() == Some(Pid::from_u32(1))
        && !executable
            .ancestors()
            .any(|part| part.extension().is_some_and(|extension| extension == "app"))
    {
        return Err("background-service; manual-action-required".into());
    }
    if protected_executable(&executable) {
        return Err("system-process; manual-action-required".into());
    }
    Ok((process.name().to_string_lossy().into_owned(), executable))
}

fn protected_executable(path: &Path) -> bool {
    let lower = path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    lower.starts_with("/system/")
        || lower.starts_with("/usr/libexec/")
        || lower.starts_with("/sbin/")
        || lower.starts_with("/usr/sbin/")
        || lower.contains("/windows/")
}
