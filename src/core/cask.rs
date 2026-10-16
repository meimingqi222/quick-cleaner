//! Installed Homebrew metadata describes the whole cask, not just its selected app.
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

pub(crate) struct InstalledScope {
    pub apps: Vec<PathBuf>,
    pub app_only: bool,
}

impl InstalledScope {
    pub fn parse(
        receipt: &Value,
        definition: &Value,
        config: &Value,
        home: &Path,
        bundle_id: &str,
    ) -> Result<Self, String> {
        let artifacts = definition
            .get("artifacts")
            .or_else(|| receipt.get("uninstall_artifacts"))
            .and_then(Value::as_array)
            .ok_or("Homebrew installed artifact scope unavailable")?;
        let appdir = ["explicit", "env", "default"]
            .into_iter()
            .find_map(|key| config.get(key).and_then(|v| v.get("appdir")))
            .and_then(Value::as_str)
            .ok_or("Homebrew installed appdir unavailable")?;
        let appdir = expand(appdir, home)?;
        let mut scope = Self {
            apps: Vec::new(),
            app_only: receipt.get("uninstall_flight_blocks") == Some(&Value::Bool(false))
                && definition
                    .get("variations")
                    .is_none_or(|v| v.as_object().is_some_and(serde_json::Map::is_empty)),
        };
        for artifact in artifacts {
            let row = artifact.as_object().ok_or("Invalid Homebrew artifact")?;
            if let Some(app) = row.get("app") {
                if row.keys().any(|key| key != "app" && key != "target") {
                    scope.app_only = false;
                }
                let args = app.as_array().ok_or("Invalid Homebrew app artifact")?;
                let source = args
                    .first()
                    .and_then(Value::as_str)
                    .ok_or("Missing Homebrew app source")?;
                if source.is_empty()
                    || source.contains('\\')
                    || source.chars().any(char::is_control)
                    || !Path::new(source)
                        .components()
                        .all(|c| matches!(c, Component::Normal(_)))
                {
                    return Err("Unconfined Homebrew app source".into());
                }
                let basename = Path::new(source)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .ok_or("Invalid Homebrew app source")?;
                let target = match args.as_slice() {
                    [_] => basename,
                    [_, options] => {
                        let options = options.as_object().ok_or("Invalid Homebrew app options")?;
                        if options.keys().any(|key| key != "target") {
                            scope.app_only = false;
                        }
                        options
                            .get("target")
                            .and_then(Value::as_str)
                            .ok_or("Missing Homebrew app target")?
                    }
                    _ => return Err("Invalid Homebrew app options".into()),
                };
                let path = if target.starts_with('/') || target.starts_with("~/") {
                    expand(target, home)?
                } else {
                    if target.is_empty()
                        || target.chars().any(char::is_control)
                        || !Path::new(target)
                            .components()
                            .all(|c| matches!(c, Component::Normal(_)))
                    {
                        return Err("Unconfined Homebrew app target".into());
                    }
                    appdir.join(target)
                };
                if path.extension().is_none_or(|ext| ext != "app")
                    || row
                        .get("target")
                        .is_some_and(|value| value.as_str().map(Path::new) != Some(path.as_path()))
                {
                    return Err("Inconsistent Homebrew app target".into());
                }
                scope.apps.push(path);
            } else if row.len() == 1 && row.contains_key("zap") {
                // Zap is never invoked; its presence is not ordinary uninstall authority.
            } else if row.len() == 1 && row.contains_key("uninstall") {
                let safe = row["uninstall"].as_array().is_some_and(|args| {
                    !args.is_empty()
                        && args.iter().all(|arg| {
                            arg.as_object().is_some_and(|options| {
                                options.len() == 1
                                    && options.get("quit").is_some_and(|quit| {
                                        let own_id = |value: &Value| {
                                            value.as_str() == Some(bundle_id)
                                                && !bundle_id.is_empty()
                                        };
                                        own_id(quit)
                                            || quit.as_array().is_some_and(|ids| {
                                                !ids.is_empty() && ids.iter().all(own_id)
                                            })
                                    })
                            })
                        })
                });
                scope.app_only &= safe;
            } else {
                scope.app_only = false;
            }
        }
        scope.app_only &= scope.apps.len() == 1;
        Ok(scope)
    }

    pub fn authorizes(&self, selected: &Path) -> bool {
        self.app_only && self.apps.as_slice() == [selected]
    }
}

