//! Executes reviewed self-contained shell uninstallers from bundled policy.
use crate::core::apps::InstalledApp;
use crate::core::rules::uninstall::ScriptUninstaller;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

const ELEVATED_SCRIPT: &str = r#"on run argv
    set userHome to item 1 of argv as text
    set interpreter to item 2 of argv as text
    set scriptBody to item 3 of argv as text
    set cmd to "/usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin HOME=" & quoted form of userHome & " " & quoted form of interpreter & " -c " & quoted form of scriptBody & " uninstall"
    repeat with i from 4 to count of argv
        set cmd to cmd & " " & quoted form of (item i of argv as text)
    end repeat
    do shell script cmd with administrator privileges
end run"#;

pub(super) fn uninstaller(bundle: &Path, spec: &ScriptUninstaller) -> Option<PathBuf> {
    if super::read_info_plist(&bundle.join("Contents/Info.plist"))
        .0
        .as_deref()
        != Some(&spec.bundle_id)
    {
        return None;
    }
    let script = bundle.join(&spec.script);
    crate::core::rules::facts::confined_file(bundle, &script)
        .ok()
        .filter(|valid| *valid)
        .map(|_| script)
}

pub(super) fn run(app: &InstalledApp) -> Option<Result<(), String>> {
    let spec = crate::core::rules::uninstall::script_for(&app.registry_subpath)?;
    Some((|| {
        let home = super::super::user_env::user_home().ok_or("无法确认用户目录")?;
        let locations: Vec<_> = spec
            .system_bundles
            .iter()
            .map(|path| PathBuf::from("/").join(path))
            .chain(spec.user_bundles.iter().map(|path| home.join(path)))
            .collect();
        run_in_with(app, &spec, &home, &locations, run_script)
    })())
}

