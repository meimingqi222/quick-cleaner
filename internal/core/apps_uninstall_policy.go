package core

func ReviewedScriptUninstaller(bundleID string) *ScriptUninstaller {
	snapshot, err := CurrentRules()
	if err != nil {
		return nil
	}
	if rule := snapshot.Definition("residual-macos"); rule != nil {
		for _, spec := range rule.ScriptUninstallers {
			if spec.BundleID == bundleID {
				// Callers may prepare arguments without modifying bundled policy.
				spec.Arguments = append([]string(nil), spec.Arguments...)
				spec.SHA256 = append([]string(nil), spec.SHA256...)
				spec.UserBundles = append([]string(nil), spec.UserBundles...)
				spec.SystemBundles = append([]string(nil), spec.SystemBundles...)
				return &spec
			}
		}
	}
	return nil
}

func RequiresManualUninstall(app InstalledApp) bool {
	return app.Discovery == nil && !app.IsSystemComponent && app.RegistryRoot == AppHKCU &&
		member(app.RegistrySubpath, RuleList("residual-macos", "manual_uninstall_bundle_ids"))
}

func UninstallDeletesUserData(app InstalledApp) bool {
	if app.Discovery == nil && !app.IsSystemComponent && app.RegistryRoot == AppHKCU {
		if spec := ReviewedScriptUninstaller(app.RegistrySubpath); spec != nil && !spec.PreservesUserData {
			return true
		}
	}
	snapshot, err := CurrentRules()
	if err != nil {
		return false
	}
	if rule := snapshot.Definition("residual-windows"); rule != nil {
		for _, warning := range rule.UninstallDataWarnings {
			if AppRegistrationMatches(app, warning.RegistryID, warning.Name, warning.Publisher) {
				return true
			}
		}
	}
	return false
}

func AppRegistrationMatches(app InstalledApp, id, name, publisher string) bool {
	if app.Discovery != nil || app.IsSystemComponent ||
		(app.RegistryRoot != AppHKLM && app.RegistryRoot != AppHKLM32 && app.RegistryRoot != AppHKCU) ||
		asciiLower(app.ID) != asciiLower(id) || asciiLower(app.RegistrySubpath) != asciiLower(`SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\`+id) ||
		app.Publisher != publisher {
		return false
	}
	if app.Name == name {
		return true
	}
	return app.Version != "" && member(app.Name, []string{name + " " + app.Version, name + " version " + app.Version, name + " 版本 " + app.Version})
}
