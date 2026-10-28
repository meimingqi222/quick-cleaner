package core

import (
	"context"
	"slices"
	"strings"
)

func scanWindowsResidualRegistry(ctx context.Context, app InstalledApp, c windowsResidualContext, r windowsResidualReader) []ResidualItem {
	var out []ResidualItem
	key := func(root AppRegRoot, path string, source ResidualSource, confidence Confidence) {
		out = append(out, NewResidualItem(ResidualKind{Type: ResidualRegistryKey, Root: root, Path: path}, source, confidence))
	}
	value := func(root AppRegRoot, path, name string, source ResidualSource, confidence Confidence) {
		out = append(out, NewResidualItem(ResidualKind{Type: ResidualRegistryValue, Root: root, Path: path, Name: name}, source, confidence))
	}
	if app.RegistrySubpath != "" && r.KeyExists(app.RegistryRoot.Label(), app.RegistrySubpath) {
		key(app.RegistryRoot, app.RegistrySubpath, ResidualUninstallEntry, Certain)
	}
	for _, root := range []AppRegRoot{AppHKCU, AppHKLM, AppHKLM32} {
		if ctx.Err() != nil {
			break
		}
		base := "SOFTWARE"
		if root == AppHKCU {
			base = "Software"
		}
		if root == AppHKLM32 {
			base += `\WOW6432Node`
		}
		if c.matchable() {
			direct := []string{base + `\` + c.token}
			if IsSafeAppToken(c.publisher) {
				direct = append(direct, base+`\`+c.publisher+`\`+c.token)
			}
			for _, path := range direct {
				if r.KeyExists(root.Label(), path) {
					key(root, path, ResidualConfigRegKey, Certain)
				}
			}
			for _, name := range r.Subkeys(root.Label(), base) {
				if !strings.EqualFold(name, c.token) && !strings.EqualFold(name, "WOW6432Node") && c.matches(name) {
					key(root, base+`\`+name, ResidualLikelyConfigRegKey, Possible)
				}
			}
		}
		if c.install != "" {
			mentions := func(path string) bool {
				for _, text := range r.Values(root.Label(), path) {
					if c.mentions(text) {
						return true
					}
				}
				return false
			}
			for n, name := range r.Subkeys(root.Label(), base) {
				if n >= 1500 || ctx.Err() != nil {
					break
				}
				if strings.EqualFold(name, "WOW6432Node") {
					continue
				}
				p := base + `\` + name
				if mentions(p) {
					key(root, p, ResidualVendorRegKey, Certain)
					continue
				}
				for n, child := range r.Subkeys(root.Label(), p) {
					if n >= 200 || ctx.Err() != nil {
						break
					}
					path := p + `\` + child
					if mentions(path) {
						key(root, path, ResidualVendorRegKey, Certain)
					}
				}
			}
		}
		for _, anchor := range c.list("run_keys") {
			for name, text := range r.Values(root.Label(), anchor) {
				if c.mentions(text) || c.mentionsExe(text) {
					value(root, anchor, name, ResidualStartupEntry, Certain)
				} else if c.matches(name) {
					value(root, anchor, name, ResidualStartupEntry, Possible)
				}
			}
		}
		if root != AppHKCU {
			if anchor := c.anchor("app_paths_keys"); anchor != "" {
				for _, name := range r.Subkeys(root.Label(), anchor) {
					path := anchor + `\` + name
					if c.mentions(r.Value(root.Label(), path, "")) || slices.ContainsFunc(c.exes, func(exe string) bool { return strings.EqualFold(name, exe) }) {
						key(root, path, ResidualAppPathsEntry, Certain)
					}
				}
			}
		}
	}
	if anchor := c.anchor("user_environment_keys"); anchor != "" {
		for name, text := range r.Values("HKCU", anchor) {
			if c.mentions(text) || c.mentionsExe(text) {
				value(AppHKCU, anchor, name, ResidualUserEnvEntry, Certain)
			} else if c.matches(name) {
				value(AppHKCU, anchor, name, ResidualUserEnvEntry, Possible)
			}
		}
	}
	if anchor := c.anchor("service_keys"); anchor != "" && (c.install != "" || c.matchable()) {
		for _, name := range r.Subkeys("HKLM", anchor) {
			path := anchor + `\` + name
			image := r.Value("HKLM", path, "ImagePath")
			if c.mentions(image) {
				key(AppHKLM, path, ResidualService, Certain)
			} else if image != "" && c.matches(name) {
				key(AppHKLM, path, ResidualLikelyService, Possible)
			}
		}
	}
	if anchor := c.anchor("firewall_keys"); anchor != "" && c.install != "" {
		for name, text := range r.Values("HKLM", anchor) {
			if c.mentions(text) {
				value(AppHKLM, anchor, name, ResidualFirewallRule, Certain)
			}
		}
	}
	if anchor := c.anchor("tracing_keys"); anchor != "" && c.matchable() {
		for _, name := range r.Subkeys("HKLM", anchor) {
			stem := name
			if i := strings.LastIndexByte(stem, '_'); i > 0 {
				stem = stem[:i]
			}
			if c.matches(stem) {
				key(AppHKLM, anchor+`\`+name, ResidualRasTrace, Possible)
			}
		}
	}
	if anchor := c.anchor("heap_leak_keys"); anchor != "" && len(c.exes) > 0 {
		for _, name := range r.Subkeys("HKLM", anchor) {
			if slices.ContainsFunc(c.exes, func(exe string) bool { return strings.EqualFold(name, exe) }) {
				key(AppHKLM, anchor+`\`+name, ResidualLeakDiagnostics, Certain)
			}
		}
	}
	if c.install != "" {
		for _, anchor := range c.list("app_compat_keys") {
			for _, root := range []AppRegRoot{AppHKCU, AppHKLM} {
				for name := range r.Values(root.Label(), anchor) {
					if c.mentions(name) {
						value(root, anchor, name, ResidualCompatSetting, Certain)
					}
				}
			}
		}
		for _, route := range []struct {
			anchor string
			root   AppRegRoot
			source ResidualSource
		}{{"installer_folder_keys", AppHKLM, ResidualInstallerFolderEntry}, {"mui_cache_keys", AppHKCU, ResidualProgramNameCache}} {
			if anchor := c.anchor(route.anchor); anchor != "" {
				for name := range r.Values(route.root.Label(), anchor) {
					if c.mentions(name) {
						value(route.root, anchor, name, route.source, Certain)
					}
				}
			}
		}
	}
	if anchor := c.anchor("registered_apps_keys"); anchor != "" && c.matchable() {
		for name, text := range r.Values("HKLM", anchor) {
			if c.matches(name) || c.matches(text) {
				value(AppHKLM, anchor, name, ResidualDefaultProgramsEntry, Possible)
			}
		}
	}
	out = append(out, scanWindowsResidualCOM(ctx, c, r)...)
	return out
}

func scanWindowsResidualCOM(ctx context.Context, c windowsResidualContext, r windowsResidualReader) []ResidualItem {
	if c.install == "" {
		return nil
	}
	var out []ResidualItem
	guids := make(map[string]bool)
	key := func(root AppRegRoot, path string, source ResidualSource) {
		out = append(out, CertainResidual(ResidualKind{Type: ResidualRegistryKey, Root: root, Path: path}, source))
	}
	validGUID := func(guid string) bool {
		guid = strings.TrimSpace(guid)
		return strings.HasPrefix(guid, "{") && !strings.Contains(guid, "-0000-")
	}
	serverMatches := func(server string) bool {
		path := r.Expand(server)
		return path != "" && !windowsResidualSystemFile(path) && c.mentions(path)
	}
	for _, base := range c.list("com_classes_roots") {
		for _, root := range []AppRegRoot{AppHKLM, AppHKCU} {
			classes := base
			if root == AppHKCU {
				classes = strings.Replace(classes, "SOFTWARE", "Software", 1)
			}
			clsid := classes + `\CLSID`
			for n, guid := range r.Subkeys(root.Label(), clsid) {
				if n >= 8000 || ctx.Err() != nil {
					break
				}
				if !validGUID(guid) {
					continue
				}
				path := clsid + `\` + guid
				server := r.Value(root.Label(), path+`\InprocServer32`, "")
				if server == "" {
					server = r.Value(root.Label(), path+`\LocalServer32`, "")
				}
				if !serverMatches(server) {
					continue
				}
				guids[asciiLower(guid)] = true
				key(root, path, ResidualComClass)
				progid := strings.TrimSpace(r.Value(root.Label(), path+`\ProgID`, ""))
				if len(progid) >= 4 && strings.Contains(progid, ".") && r.KeyExists(root.Label(), classes+`\`+progid) {
					key(root, classes+`\`+progid, ResidualComClass)
				}
			}
			typelib := classes + `\TypeLib`
			for n, guid := range r.Subkeys(root.Label(), typelib) {
				if n >= 4000 || ctx.Err() != nil {
					break
				}
				if !validGUID(guid) {
					continue
				}
				path := typelib + `\` + guid
				for n, version := range r.Subkeys(root.Label(), path) {
					if n >= 8 {
						break
					}
					for _, arch := range []string{"win32", "win64"} {
						if serverMatches(r.Value(root.Label(), path+`\`+version+`\0\`+arch, "")) {
							guids[asciiLower(guid)] = true
							key(root, path, ResidualComClass)
							break
						}
					}
				}
			}
		}
	}
	if len(guids) == 0 {
		return out
	}
	hit := func(text string) bool { return guids[asciiLower(strings.TrimSpace(text))] }
	for _, root := range []AppRegRoot{AppHKLM, AppHKLM32, AppHKCU} {
		for _, parent := range c.list("shell_extension_parents") {
			if root == AppHKCU {
				parent = strings.Replace(parent, "SOFTWARE", "Software", 1)
			}
			for _, name := range r.Subkeys(root.Label(), parent) {
				path := parent + `\` + name
				if hit(name) || hit(r.Value(root.Label(), path, "")) {
					key(root, path, ResidualShellExtension)
				}
			}
		}
		approved := c.anchor("shell_extension_approved_keys")
		if approved == "" {
			continue
		}
		if root == AppHKCU {
			approved = strings.Replace(approved, "SOFTWARE", "Software", 1)
		}
		for name := range r.Values(root.Label(), approved) {
			if hit(name) {
				out = append(out, CertainResidual(ResidualKind{Type: ResidualRegistryValue, Root: root, Path: approved, Name: name}, ResidualShellExtension))
			}
		}
	}
	return out
}
