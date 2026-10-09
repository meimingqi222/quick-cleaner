//! Bundled, reviewed uninstall entry points; this is not a shell discovery mechanism.
use crate::core::apps::{AppRegRoot, InstalledApp};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptUninstaller {
    pub bundle_id: String,
    pub script: String,
    pub sha256: Vec<String>,
    pub interpreter: Shell,
    pub arguments: Vec<String>,
    pub preserves_user_data: bool,
    pub user_bundles: Vec<String>,
    pub system_bundles: Vec<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shell {
    Sh,
    Bash,
}

impl Shell {
    #[cfg(not(windows))]
    pub fn executable(self) -> &'static str {
        match self {
            Self::Sh => "/bin/sh",
            Self::Bash => "/bin/bash",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisteredUninstallWarning {
    pub registry_id: String,
    pub name: String,
    pub publisher: String,
}

#[cfg(not(windows))]
pub fn script_for(bundle_id: &str) -> Option<ScriptUninstaller> {
    super::current()
        .definition("residual-macos")
        .script_uninstallers
        .iter()
        .find(|entry| entry.bundle_id == bundle_id)
        .cloned()
}

pub fn requires_manual_uninstall(app: &InstalledApp) -> bool {
    app.discovery.is_none()
        && !app.is_system_component
        && app.registry_root == AppRegRoot::Hkcu
        && super::current()
            .list("residual-macos", "manual_uninstall_bundle_ids")
            .contains(&app.registry_subpath)
}

pub fn deletes_user_data(app: &InstalledApp) -> bool {
    let snapshot = super::current();
    let script_deletes_data = app.discovery.is_none()
        && !app.is_system_component
        && app.registry_root == AppRegRoot::Hkcu
        && snapshot
            .definition("residual-macos")
            .script_uninstallers
            .iter()
            .any(|entry| entry.bundle_id == app.registry_subpath && !entry.preserves_user_data);
    script_deletes_data
        || snapshot
            .definition("residual-windows")
            .uninstall_data_warnings
            .iter()
            .any(|warning| {
                registration_matches(app, &warning.registry_id, &warning.name, &warning.publisher)
            })
}

pub fn registration_matches(app: &InstalledApp, id: &str, name: &str, publisher: &str) -> bool {
    app.discovery.is_none()
        && !app.is_system_component
        && matches!(
            app.registry_root,
            AppRegRoot::Hklm | AppRegRoot::Hklm32 | AppRegRoot::Hkcu
        )
        && app.id.eq_ignore_ascii_case(id)
        && app.registry_subpath.eq_ignore_ascii_case(&format!(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{id}"
        ))
        && app.publisher == publisher
        && (app.name == name
            || (!app.version.is_empty()
                && [
                    format!("{name} {}", app.version),
                    format!("{name} version {}", app.version),
                    format!("{name} 版本 {}", app.version),
                ]
                .contains(&app.name)))
}

fn token(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 256
        && !value.contains(['/', '\\'])
        && !value.chars().any(char::is_control)
}

fn relative(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value.contains(['\\', ':'])
        && !value.chars().any(char::is_control)
        && value
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

pub(super) fn validate(rule: &super::RuleDefinition) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    if rule.script_uninstallers.len() > 128 {
        return Err("Too many script uninstallers".into());
    }
    for entry in &rule.script_uninstallers {
        let bundles: Vec<_> = entry
            .user_bundles
            .iter()
            .chain(&entry.system_bundles)
            .collect();
        if rule.id != "residual-macos"
            || rule.platform != "macos"
            || !token(&entry.bundle_id)
            || !ids.insert(&entry.bundle_id)
            || !relative(&entry.script)
            || !entry.script.starts_with("Contents/")
            || entry.sha256.is_empty()
            || entry.sha256.len() > 16
            || entry.sha256.iter().any(|hash| {
                hash.len() != 64
                    || !hash
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            || entry.arguments.len() > 16
            || entry
                .arguments
                .iter()
                .any(|arg| arg.len() > 256 || arg.chars().any(char::is_control))
            || bundles.is_empty()
            || bundles.len() > 16
            || bundles.iter().any(|path| {
                !relative(path)
                    || !path.ends_with(".app")
                    || !(path.starts_with("Applications/") || path.starts_with("Library/"))
            })
            || entry.user_bundles.iter().collect::<BTreeSet<_>>().len() != entry.user_bundles.len()
            || entry.system_bundles.iter().collect::<BTreeSet<_>>().len()
                != entry.system_bundles.len()
            || rule
                .lists
                .get("manual_uninstall_bundle_ids")
                .is_some_and(|manual| manual.contains(&entry.bundle_id))
        {
            return Err("Invalid reviewed script uninstaller".into());
        }
    }
    let manual = rule.lists.get("manual_uninstall_bundle_ids");
    if manual.is_some_and(|ids| {
        rule.id != "residual-macos"
            || rule.platform != "macos"
            || ids.len() > 128
            || ids.iter().any(|id| !token(id))
    }) {
        return Err("Invalid manual uninstall identities".into());
    }
    let mut ids = BTreeSet::new();
    if rule.uninstall_data_warnings.len() > 128
        || rule.uninstall_data_warnings.iter().any(|entry| {
            rule.id != "residual-windows"
                || rule.platform != "windows"
                || !token(&entry.registry_id)
                || !token(&entry.name)
                || !token(&entry.publisher)
                || !ids.insert(&entry.registry_id)
        })
    {
        return Err("Invalid registered uninstall data warning".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app(id: &str, name: &str, publisher: &str) -> InstalledApp {
        InstalledApp {
            discovery: None,
            id: id.into(),
            name: name.into(),
            version: "1.2.3".into(),
            publisher: publisher.into(),
            last_used_date: None,
            last_used_raw: 0,
            install_date: None,
            install_date_raw: 0,
            install_location: None,
            display_icon: None,
            uninstall_string: Some("uninstall.exe".into()),
            quiet_uninstall_string: None,
            estimated_size: 0,
            registry_root: AppRegRoot::Hklm,
            registry_subpath: format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{id}"),
            is_system_component: false,
            uninstaller_missing: false,
        }
    }

    #[test]
    fn msime_manual_uninstall_matches_exact_editions() {
        for suffix in [
            "",
            ".pinyin",
            ".wubi",
            ".japanese",
            ".vietnamese",
            ".tibetan",
        ] {
            let mut settings = app("fixture", "MSIME", "fixture");
            settings.registry_root = AppRegRoot::Hkcu;
            settings.registry_subpath = format!("app.msime.macos{suffix}");
            assert!(
                requires_manual_uninstall(&settings),
                "{}",
                settings.registry_subpath
            );
            settings.registry_subpath = if suffix.is_empty() {
                "app.msime.inputmethod.MetasequoiaIME".into()
            } else {
                format!("app.msime.inputmethod{suffix}")
            };
            assert!(requires_manual_uninstall(&settings));
            settings.registry_subpath.push_str(".other");
            assert!(!requires_manual_uninstall(&settings));
        }
    }

    #[test]
    fn msime_data_warning_requires_complete_registration_identity() {
        let snapshot = super::super::current();
        let warnings = &snapshot
            .definition("residual-windows")
            .uninstall_data_warnings;
        assert_eq!(warnings.len(), 6);
        for warning in warnings {
            let original = app(&warning.registry_id, &warning.name, &warning.publisher);
            assert!(deletes_user_data(&original));
            let mut versioned = original.clone();
            versioned.name = format!("{} version {}", warning.name, versioned.version);
            assert!(deletes_user_data(&versioned));
            for field in 0..5 {
                let mut other = original.clone();
                match field {
                    0 => other.id.push_str("-other"),
                    1 => other.registry_subpath = "somewhere-else".into(),
                    2 => other.publisher = "other".into(),
                    3 => other.name.push_str("-other"),
                    _ => other.registry_root = AppRegRoot::Unregistered,
                }
                assert!(!deletes_user_data(&other), "field {field}");
            }
        }
        assert!(!deletes_user_data(&app(
            "msime-windows",
            "水杉输入法",
            "Metasequoia"
        )));
    }

    #[test]
    fn script_data_warning_follows_reviewed_arguments_policy() {
        let mut product = app("app.qingjian.inputmethod", "青简", "fixture");
        product.registry_root = AppRegRoot::Hkcu;
        product.registry_subpath = product.id.clone();
        assert!(!deletes_user_data(&product));
        let mut policy = super::super::snapshot().bundle.clone();
        policy
            .rules
            .iter_mut()
            .find(|rule| rule.id == "residual-macos")
            .unwrap()
            .script_uninstallers[0]
            .preserves_user_data = false;
        policy.validate().unwrap();
        super::super::with_snapshot(
            std::sync::Arc::new(super::super::RuleSnapshot { bundle: policy }),
            || {
                assert!(deletes_user_data(&product));
                product.registry_subpath.push_str(".other");
                assert!(!deletes_user_data(&product));
            },
        );
    }

    #[test]
    fn script_uninstall_policy_rejects_ambiguous_or_unbounded_commands() {
        let original = super::super::snapshot().bundle.clone();
        for fault in 0..12 {
            let mut policy = original.clone();
            let rule = policy
                .rules
                .iter_mut()
                .find(|rule| rule.id == "residual-macos")
                .unwrap();
            let entry = &mut rule.script_uninstallers[0];
            match fault {
                0 => entry.script = "../uninstall.sh".into(),
                1 => entry.script = "Contents/../uninstall.sh".into(),
                2 => entry.sha256.clear(),
                3 => entry.sha256 = vec!["z".repeat(64)],
                4 => entry.user_bundles = vec!["/Applications/Test.app".into()],
                5 => entry.system_bundles = vec!["Library/../Test.app".into()],
                6 => {
                    entry.user_bundles.clear();
                    entry.system_bundles.clear();
                }
                7 => entry.arguments = vec!["x".into(); 17],
                8 => {
                    let second = entry.clone();
                    rule.script_uninstallers.push(second);
                }
                9 => entry.bundle_id.clear(),
                10 => entry.script = "Resources/uninstall.sh".into(),
                _ => {
                    rule.lists
                        .get_mut("manual_uninstall_bundle_ids")
                        .unwrap()
                        .push(entry.bundle_id.clone());
                }
            }
            assert!(policy.validate().is_err(), "fault {fault}");
        }
        let mut entry = serde_json::to_value(
            &original
                .rules
                .iter()
                .find(|r| r.id == "residual-macos")
                .unwrap()
                .script_uninstallers[0],
        )
        .unwrap();
        entry["interpreter"] = "/tmp/unknown-shell".into();
        assert!(serde_json::from_value::<ScriptUninstaller>(entry).is_err());
    }
}
