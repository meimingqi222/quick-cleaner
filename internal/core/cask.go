package core

import (
	"errors"
	"path"
	"strings"
	"unicode"
)

// InstalledCaskScope describes the entire persisted uninstall, not just one app.
type InstalledCaskScope struct {
	Apps    []string
	AppOnly bool
}

func caskObject(v any) map[string]any { m, _ := v.(map[string]any); return m }

func caskCleanPath(s string, relative bool) bool {
	if s == "" || strings.Contains(s, `\`) || strings.ContainsFunc(s, unicode.IsControl) {
		return false
	}
	if relative && strings.HasPrefix(s, "/") {
		return false
	}
	for _, p := range strings.Split(s, "/") {
		if p == "." || p == ".." {
			return false
		}
	}
	return true
}

func caskExpand(s, home string) (string, error) {
	if !caskCleanPath(s, false) {
		return "", errors.New("unverifiable Homebrew appdir")
	}
	if strings.HasPrefix(s, "~/") {
		s = home + "/" + strings.TrimPrefix(s, "~/")
	}
	if !strings.HasPrefix(s, "/") || !caskCleanPath(s, false) {
		return "", errors.New("unverifiable Homebrew appdir")
	}
	return path.Clean(s), nil
}

// ParseInstalledCaskScope uses persisted appdir precedence and exact app targets.
// Unknown artifacts retain ownership information but never grant uninstall authority.
func ParseInstalledCaskScope(receipt, definition, config map[string]any, home, bundleID string) (InstalledCaskScope, error) {
	scope := InstalledCaskScope{}
	invalid := func() (InstalledCaskScope, error) {
		return InstalledCaskScope{}, errors.New("invalid or unconfined Homebrew artifact scope")
	}
	v, exists := definition["artifacts"]
	if !exists {
		v = receipt["uninstall_artifacts"]
	}
	artifacts, ok := v.([]any)
	if !ok {
		return invalid()
	}
	var appdirValue any
	for _, key := range []string{"explicit", "env", "default"} {
		if v, exists := caskObject(config[key])["appdir"]; exists {
			appdirValue = v
			break
		}
	}
	appdir, ok := appdirValue.(string)
	if !ok {
		return invalid()
	}
	appdir, err := caskExpand(appdir, home)
	if err != nil {
		return scope, err
	}
	flight, ok := receipt["uninstall_flight_blocks"].(bool)
	scope.AppOnly = ok && !flight
	if variations, exists := definition["variations"]; exists {
		m, ok := variations.(map[string]any)
		scope.AppOnly = scope.AppOnly && ok && len(m) == 0
	}
	for _, artifact := range artifacts {
		row, ok := artifact.(map[string]any)
		if !ok {
			return invalid()
		}
		if app, exists := row["app"]; exists {
			for key := range row {
				if key != "app" && key != "target" {
					scope.AppOnly = false
				}
			}
			args, ok := app.([]any)
			if !ok || len(args) < 1 || len(args) > 2 {
				return invalid()
			}
			source, ok := args[0].(string)
			if !ok || !caskCleanPath(source, true) {
				return invalid()
			}
			target := path.Base(source)
			if len(args) == 2 {
				options, ok := args[1].(map[string]any)
				if !ok {
					return invalid()
				}
				for key := range options {
					if key != "target" {
						scope.AppOnly = false
					}
				}
				target, ok = options["target"].(string)
				if !ok {
					return invalid()
				}
			}
			var destination string
			if strings.HasPrefix(target, "/") || strings.HasPrefix(target, "~/") {
				destination, err = caskExpand(target, home)
				if err != nil {
					return invalid()
				}
			} else {
				if !caskCleanPath(target, true) {
					return invalid()
				}
				destination = path.Join(appdir, target)
			}
			if path.Ext(destination) != ".app" {
				return invalid()
			}
			if v, exists := row["target"]; exists && v != destination {
				return invalid()
			}
			scope.Apps = append(scope.Apps, destination)
		} else if _, ok := row["zap"]; ok && len(row) == 1 {
			// Zap is never invoked and grants no ordinary uninstall authority.
		} else if uninstall, ok := row["uninstall"]; ok && len(row) == 1 {
			args, ok := uninstall.([]any)
			safe := ok && len(args) > 0
			own := func(v any) bool { s, ok := v.(string); return ok && bundleID != "" && s == bundleID }
			for _, arg := range args {
				options := caskObject(arg)
				quit := options["quit"]
				valid := own(quit)
				if ids, ok := quit.([]any); ok {
					valid = len(ids) > 0
					for _, id := range ids {
						valid = valid && own(id)
					}
				}
				safe = safe && len(options) == 1 && valid
			}
			scope.AppOnly = scope.AppOnly && safe
		} else {
			scope.AppOnly = false
		}
	}
	scope.AppOnly = scope.AppOnly && len(scope.Apps) == 1
	return scope, nil
}

func (s InstalledCaskScope) Authorizes(selected string) bool {
	return s.AppOnly && len(s.Apps) == 1 && s.Apps[0] == selected
}