fn run_in_with(
    app: &InstalledApp,
    spec: &ScriptUninstaller,
    home: &Path,
    locations: &[PathBuf],
    execute: impl FnOnce(&str, &ScriptUninstaller, &Path, bool) -> Result<(), String>,
) -> Result<(), String> {
    let bundle = app.install_location.as_ref().ok_or("未找到应用路径")?;
    if !locations.contains(bundle) {
        return Err("应用不在已核验的安装位置，拒绝执行卸载脚本".into());
    }
    let mut elevated = false;
    // All copies are in the script's deletion extent, even when launched from only one.
    for path in locations {
        match std::fs::symlink_metadata(path) {
            Ok(md) if md.is_dir() && !md.file_type().is_symlink() => {
                let user_install = path.starts_with(home);
                let anchor = if user_install {
                    home
                } else {
                    path.ancestors()
                        .filter(|parent| {
                            parent
                                .file_name()
                                .is_some_and(|name| name == "Applications" || name == "Library")
                        })
                        .last()
                        .ok_or("无法确认系统安装根")?
                };
                if crate::core::safety::is_protected_residual_path(path)
                    || !crate::core::rules::facts::confined_path(anchor, path).unwrap_or(false)
                    || !crate::core::rules::facts::confined_file(
                        path,
                        &path.join("Contents/Info.plist"),
                    )
                    .unwrap_or(false)
                    || super::read_info_plist(&path.join("Contents/Info.plist"))
                        .0
                        .as_deref()
                        != Some(&spec.bundle_id)
                {
                    return Err(format!("无法确认应用身份或路径：{}", path.display()));
                }
                elevated |= !user_install;
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err(format!("无法确认应用路径：{}", path.display())),
        }
    }
    let script = uninstaller(bundle, spec).ok_or("未找到已核验的官方卸载脚本")?;
    let bytes = std::fs::read(script).map_err(|err| err.to_string())?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if !spec.sha256.contains(&digest) {
        return Err("官方卸载脚本版本尚未核验，请使用官方卸载方式；未执行回退删除".into());
    }
    let body = String::from_utf8(bytes).map_err(|err| err.to_string())?;
    // Execute frozen bytes, with the exact reviewed argument list and original user HOME.
    execute(&body, spec, home, elevated)?;
    for path in locations {
        if !matches!(std::fs::symlink_metadata(path), Err(err) if err.kind() == std::io::ErrorKind::NotFound)
        {
            return Err(format!("卸载后应用仍存在或无法核验：{}", path.display()));
        }
    }
    Ok(())
}

fn run_script(
    body: &str,
    spec: &ScriptUninstaller,
    home: &Path,
    elevated: bool,
) -> Result<(), String> {
    use std::ffi::OsString;
    let mut user_home = OsString::from("HOME=");
    user_home.push(home);
    let (program, mut args) = if elevated {
        (
            "/usr/bin/osascript",
            vec![
                "-e".into(),
                ELEVATED_SCRIPT.into(),
                "--".into(),
                home.as_os_str().to_owned(),
                spec.interpreter.executable().into(),
                body.into(),
            ],
        )
    } else {
        (
            "/usr/bin/env",
            vec![
                "-i".into(),
                "PATH=/usr/bin:/bin:/usr/sbin:/sbin".into(),
                user_home,
                spec.interpreter.executable().into(),
                "-c".into(),
                body.into(),
                "uninstall".into(),
            ],
        )
    };
    args.extend(spec.arguments.iter().map(OsString::from));
    let run = crate::core::proc::run_with_timeout(program, &args, Duration::from_secs(30 * 60))
        .ok_or("官方卸载未完成或已超时；未执行回退删除")?;
    if !run.ok {
        return Err(format!(
            "官方卸载失败：{}",
            String::from_utf8_lossy(&run.stderr).trim()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    const BUNDLE_ID: &str = "app.qingjian.inputmethod";
    const SCRIPT_PATH: &str = "Contents/Resources/uninstall.sh";
    fn profile() -> ScriptUninstaller {
        crate::core::rules::uninstall::script_for(BUNDLE_ID).unwrap()
    }
    fn run_in(app: &InstalledApp, home: &Path, locations: &[PathBuf]) -> Result<(), String> {
        run_in_with(app, &profile(), home, locations, run_script)
    }

    struct Fixture {
        root: PathBuf,
        home: PathBuf,
        locations: [PathBuf; 2],
        app: InstalledApp,
    }
    impl Fixture {
        fn new(tag: &str, body: &str) -> Self {
            let root = crate::core::testing::fixture(tag);
            let home = root.join("home ' $(ignored)");
            let locations = [
                root.join("system/Library/Input Methods/Qingjian.app"),
                home.join("Library/Input Methods/Qingjian.app"),
            ];
            Self::bundle(&locations[1], BUNDLE_ID, body);
            let app = super::super::parse_app_bundle(&locations[1], false, (None, 0), 0).unwrap();
            Self {
                root,
                home,
                locations,
                app,
            }
        }
        fn bundle(path: &Path, id: &str, body: &str) {
            std::fs::create_dir_all(path.join("Contents/Resources")).unwrap();
            std::fs::write(path.join("Contents/Info.plist"), format!(r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>{id}</string></dict></plist>"#)).unwrap();
            std::fs::write(path.join(SCRIPT_PATH), body).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn approved<T>(body: &str, work: impl FnOnce() -> T) -> T {
        let mut bundle = crate::core::rules::snapshot().bundle.clone();
        bundle
            .rules
            .iter_mut()
            .find(|rule| rule.id == "residual-macos")
            .unwrap()
            .script_uninstallers[0]
            .sha256 = vec![format!("{:x}", Sha256::digest(body.as_bytes()))];
        bundle.validate().unwrap();
        crate::core::rules::with_snapshot(
            Arc::new(crate::core::rules::RuleSnapshot { bundle }),
            work,
        )
    }

    #[test]
    fn qingjian_script_uninstall_preserves_data_and_uses_verified_bytes() {
        let body = "rm -rf \"$HOME/Library/Input Methods/Qingjian.app\"\n";
        let fixture = Fixture::new("qc_qingjian_script_success", body);
        let data = fixture
            .home
            .join("Library/Application Support/Qingjian/config.toml");
        std::fs::create_dir_all(data.parent().unwrap()).unwrap();
        std::fs::write(&data, "keep").unwrap();
        assert_eq!(
            fixture.app.registry_subpath, BUNDLE_ID,
            "plist identity must be read before selecting a script"
        );
        assert_eq!(
            fixture.app.uninstall_string.as_deref(),
            fixture.locations[1].join(SCRIPT_PATH).to_str()
        );
        approved(body, || {
            run_in_with(
                &fixture.app,
                &profile(),
                &fixture.home,
                &fixture.locations,
                |verified, spec, home, elevated| {
                    assert!(!elevated);
                    assert_eq!(verified, body);
                    std::fs::write(fixture.locations[1].join(SCRIPT_PATH), "exit 5\n").unwrap();
                    run_script(verified, spec, home, elevated)
                },
            )
            .unwrap();
        });
        assert_eq!(std::fs::read_to_string(data).unwrap(), "keep");
    }

    #[test]
    fn qingjian_script_zero_exit_and_failure_do_not_grant_trash_fallback() {
        for (tag, body) in [
            ("qc_qingjian_script_noop", "exit 0\n"),
            ("qc_qingjian_script_failure", "exit 3\n"),
        ] {
            let fixture = Fixture::new(tag, body);
            approved(body, || {
                assert!(run_in(&fixture.app, &fixture.home, &fixture.locations).is_err())
            });
            assert!(fixture.locations[1].is_dir());
        }
    }

    #[test]
    fn qingjian_script_rejects_unreviewed_content_and_redirected_paths() {
        let body = "exit 0\n";
        let fixture = Fixture::new("qc_qingjian_script_reject", body);
        assert!(run_in_with(
            &fixture.app,
            &profile(),
            &fixture.home,
            &fixture.locations,
            |_, _, _, _| panic!("unreviewed script executed")
        )
        .is_err());
        let script = fixture.locations[1].join(SCRIPT_PATH);
        std::fs::rename(&script, fixture.root.join("external.sh")).unwrap();
        std::os::unix::fs::symlink(fixture.root.join("external.sh"), &script).unwrap();
        approved(body, || {
            assert!(run_in_with(
                &fixture.app,
                &profile(),
                &fixture.home,
                &fixture.locations,
                |_, _, _, _| panic!("redirected script executed")
            )
            .is_err())
        });
        std::fs::remove_file(&script).unwrap();
        std::fs::write(&script, body).unwrap();
        Fixture::bundle(&fixture.locations[0], "other.inputmethod", body);
        approved(body, || {
            assert!(run_in_with(
                &fixture.app,
                &profile(),
                &fixture.home,
                &fixture.locations,
                |_, _, _, _| panic!("unknown second copy removed")
            )
            .is_err())
        });
    }

    #[test]
    fn qingjian_script_verifies_both_copies_after_elevated_operation() {
        let body = "exit 0\n";
        let fixture = Fixture::new("qc_qingjian_script_two_copies", body);
        Fixture::bundle(&fixture.locations[0], BUNDLE_ID, body);
        let called = std::cell::Cell::new(false);
        approved(body, || {
            assert!(run_in_with(
                &fixture.app,
                &profile(),
                &fixture.home,
                &fixture.locations,
                |_, _, _, elevated| {
                    called.set(true);
                    assert!(elevated);
                    std::fs::remove_dir_all(&fixture.locations[1]).unwrap();
                    Ok(())
                }
            )
            .is_err());
        });
        assert!(
            called.get(),
            "identity checks must reach the official executor"
        );
        assert!(fixture.locations[0].is_dir());
    }

    #[test]
    fn qingjian_elevated_command_quotes_home_and_does_not_pass_purge() {
        // 真实执行同一 AppleScript 的命令组装，仅去掉管理员授权，避免测试弹密码框。
        let script = ELEVATED_SCRIPT.replace(" with administrator privileges", "");
        let home = "/Users/test ' $(ignored)";
        let run = crate::core::proc::run_with_timeout(
            "/usr/bin/osascript",
            &[
                "-e",
                &script,
                "--",
                home,
                "/bin/sh",
                "test \"$#\" -eq 0 && printf '%s' \"$HOME\"",
            ],
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(run.ok, "{}", String::from_utf8_lossy(&run.stderr));
        assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), home);
    }

    #[test]
    fn script_uninstall_config_only_extension_runs_bash_and_literal_arguments() {
        let body = r#"test -z "$UNREVIEWED_STARTUP" || exit 9
test "$1" = --keep-data || exit 9
values=(one two)
[[ ${#values[@]} = 2 ]] || exit 9
test -z "$3" || exit 9
printf '%s' "$2" > "$HOME/argv-receipt"
rm -rf "$HOME/Applications/Fixture Tool.app"
"#;
        let fixture = Fixture::new("qc_script_config_extension", "exit 0\n");
        let bundle = fixture.home.join("Applications/Fixture Tool.app");
        Fixture::bundle(&bundle, "fixture.tool", "exit 0\n");
        let script = "Contents/Resources/remove-custom.sh";
        std::fs::write(bundle.join(script), body).unwrap();
        let literal = "literal ' $(touch injected) ; --purge";
        let mut spec = profile();
        spec.bundle_id = "fixture.tool".into();
        spec.script = script.into();
        spec.sha256 = vec![format!("{:x}", Sha256::digest(body.as_bytes()))];
        spec.interpreter = crate::core::rules::uninstall::Shell::Bash;
        spec.arguments = vec!["--keep-data".into(), literal.into(), "".into()];
        spec.user_bundles = vec!["Applications/Fixture Tool.app".into()];
        spec.system_bundles.clear();
        let mut policy = crate::core::rules::snapshot().bundle.clone();
        policy
            .rules
            .iter_mut()
            .find(|rule| rule.id == "residual-macos")
            .unwrap()
            .script_uninstallers
            .push(spec);
        policy.validate().unwrap();
        crate::core::rules::with_snapshot(
            Arc::new(crate::core::rules::RuleSnapshot { bundle: policy }),
            || {
                let app = super::super::parse_app_bundle(&bundle, false, (None, 0), 0).unwrap();
                assert_eq!(
                    app.uninstall_string.as_deref(),
                    bundle.join(script).to_str()
                );
                let spec =
                    crate::core::rules::uninstall::script_for(&app.registry_subpath).unwrap();
                run_in_with(
                    &app,
                    &spec,
                    &fixture.home,
                    std::slice::from_ref(&bundle),
                    run_script,
                )
                .unwrap();
            },
        );
        assert!(!bundle.exists());
        assert_eq!(
            std::fs::read_to_string(fixture.home.join("argv-receipt")).unwrap(),
            literal
        );
        assert!(
            fixture.locations[1].is_dir(),
            "a different installation must stay intact"
        );
    }

    #[test]
    fn script_uninstall_elevated_arguments_are_literal() {
        let apple_script = ELEVATED_SCRIPT.replace(" with administrator privileges", "");
        let run = crate::core::proc::run_with_timeout(
            "/usr/bin/osascript",
            &[
                "-e",
                &apple_script,
                "--",
                "/Users/test ' $(ignored)",
                "/bin/bash",
                "test \"$#\" -eq 2 && test -z \"$2\" && printf '%s' \"$1\"",
                "literal ' $(ignored) ; --purge",
                "",
            ],
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(run.ok, "{}", String::from_utf8_lossy(&run.stderr));
        assert_eq!(
            String::from_utf8_lossy(&run.stdout).trim(),
            "literal ' $(ignored) ; --purge"
        );
    }

    #[test]
    fn msime_manual_uninstall_does_not_delete_either_bundle() {
        let fixture = Fixture::new("qc_msime_manual_uninstall", "exit 0\n");
        for id in [
            "app.msime.macos",
            "app.msime.inputmethod.MetasequoiaIME",
            "app.msime.macos.wubi",
            "app.msime.inputmethod.wubi",
        ] {
            Fixture::bundle(&fixture.locations[1], id, "exit 0\n");
            let app =
                super::super::parse_app_bundle(&fixture.locations[1], false, (None, 0), 0).unwrap();
            assert_eq!(
                app.registry_subpath, id,
                "bundle identity must be read exactly"
            );
            assert_eq!(app.registry_root, crate::core::apps::AppRegRoot::Hkcu);
            assert!(crate::core::rules::uninstall::requires_manual_uninstall(
                &app
            ));
            assert!(super::super::run_uninstaller_and_wait(&app).is_err());
            assert!(fixture.locations[1].join("Contents/Info.plist").is_file());
        }
    }
}
