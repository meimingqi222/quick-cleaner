use super::process_snapshot;
use crate::core::{cache_release::Occupant, proc::run_with_timeout};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

pub fn occupants(path: &Path) -> Result<Vec<Occupant>, String> {
    let path = path.canonicalize().map_err(|error| error.to_string())?;
    let args = [
        std::ffi::OsStr::new("-F0pn"),
        std::ffi::OsStr::new("+D"),
        path.as_os_str(),
    ];
    let run = run_with_timeout("/usr/sbin/lsof", &args, Duration::from_secs(30))
        .ok_or("cache-owner-probe-timeout")?;
    if !matches!(run.exit_code, Some(0 | 1))
        || !run.stderr.iter().all(u8::is_ascii_whitespace)
        || (!run.stdout.is_empty() && !run.stdout.ends_with(b"\0\n"))
    {
        return Err("cache-owner-probe-incomplete".into());
    }
    let records = parse_owners(&run.stdout, &path)?;
    let mut owners = Vec::new();
    for (pid, evidence) in records {
        let identity = crate::platform::process_unique_id(pid).ok_or("owner-identity-unknown")?;
        let (name, executable) = process_snapshot(pid)?;
        if crate::platform::process_unique_id(pid) != Some(identity) {
            return Err("owner-identity-changed".into());
        }
        owners.push(Occupant {
            pid,
            identity,
            executable,
            name,
            evidence,
        });
    }
    Ok(owners)
}

fn parse_owners(bytes: &[u8], target: &Path) -> Result<BTreeMap<u32, PathBuf>, String> {
    let mut pid = None;
    let mut owners = BTreeMap::new();
    for field in bytes.split(|&byte| byte == 0) {
        let field = field.strip_prefix(b"\n").unwrap_or(field);
        match field.first() {
            Some(b'p') => {
                pid = Some(
                    std::str::from_utf8(&field[1..])
                        .ok()
                        .and_then(|value| value.parse::<u32>().ok())
                        .ok_or("invalid-owner-pid")?,
                )
            }
            Some(b'n') => {
                let path = PathBuf::from(
                    std::str::from_utf8(&field[1..]).map_err(|_| "invalid-owner-path")?,
                );
                if path == target || path.starts_with(target) {
                    owners
                        .entry(pid.ok_or("missing-owner-pid")?)
                        .or_insert(path);
                }
            }
            _ => {}
        }
    }
    Ok(owners)
}

pub fn request_exit(owner: &Occupant, force: bool) -> Result<(), String> {
    let (_, executable) = process_snapshot(owner.pid)?;
    if executable != owner.executable
        || crate::platform::process_unique_id(owner.pid) != Some(owner.identity)
    {
        return Err("owner-identity-changed".into());
    }
    if force {
        return crate::platform::force_terminate_process(owner.pid, 0, Some(owner.identity));
    }
    use objc::{class, msg_send, sel, sel_impl};
    unsafe {
        let app: *mut objc::runtime::Object = msg_send![class!(NSRunningApplication), runningApplicationWithProcessIdentifier: owner.pid as i32];
        // A CLI has no NSRunningApplication. Never silently turn normal Quit into SIGTERM/SIGKILL.
        if app.is_null() {
            return Ok(());
        }
        let _: objc::runtime::BOOL = msg_send![app, terminate];
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lsof_evidence_retains_every_owner_and_respects_path_boundaries() {
        let data =
            b"p40\0\nn/cache/a\0\nn/cache/b\0\np41\0\nn/cache/c\0\np42\0\nn/cache-other/x\0\n";
        let owners = parse_owners(data, Path::new("/cache")).unwrap();
        assert_eq!(owners.len(), 2);
        assert_eq!(owners.get(&40), Some(&PathBuf::from("/cache/a")));
        assert!(parse_owners(b"n/cache/a\0\n", Path::new("/cache")).is_err());
    }
}
