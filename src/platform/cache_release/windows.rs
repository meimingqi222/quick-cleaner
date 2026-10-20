use super::process_snapshot;
use crate::core::cache_release::Occupant;
use std::{collections::BTreeMap, os::windows::ffi::OsStrExt, path::Path};
use winapi::{
    shared::{
        minwindef::{BOOL, DWORD, FILETIME, LPARAM},
        windef::HWND,
    },
    um::{
        handleapi::CloseHandle,
        processthreadsapi::{GetExitCodeProcess, GetProcessTimes, OpenProcess, TerminateProcess},
        restartmanager::{
            RmCritical, RmEndSession, RmExplorer, RmGetList, RmRegisterResources, RmService,
            RmStartSession, CCH_RM_SESSION_KEY, RM_PROCESS_INFO,
        },
        winnt::{HANDLE, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE},
        winuser::{EnumWindows, GetWindowThreadProcessId, PostMessageW, WM_CLOSE},
    },
};

struct Session(DWORD);
impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            RmEndSession(self.0);
        }
    }
}
fn cookie(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

pub fn occupants(path: &Path) -> Result<Vec<Occupant>, String> {
    let mut session = 0;
    let mut key = [0u16; CCH_RM_SESSION_KEY + 1];
    let code = unsafe { RmStartSession(&mut session, 0, key.as_mut_ptr()) };
    if code != 0 {
        return Err(format!("RmStartSession={code}"));
    }
    let session = Session(session);
    let mut files = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    for (count, entry) in walkdir::WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .enumerate()
    {
        if count >= 20_000 || std::time::Instant::now() >= deadline {
            return Err("cache-owner-probe-budget-exceeded".into());
        }
        let entry = entry.map_err(|error| error.to_string())?;
        if entry.file_type().is_symlink() {
            return Err("cache-owner-probe-link; manual-action-required".into());
        }
        if entry.file_type().is_file() {
            files.push(
                entry
                    .path()
                    .as_os_str()
                    .encode_wide()
                    .chain(Some(0))
                    .collect::<Vec<_>>(),
            );
        }
    }
    for batch in files.chunks(256) {
        let mut pointers: Vec<_> = batch.iter().map(|file| file.as_ptr()).collect();
        let code = unsafe {
            RmRegisterResources(
                session.0,
                pointers.len() as u32,
                pointers.as_mut_ptr(),
                0,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
            )
        };
        if code != 0 {
            return Err(format!("RmRegisterResources={code}"));
        }
    }
    let mut needed = 0;
    let mut count = 0;
    let mut reboot = 0;
    let code = unsafe {
        RmGetList(
            session.0,
            &mut needed,
            &mut count,
            std::ptr::null_mut(),
            &mut reboot,
        )
    };
    if code == 0 {
        return Ok(Vec::new());
    }
    if code != 234 || needed > 128 {
        return Err(format!("RmGetList={code}; needed={needed}"));
    }
    let mut infos: Vec<RM_PROCESS_INFO> = vec![unsafe { std::mem::zeroed() }; needed as usize];
    count = needed;
    let code = unsafe {
        RmGetList(
            session.0,
            &mut needed,
            &mut count,
            infos.as_mut_ptr(),
            &mut reboot,
        )
    };
    if code != 0 {
        return Err(format!("RmGetList={code}"));
    }
    let mut owners = BTreeMap::new();
    for info in infos.into_iter().take(count as usize) {
        if matches!(info.ApplicationType, RmService | RmCritical | RmExplorer) {
            return Err("service-or-critical-owner; manual-action-required".into());
        }
        let pid = info.Process.dwProcessId;
        let identity = cookie(info.Process.ProcessStartTime);
        let (name, executable) = process_snapshot(pid)?;
        let owner = Occupant {
            pid,
            identity,
            executable,
            name,
            evidence: path.to_path_buf(),
        };
        let handle = open_verified(&owner, false)?;
        unsafe {
            CloseHandle(handle);
        }
        owners.insert(pid, owner);
    }
    Ok(owners.into_values().collect())
}

fn open_verified(owner: &Occupant, force: bool) -> Result<HANDLE, String> {
    let access = PROCESS_QUERY_LIMITED_INFORMATION | if force { PROCESS_TERMINATE } else { 0 };
    let handle = unsafe { OpenProcess(access, 0, owner.pid) };
    if handle.is_null() {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let ok = unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) };
    if ok == 0 || cookie(created) != owner.identity {
        unsafe {
            CloseHandle(handle);
        }
        return Err("owner-identity-changed".into());
    }
    Ok(handle)
}

struct CloseRequest {
    pid: u32,
    handle: HANDLE,
}
unsafe extern "system" fn close_window(window: HWND, context: LPARAM) -> BOOL {
    let request = &*(context as *const CloseRequest);
    let mut pid = 0;
    GetWindowThreadProcessId(window, &mut pid);
    let mut status = 0;
    if pid == request.pid && GetExitCodeProcess(request.handle, &mut status) != 0 && status == 259 {
        PostMessageW(window, WM_CLOSE, 0, 0);
    }
    1
}

pub fn request_exit(owner: &Occupant, force: bool) -> Result<(), String> {
    let (_, executable) = process_snapshot(owner.pid)?;
    if executable != owner.executable {
        return Err("owner-executable-changed".into());
    }
    let handle = open_verified(owner, force)?;
    let result = if force {
        if unsafe { TerminateProcess(handle, 1) } == 0 {
            Err(std::io::Error::last_os_error().to_string())
        } else {
            Ok(())
        }
    } else {
        let request = CloseRequest {
            pid: owner.pid,
            handle,
        };
        unsafe {
            EnumWindows(
                Some(close_window),
                &request as *const CloseRequest as LPARAM,
            );
        }
        Ok(())
    };
    unsafe {
        CloseHandle(handle);
    }
    result
}
