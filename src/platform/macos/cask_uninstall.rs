//! Receipt-bound, single-application Homebrew removal. Unknown scope never grants Trash fallback.
use crate::core::apps::InstalledApp;
use crate::core::cask::InstalledScope;
use crate::core::model::capture_identity;
use crate::core::rules::facts::{confined_file, confined_path, read};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

struct Installation {
    prefix: PathBuf,
    token: String,
    files: Vec<(PathBuf, Vec<u8>)>,
    receipt: PathBuf,
    definition: PathBuf,
    scope: InstalledScope,
    supported: bool,
}

pub(super) fn run(app: &InstalledApp) -> Result<bool, String> {
    let home = super::super::user_env::user_home().ok_or("User home unavailable")?;
    let selected = app
        .install_location
        .as_deref()
        .ok_or("Application path unavailable")?;
    let mut matched = None;
    for prefix in ["/opt/homebrew", "/usr/local"] {
        for install in inventory(Path::new(prefix), &home, &app.registry_subpath)? {
            if !install.scope.apps.iter().any(|path| path == selected) {
                continue;
            }
            if matched.is_some() {
                return Err("Multiple Homebrew installations claim this application; use Homebrew to uninstall".into());
            }
            matched = Some(install);
        }
    }
    let Some(install) = matched else {
        return Ok(false);
    };
    if !install.supported || !install.scope.authorizes(selected) {
        return Err("Homebrew removal includes scripts, services, other apps or unknown scope; use its official uninstall flow. No fallback deletion performed.".into());
    }
    if !confined_path(selected, selected)?
        || super::read_info_plist(&selected.join("Contents/Info.plist"))
            .0
            .as_deref()
            != Some(&app.registry_subpath)
    {
        return Err("Application identity changed before Homebrew uninstall".into());
    }
    let identity = capture_identity(selected).ok_or("Application identity unavailable")?;
    let brew = std::fs::canonicalize(install.prefix.join("bin/brew")).map_err(|e| e.to_string())?;
    if !confined_file(&install.prefix, &brew)? {
        return Err("Homebrew executable is outside its installation".into());
    }
    let executable_identity =
        capture_identity(&brew).ok_or("Homebrew executable identity unavailable")?;
    let current = inventory(&install.prefix, &home, &app.registry_subpath)?;
    let same = current
        .iter()
        .find(|record| record.token == install.token)
        .is_some_and(|record| {
            record.files == install.files && record.definition == install.definition
        });
    if !same || !identity.recheck(selected) || !executable_identity.recheck(&brew) {
        return Err("Homebrew installation changed; retry after refreshing".into());
    }
    let mut home_env = std::ffi::OsString::from("HOME=");
    home_env.push(home);
    let args = vec![
        "-i".into(),
        "PATH=/usr/bin:/bin:/usr/sbin:/sbin".into(),
        home_env,
        "HOMEBREW_NO_AUTO_UPDATE=1".into(),
        "HOMEBREW_NO_AUTOREMOVE=1".into(),
        "HOMEBREW_NO_INSTALL_FROM_API=1".into(),
        "HOMEBREW_NO_ANALYTICS=1".into(),
        brew.into_os_string(),
        "uninstall".into(),
        "--cask".into(),
        "--".into(),
        install.token.into(),
    ];
    let result =
        crate::core::proc::run_with_timeout("/usr/bin/env", &args, Duration::from_secs(30 * 60))
            .ok_or("Homebrew uninstall unavailable or timed out; no fallback deletion performed")?;
    if !result.ok {
        return Err(format!(
            "Homebrew uninstall failed: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    if ![selected, &install.receipt, &install.definition]
        .into_iter()
        .all(absent)
    {
        return Err(
            "Homebrew exited, but application or installation records remain or cannot be verified"
                .into(),
        );
    }
    Ok(true)
}

fn absent(path: &Path) -> bool {
    matches!(std::fs::symlink_metadata(path), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
}

fn children(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(root).map_err(|e| e.to_string())? {
        if paths.len() >= 4096 {
            return Err("Homebrew inventory limit exceeded".into());
        }
        paths.push(entry.map_err(|e| e.to_string())?.path());
    }
    Ok(paths)
}

fn json_file(
    root: &Path,
    path: &Path,
    files: &mut Vec<(PathBuf, Vec<u8>)>,
) -> Result<Value, String> {
    if !confined_file(root, path)? {
        return Err(format!("Homebrew evidence missing: {}", path.display()));
    }
    let bytes = read(path)?;
    let value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    files.push((path.to_owned(), bytes));
    Ok(value)
}

fn inventory(prefix: &Path, home: &Path, id: &str) -> Result<Vec<Installation>, String> {
    let root = prefix.join("Caskroom");
    if absent(&root) {
        return Ok(Vec::new());
    }
    if !confined_path(prefix, &root)? {
        return Err("Homebrew Caskroom unavailable".into());
    }
    let mut installs = Vec::new();
    for directory in children(&root)? {
        // Homebrew leaves old-token aliases; only the physical installation owns records.
        let metadata = std::fs::symlink_metadata(&directory).map_err(|e| e.to_string())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            continue;
        }
        if !confined_path(&root, &directory)? {
            return Err("Unverifiable Homebrew installation".into());
        }
        let token = directory
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("Invalid Homebrew token")?;
        if token.is_empty()
            || token.starts_with('-')
            || !token
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"-+@._".contains(&c))
        {
            return Err("Invalid Homebrew installation identity".into());
        }
        let metadata = directory.join(".metadata");
        if absent(&metadata) {
            continue;
        }
        let receipt_path = metadata.join("INSTALL_RECEIPT.json");
        let mut files = Vec::new();
        let receipt = json_file(&root, &receipt_path, &mut files)?;
        let config = json_file(&root, &metadata.join("config.json"), &mut files)?;
        let mut candidates = Vec::new();
        for version in children(&metadata)? {
            if !std::fs::symlink_metadata(&version)
                .map_err(|e| e.to_string())?
                .is_dir()
            {
                continue;
            }
            if !confined_path(&root, &version)? {
                return Err("Redirected Homebrew metadata".into());
            }
            for timestamp in children(&version)? {
                if !confined_path(&root, &timestamp)? {
                    return Err("Redirected Homebrew metadata".into());
                }
                candidates.push(timestamp);
            }
        }
        let latest = candidates
            .into_iter()
            .max_by(|a, b| a.file_name().cmp(&b.file_name()))
            .ok_or("Installed Homebrew definition unavailable")?;
        let mut definition = None;
        for extension in ["json", "internal.json", "rb"] {
            let path = latest.join("Casks").join(format!("{token}.{extension}"));
            if !absent(&path) {
                definition = Some(path);
                break;
            }
        }
        let path = definition.ok_or("Installed Homebrew definition unavailable")?;
        let is_json = path.extension().is_some_and(|ext| ext == "json");
        let definition = if is_json {
            json_file(&root, &path, &mut files)?
        } else {
            if !confined_file(&root, &path)? {
                return Err("Homebrew definition unavailable".into());
            }
            files.push((path.clone(), read(&path)?));
            serde_json::json!({})
        };
        let supported = is_json
            && receipt.pointer("/source/tap").and_then(Value::as_str) == Some("homebrew/cask")
            && receipt
                .pointer("/source/version")
                .and_then(Value::as_str)
                .map(std::ffi::OsStr::new)
                == latest.parent().and_then(Path::file_name)
            && definition
                .get("token")
                .is_none_or(|v| v.as_str() == Some(token))
            && definition
                .get("old_tokens")
                .is_none_or(|v| v.as_array().is_some_and(Vec::is_empty));
        let scope = InstalledScope::parse(&receipt, &definition, &config, home, id)?;
        installs.push(Installation {
            prefix: prefix.to_owned(),
            token: token.into(),
            files,
            receipt: receipt_path,
            definition: path,
            scope,
            supported,
        });
    }
    Ok(installs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cask_inventory_uses_installed_scope_and_rejects_corrupt_or_redirected_records() {
        let root = crate::core::testing::fixture("cask_installed_scope");
        let prefix = root.join("brew");
        let home = root.join("home");
        let metadata = prefix.join("Caskroom/renamed/.metadata");
        let definitions = metadata.join("1.2/20261016090000/Casks");
        std::fs::create_dir_all(&definitions).unwrap();
        std::fs::write(prefix.join("Caskroom/.DS_Store"), b"unrelated").unwrap();
        let receipt = json!({
            "source": {"tap": "homebrew/cask", "version": "1.2"},
            "uninstall_flight_blocks": false,
            "uninstall_artifacts": [{"app": ["Original.app", {"target": "Selected.app"}]}, {"zap": [{"trash": "~/Documents"}]}]
        });
        let receipt_path = metadata.join("INSTALL_RECEIPT.json");
        let config_path = metadata.join("config.json");
        std::fs::write(&receipt_path, receipt.to_string()).unwrap();
        std::fs::write(&config_path, json!({"default": {"appdir": "/Applications"}, "explicit": {"appdir": home.join("Applications")}}).to_string()).unwrap();
        std::fs::write(definitions.join("renamed.json"), b"{}").unwrap();
        let records = inventory(&prefix, &home, "com.example.main").unwrap();
        assert_eq!(records.len(), 1);
        assert!(records[0].supported);
        assert!(records[0]
            .scope
            .authorizes(&home.join("Applications/Selected.app")));
        assert!(!records[0]
            .scope
            .authorizes(Path::new("/Applications/Selected.app")));
        let newer = metadata.join("1.2/20261016100000/Casks");
        std::fs::create_dir_all(&newer).unwrap();
        std::fs::write(
            newer.join("renamed.json"),
            json!({"artifacts": [{"app": ["Selected.app"]}, {"app": ["Companion.app"]}]})
                .to_string(),
        )
        .unwrap();
        let records = inventory(&prefix, &home, "com.example.main").unwrap();
        assert!(!records[0]
            .scope
            .authorizes(&home.join("Applications/Selected.app")));
        assert_eq!(records[0].definition, newer.join("renamed.json"));
        std::fs::write(&receipt_path, b"{").unwrap();
        assert!(inventory(&prefix, &home, "com.example.main").is_err());
        std::fs::write(&receipt_path, receipt.to_string()).unwrap();
        let external = root.join("external-config.json");
        std::fs::rename(&config_path, &external).unwrap();
        std::os::unix::fs::symlink(&external, &config_path).unwrap();
        assert!(inventory(&prefix, &home, "com.example.main").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
