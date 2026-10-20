//! Spotlight 会漏掉开发工具浏览器和内嵌应用；额外查询系统登记的实际包路径。
use objc::runtime::Object;
use objc::{class, msg_send, sel, sel_impl};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static WORKSPACE_LOCK: Mutex<()> = Mutex::new(());

pub(super) fn registered_paths(id: &str) -> Option<Vec<PathBuf>> {
    let _guard = WORKSPACE_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    unsafe {
        let pool: *mut Object = msg_send![class!(NSAutoreleasePool), new];
        let result = workspace_paths(id);
        let _: () = msg_send![pool, drain];
        result
    }
}

unsafe fn workspace_paths(id: &str) -> Option<Vec<PathBuf>> {
    let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
    if workspace.is_null() {
        return None;
    }
    let key: *mut Object = msg_send![class!(NSString), alloc];
    let key: *mut Object = msg_send![key, initWithBytes: id.as_ptr().cast::<std::ffi::c_void>() length: id.len() encoding: 4usize];
    if key.is_null() {
        return None;
    }
    let urls: *mut Object = msg_send![workspace, URLsForApplicationsWithBundleIdentifier: key];
    let _: () = msg_send![key, release];
    if urls.is_null() {
        return None;
    }
    let count: usize = msg_send![urls, count];
    let mut paths = Vec::with_capacity(count);
    for index in 0..count {
        let url: *mut Object = msg_send![urls, objectAtIndex: index];
        let path: *mut Object = msg_send![url, path];
        let raw: *const std::ffi::c_char = msg_send![path, UTF8String];
        if raw.is_null() {
            return None;
        }
        paths.push(PathBuf::from(std::ffi::CStr::from_ptr(raw).to_str().ok()?));
    }
    Some(paths)
}

pub(super) fn supplement(
    owners: impl Iterator<Item = String>,
    installed: &mut HashSet<String>,
) -> Option<HashSet<String>> {
    let run = crate::core::proc::run_with_timeout(
        "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister",
        &["-dump"],
        std::time::Duration::from_secs(15),
    )?;
    if !run.ok || !run.stderr.iter().all(u8::is_ascii_whitespace) {
        return None;
    }
    let records = registration_records(std::str::from_utf8(&run.stdout).ok()?)?;
    let trash_root = super::super::user_env::user_home()?.join(".Trash");
    supplement_live_ids(owners, installed, &trash_root, |id| {
        let mut paths = registered_paths(id)?;
        paths.extend(records.get(id).into_iter().flatten().cloned());
        paths.sort();
        paths.dedup();
        Some(paths)
    })
}

fn registration_records(text: &str) -> Option<HashMap<String, Vec<PathBuf>>> {
    let mut records: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for block in text
        .split("--------------------------------------------------------------------------------")
    {
        if !block.trim_start().starts_with("bundle id:") {
            continue;
        }
        let field = |key: &str| {
            block
                .lines()
                .find_map(|line| line.strip_prefix(key).map(str::trim))
        };
        let Some(id) = field("identifier:") else {
            continue;
        };
        if !crate::core::macos_text::valid_bundle_id(id) {
            continue;
        }
        let (path, object) = field("path:")?.rsplit_once(" (0x")?;
        let object = object.strip_suffix(')')?;
        if object.is_empty()
            || !object.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !Path::new(path).is_absolute()
        {
            return None;
        }
        records
            .entry(id.to_ascii_lowercase())
            .or_default()
            .push(PathBuf::from(path));
    }
    (!records.is_empty()).then_some(records)
}

pub(super) fn supplement_live_ids(
    owners: impl Iterator<Item = String>,
    installed: &mut HashSet<String>,
    trash_root: &Path,
    mut lookup: impl FnMut(&str) -> Option<Vec<PathBuf>>,
) -> Option<HashSet<String>> {
    let trash_root = trash_root
        .canonicalize()
        .unwrap_or_else(|_| trash_root.to_path_buf());
    let mut removed = HashSet::new();
    let mut queries = HashSet::new();
    for owner in owners {
        let parts: Vec<_> = owner.split('.').collect();
        for depth in 2..=parts.len() {
            queries.insert(parts[..depth].join("."));
        }
    }
    for id in queries {
        for path in lookup(&id)? {
            // 登记可能指向已删路径或废纸篓，不能据此认定仍在安装。
            match std::fs::metadata(path.join("Contents/Info.plist")) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return None,
                Ok(meta) if !meta.is_file() => return None,
                Ok(_) => {}
            }
            let actual = super::super::apps::plist_string(
                &path.join("Contents/Info.plist"),
                "CFBundleIdentifier",
            )?;
            if !actual.eq_ignore_ascii_case(&id) {
                return None;
            }
            let resolved = path.canonicalize().ok()?;
            if resolved.starts_with(&trash_root) {
                removed.insert(actual.to_ascii_lowercase());
            } else {
                installed.insert(actual.to_ascii_lowercase());
            }
        }
    }
    Some(removed)
}

