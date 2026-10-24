package platform

import "strings"

// appRegistration keeps registry parsing fixture-testable without a live hive.
type appRegistration struct {
	text  func(string) *string
	dword func(string) uint32
}

func parseAppRegistration(v appRegistration, id, root, subpath string) (AppRecord, bool) {
	var app AppRecord
	name := v.text("DisplayName")
	if name == nil {
		return app, false
	}
	app.Name = strings.TrimSpace(*name)
	if app.Name == "" || v.dword("SystemComponent") == 1 {
		return app, false
	}
	if parent := v.text("ParentKeyName"); parent != nil && strings.TrimSpace(*parent) != "" && !appASCIIEqualSteam(*parent) {
		return app, false
	}
	if len(app.Name) >= 3 && strings.HasPrefix(app.Name, "KB") && app.Name[2] >= '0' && app.Name[2] <= '9' {
		return app, false
	}
	trimmed := func(key string, quotes bool) *string {
		value := v.text(key)
		if value == nil {
			return nil
		}
		s := strings.TrimSpace(*value)
		if quotes {
			s = strings.Trim(s, `"`)
		}
		if s == "" {
			return nil
		}
		return &s
	}
	if s := v.text("DisplayVersion"); s != nil {
		app.Version = strings.TrimSpace(*s)
	}
	if s := v.text("Publisher"); s != nil {
		app.Publisher = strings.TrimSpace(*s)
	}
	app.ID, app.RegistryRoot, app.RegistrySubpath = id, root, subpath
	// The orchestrator parses this raw date in its own local-date policy.
	app.InstallDate = v.text("InstallDate")
	app.InstallLocation = trimmed("InstallLocation", true)
	app.DisplayIcon = trimmed("DisplayIcon", false)
	app.UninstallString = trimmed("UninstallString", false)
	app.QuietUninstallString = trimmed("QuietUninstallString", false)
	app.EstimatedSize = uint64(v.dword("EstimatedSize")) * 1024
	return app, true
}

func appASCIIEqualSteam(s string) bool {
	if len(s) != 5 {
		return false
	}
	for j := range len(s) {
		c := s[j]
		if c >= 'A' && c <= 'Z' {
			c += 'a' - 'A'
		}
		if c != "steam"[j] {
			return false
		}
	}
	return true
}