fn expand(value: &str, home: &Path) -> Result<PathBuf, String> {
    let path = if let Some(relative) = value.strip_prefix("~/") {
        home.join(relative)
    } else {
        PathBuf::from(value)
    };
    // These are macOS paths even when policy tests run on Windows.
    if !path.to_string_lossy().starts_with('/')
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        || value.chars().any(char::is_control)
        || value.contains('\\')
    {
        return Err("Unverifiable Homebrew appdir".into());
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn installed_cask_scope_uses_persisted_appdir_and_exact_target() {
        let home = Path::new("/test-home");
        let receipt = json!({"uninstall_flight_blocks": false, "uninstall_artifacts": [
            {"app": ["subdir/Original.app", {"target": "Renamed.app"}]},
            {"uninstall": [{"quit": "com.example.main"}]},
            {"zap": [{"trash": "~/Documents/*"}]}
        ]});
        let config = json!({"default": {"appdir": "/Applications"}, "env": {"appdir": "/Other"}, "explicit": {"appdir": "~/Applications"}});
        let scope =
            InstalledScope::parse(&receipt, &json!({}), &config, home, "com.example.main").unwrap();
        assert!(scope.authorizes(&home.join("Applications/Renamed.app")));
        assert!(!scope.authorizes(Path::new("/Applications/Renamed.app")));
        assert!(!scope.authorizes(&home.join("Applications/Original.app")));
        let absolute = json!({"artifacts": [{"app": ["Original.app", {"target": "/Applications/Absolute.app"}]}]});
        let scope =
            InstalledScope::parse(&receipt, &absolute, &config, home, "com.example.main").unwrap();
        assert!(scope.authorizes(Path::new("/Applications/Absolute.app")));
    }

    #[test]
    fn installed_cask_scope_refuses_companions_scripts_and_unknown_scope() {
        let home = Path::new("/test-home");
        let config = json!({"default": {"appdir": "/Applications"}});
        for extra in [
            json!({"app": ["Companion.app"]}),
            json!({"binary": ["tool"]}),
            json!({"uninstall": [{"delete": "~/Documents"}]}),
            json!({"uninstall": [{"script": "uninstall.sh"}]}),
            json!({"uninstall": [{"pkgutil": "com.example.pkg"}]}),
            json!({"uninstall": [{"launchctl": "com.example.service"}]}),
            json!({"uninstall": [{"quit": "com.example.other"}]}),
        ] {
            let receipt = json!({"uninstall_flight_blocks": false, "uninstall_artifacts": [{"app": ["Main.app"]}, extra]});
            let scope =
                InstalledScope::parse(&receipt, &json!({}), &config, home, "com.example.main")
                    .unwrap();
            assert!(
                !scope.authorizes(Path::new("/Applications/Main.app")),
                "{receipt}"
            );
        }
        let receipt = json!({"uninstall_flight_blocks": true, "uninstall_artifacts": [{"app": ["Main.app"]}]});
        assert!(
            !InstalledScope::parse(&receipt, &json!({}), &config, home, "com.example.main")
                .unwrap()
                .authorizes(Path::new("/Applications/Main.app"))
        );
        // An explicit empty definition must not recover a different receipt scope.
        let scope = InstalledScope::parse(
            &receipt,
            &json!({"artifacts": []}),
            &config,
            home,
            "com.example.main",
        )
        .unwrap();
        assert!(scope.apps.is_empty());
        let receipt = json!({"uninstall_flight_blocks": false, "uninstall_artifacts": [{"app": ["Main.app"]}]});
        let varied = json!({"variations": {"arm64_sonoma": {"artifacts": [{"uninstall": [{"script": "remove.sh"}]}]}}});
        assert!(
            !InstalledScope::parse(&receipt, &varied, &config, home, "com.example.main")
                .unwrap()
                .authorizes(Path::new("/Applications/Main.app"))
        );
        assert!(
            InstalledScope::parse(&json!({}), &json!({}), &config, home, "com.example.main")
                .is_err()
        );
        for app in [
            json!(["../Other.app"]),
            json!(["Original.app", {"target": "../Outside.app"}]),
        ] {
            let receipt =
                json!({"uninstall_flight_blocks": false, "uninstall_artifacts": [{"app": app}]});
            assert!(
                InstalledScope::parse(&receipt, &json!({}), &config, home, "com.example.main")
                    .is_err()
            );
        }
    }
}
