//! 精确卸载登记身份与数据目录别名；不参与名称模糊匹配。
use crate::core::apps::InstalledApp;
use std::path::{Path, PathBuf};

pub(crate) fn registered_data_directories(app: &InstalledApp, roots: &[PathBuf]) -> Vec<PathBuf> {
    let snapshot = super::current();
    snapshot
        .definition("residual-windows")
        .app_data_aliases
        .iter()
        .filter(|alias| {
            super::uninstall::registration_matches(
                app,
                &alias.registry_id,
                &alias.name,
                &alias.publisher,
            )
        })
        .flat_map(|alias| roots.iter().map(move |root| root.join(&alias.directory)))
        .filter(|path| safe_directory(path))
        .collect()
}

fn safe_directory(path: &Path) -> bool {
    !crate::core::safety::is_protected_residual_path(path)
        && super::facts::confined_path(path.parent().unwrap_or(path), path).unwrap_or(false)
        && std::fs::symlink_metadata(path)
            .is_ok_and(|md| md.is_dir() && !super::facts::is_link(&md))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::apps::AppRegRoot;
    fn qingjian() -> InstalledApp {
        let id = "{A7E3C1F2-5B94-4D6A-9C0E-2F8B1D3A6E70}_is1";
        InstalledApp {
            discovery: None,
            id: id.into(),
            name: "青简".into(),
            version: "0.1.0".into(),
            publisher: "青简".into(),
            last_used_date: None,
            last_used_raw: 0,
            install_date: None,
            install_date_raw: 0,
            install_location: None,
            display_icon: None,
            uninstall_string: None,
            quiet_uninstall_string: None,
            estimated_size: 0,
            registry_root: AppRegRoot::Hklm,
            registry_subpath: format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{id}"),
            is_system_component: false,
            uninstaller_missing: false,
        }
    }
    #[test]
    fn qingjian_data_alias_requires_complete_registration_identity() {
        let root = crate::core::testing::fixture("qc_qingjian_alias");
        for name in ["Qingjian", "QingjianOther", "青简"] {
            std::fs::create_dir(root.join(name)).unwrap();
        }
        let roots = [root.clone()];
        let app = qingjian();
        assert_eq!(
            registered_data_directories(&app, &roots),
            vec![root.join("Qingjian")]
        );
        for name in ["青简 0.1.0", "青简 version 0.1.0", "青简 版本 0.1.0"] {
            let mut versioned = app.clone();
            versioned.name = name.into();
            assert_eq!(
                registered_data_directories(&versioned, &roots),
                vec![root.join("Qingjian")]
            );
        }
        for field in 0..5 {
            let mut other = app.clone();
            match field {
                0 => other.id = "other".into(),
                1 => other.name = "青简工具".into(),
                2 => other.publisher = "other".into(),
                3 => other.registry_subpath = "other".into(),
                _ => other.is_system_component = true,
            }
            assert!(registered_data_directories(&other, &roots).is_empty());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn qingjian_data_alias_rejects_links_and_path_escape() {
        let mut bundle = super::super::snapshot().bundle.clone();
        let rule = bundle
            .rules
            .iter_mut()
            .find(|rule| rule.id == "residual-windows")
            .unwrap();
        rule.app_data_aliases = vec![super::super::AppDataAlias {
            registry_id: qingjian().id,
            name: "青简".into(),
            publisher: "青简".into(),
            directory: "../other".into(),
        }];
        assert!(bundle.validate().is_err());
        #[cfg(unix)]
        {
            let root = crate::core::testing::fixture("qc_qingjian_alias_link");
            std::fs::create_dir(root.join("other")).unwrap();
            std::os::unix::fs::symlink(root.join("other"), root.join("Qingjian")).unwrap();
            assert!(
                registered_data_directories(&qingjian(), std::slice::from_ref(&root)).is_empty()
            );
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