/// 只有实际应用包里的精确身份能提升归属置信度；不使用目录显示名猜测。
pub(super) fn trashed_bundle_ids(home: &Path) -> HashSet<String> {
    let mut ids = HashSet::new();
    if let Ok(entries) = std::fs::read_dir(home.join(".Trash")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !entry.file_type().is_ok_and(|kind| kind.is_dir())
                || path.extension().is_none_or(|ext| ext != "app")
            {
                continue;
            }
            if let Some(id) = super::super::apps::plist_string(
                &path.join("Contents/Info.plist"),
                "CFBundleIdentifier",
            ) {
                if crate::core::macos_text::valid_bundle_id(&id) {
                    ids.insert(id.to_ascii_lowercase());
                }
            }
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_records_keep_exact_paths_and_reject_unknown_output() {
        let text = "bundle id: Widget (0x1)\npath: /tmp/A (test).app (0x2)\nidentifier: com.vendor.Widget\n--------------------------------------------------------------------------------\nbundle id: Ghostty (0x3)\npath: /Users/test/.Trash/Ghostty.app (0x4)\nidentifier: com.mitchellh.ghostty\n";
        let records = registration_records(text).unwrap();
        assert_eq!(
            records["com.vendor.widget"],
            vec![PathBuf::from("/tmp/A (test).app")]
        );
        assert_eq!(
            records["com.mitchellh.ghostty"],
            vec![PathBuf::from("/Users/test/.Trash/Ghostty.app")]
        );
        assert!(registration_records("unknown output").is_none());
        assert!(registration_records(&format!("{text}--------------------------------------------------------------------------------\nbundle id: Broken\nidentifier: com.vendor.broken\npath: malformed")).is_none());
    }

    #[test]
    fn registered_live_bundle_blocks_orphans_and_unknown_refuses() {
        let root = crate::core::testing::fixture("qc_registered_orphan");
        let app = root.join("hidden/Playwright.app");
        std::fs::create_dir_all(app.join("Contents")).unwrap();
        std::fs::write(app.join("Contents/Info.plist"), r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>org.webkit.Playwright</string></dict></plist>"#).unwrap();
        let owners = || ["org.webkit.playwright".into()].into_iter();
        let mut installed = HashSet::new();
        assert!(
            supplement_live_ids(owners(), &mut installed, &root.join(".Trash"), |id| Some(
                if id == "org.webkit.playwright" {
                    vec![app.clone()]
                } else {
                    vec![]
                }
            ))
            .is_some()
        );
        assert!(installed.contains("org.webkit.playwright"));
        assert!(
            supplement_live_ids(owners(), &mut installed, &root.join(".Trash"), |_| None).is_none()
        );
        assert!(
            supplement_live_ids(owners(), &mut installed, &root.join(".Trash"), |_| Some(
                vec![root.join("gone.app")]
            ))
            .is_some()
        );
        let trash_app = root.join(".Trash/Playwright.app");
        std::fs::create_dir_all(trash_app.join("Contents")).unwrap();
        std::fs::copy(
            app.join("Contents/Info.plist"),
            trash_app.join("Contents/Info.plist"),
        )
        .unwrap();
        let mut live = HashSet::new();
        let removed = supplement_live_ids(owners(), &mut live, &root.join(".Trash"), |id| {
            Some(if id == "org.webkit.playwright" {
                vec![trash_app.clone()]
            } else {
                vec![]
            })
        })
        .unwrap();
        assert!(removed.contains("org.webkit.playwright"));
        assert!(live.is_empty());
        assert!(
            supplement_live_ids(owners(), &mut live, &root.join(".Trash"), |_| Some(vec![
                app.clone()
            ]))
            .is_none(),
            "mismatched registered identity must refuse"
        );
        let mut stale = HashSet::new();
        let removed = supplement_live_ids(owners(), &mut stale, &root.join(".Trash"), |_| {
            Some(vec![root.join("gone.app")])
        })
        .unwrap();
        assert!(
            stale.is_empty() && removed.is_empty(),
            "stale registrations prove neither live nor removed identity"
        );
        let alias = root.join(".Trash/Alias.app");
        std::os::unix::fs::symlink(&app, &alias).unwrap();
        let mut aliased_live = HashSet::new();
        let removed =
            supplement_live_ids(owners(), &mut aliased_live, &root.join(".Trash"), |id| {
                Some(if id == "org.webkit.playwright" {
                    vec![alias.clone()]
                } else {
                    vec![]
                })
            })
            .unwrap();
        assert!(
            removed.is_empty(),
            "a trash symlink to a live app is not a removed identity"
        );
        assert!(aliased_live.contains("org.webkit.playwright"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
