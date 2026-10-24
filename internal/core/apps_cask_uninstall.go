package core

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type caskEvidence struct {
	Path  string
	Bytes []byte
}
type caskInstallation struct {
	Prefix, Token, Receipt, Definition string
	Files                              []caskEvidence
	Scope                              InstalledCaskScope
	Supported                          bool
}

func caskAbsent(path string) bool { _, err := os.Lstat(path); return os.IsNotExist(err) }

func caskChildren(root string) ([]string, error) {
	f, err := os.Open(root)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	names, err := f.Readdirnames(4097)
	if err != nil && err != io.EOF {
		return nil, err
	}
	if len(names) > 4096 {
		return nil, errors.New("Homebrew inventory limit exceeded")
	}
	paths := make([]string, len(names))
	for i, name := range names {
		paths[i] = filepath.Join(root, name)
	}
	return paths, nil
}

func caskRead(root, path string, files *[]caskEvidence) ([]byte, error) {
	ok, err := ConfinedRuleFile(root, path)
	if err != nil {
		return nil, err
	}
	if !ok {
		return nil, fmt.Errorf("Homebrew evidence missing: %s", path)
	}
	b, err := readRuleEvidence(path)
	if err != nil {
		return nil, err
	}
	*files = append(*files, caskEvidence{path, b})
	return b, nil
}

func caskJSON(root, path string, files *[]caskEvidence) (map[string]any, error) {
	b, err := caskRead(root, path, files)
	if err != nil {
		return nil, err
	}
	if !utf8.Valid(b) {
		return nil, errors.New("invalid UTF-8 in Homebrew evidence")
	}
	var v any
	if err = json.Unmarshal(b, &v); err != nil {
		return nil, err
	}
	return caskObject(v), nil
}

func caskRequirePath(root, target string) error {
	ok, err := ConfinedRulePath(root, target)
	if err != nil {
		return err
	}
	if !ok {
		return errors.New("Homebrew path unavailable")
	}
	return nil
}

func caskInventory(prefix, home, id string) ([]caskInstallation, error) {
	root := filepath.Join(prefix, "Caskroom")
	if caskAbsent(root) {
		return nil, nil
	}
	if err := caskRequirePath(prefix, root); err != nil {
		return nil, err
	}
	directories, err := caskChildren(root)
	if err != nil {
		return nil, err
	}
	var installs []caskInstallation
	for _, directory := range directories {
		info, err := os.Lstat(directory)
		if err != nil {
			return nil, err
		}
		if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
			continue
		}
		if err := caskRequirePath(root, directory); err != nil {
			return nil, err
		}
		token := filepath.Base(directory)
		if token == "" || strings.HasPrefix(token, "-") || strings.ContainsFunc(token, func(r rune) bool {
			return !(r >= 'a' && r <= 'z' || r >= '0' && r <= '9' || strings.ContainsRune("-+@._", r))
		}) {
			return nil, errors.New("invalid Homebrew installation identity")
		}
		metadata := filepath.Join(directory, ".metadata")
		if caskAbsent(metadata) {
			continue
		}
		install := caskInstallation{Prefix: prefix, Token: token, Receipt: filepath.Join(metadata, "INSTALL_RECEIPT.json")}
		receipt, err := caskJSON(root, install.Receipt, &install.Files)
		if err != nil {
			return nil, err
		}
		config, err := caskJSON(root, filepath.Join(metadata, "config.json"), &install.Files)
		if err != nil {
			return nil, err
		}
		versions, err := caskChildren(metadata)
		if err != nil {
			return nil, err
		}
		latest := ""
		for _, version := range versions {
			info, err := os.Lstat(version)
			if err != nil {
				return nil, err
			}
			if !info.IsDir() {
				continue
			}
			if err := caskRequirePath(root, version); err != nil {
				return nil, err
			}
			timestamps, err := caskChildren(version)
			if err != nil {
				return nil, err
			}
			for _, timestamp := range timestamps {
				if err := caskRequirePath(root, timestamp); err != nil {
					return nil, err
				}
				if latest == "" || filepath.Base(timestamp) > filepath.Base(latest) {
					latest = timestamp
				}
			}
		}
		if latest == "" {
			return nil, errors.New("installed Homebrew definition unavailable")
		}
		for _, ext := range []string{"json", "internal.json", "rb"} {
			p := filepath.Join(latest, "Casks", token+"."+ext)
			if !caskAbsent(p) {
				install.Definition = p
				break
			}
		}
		if install.Definition == "" {
			return nil, errors.New("installed Homebrew definition unavailable")
		}
		definition := map[string]any{}
		isJSON := filepath.Ext(install.Definition) == ".json"
		if isJSON {
			definition, err = caskJSON(root, install.Definition, &install.Files)
		} else {
			_, err = caskRead(root, install.Definition, &install.Files)
		}
		if err != nil {
			return nil, err
		}
		source := caskObject(receipt["source"])
		install.Supported = isJSON && source["tap"] == "homebrew/cask" && source["version"] == filepath.Base(filepath.Dir(latest))
		if v, exists := definition["token"]; exists {
			install.Supported = install.Supported && v == token
		}
		if v, exists := definition["old_tokens"]; exists {
			a, ok := v.([]any)
			install.Supported = install.Supported && ok && len(a) == 0
		}
		install.Scope, err = ParseInstalledCaskScope(receipt, definition, config, home, id)
		if err != nil {
			return nil, err
		}
		installs = append(installs, install)
	}
	return installs, nil
}

// RunAppCaskUninstaller returns false,nil only when no persisted install owns the
// selected path. Every refusal or attempted uninstall is an error, never fallback
// deletion authority. Callers must propagate errors without trying another route.
func RunAppCaskUninstaller(ctx context.Context, app InstalledApp) (handled bool, err error) {
	home, ok := platform.UserHome()
	if !ok {
		return false, errors.New("user home unavailable")
	}
	return runAppCaskUninstaller(ctx, app, home, []string{"/opt/homebrew", "/usr/local"}, platform.ReadBundleMetadata, runCaskCommand)
}

func runCaskCommand(ctx context.Context, brew, home, token string) (platform.ProcessResult, bool) {
	ctx, cancel := context.WithTimeout(ctx, 30*time.Minute)
	defer cancel()
	cmd := exec.CommandContext(ctx, "/usr/bin/env", "-i", "PATH=/usr/bin:/bin:/usr/sbin:/sbin", "HOME="+home, "HOMEBREW_NO_AUTO_UPDATE=1", "HOMEBREW_NO_AUTOREMOVE=1", "HOMEBREW_NO_INSTALL_FROM_API=1", "HOMEBREW_NO_ANALYTICS=1", brew, "uninstall", "--cask", "--", token)
	return platform.RunCommandTreeContext(ctx, cmd, 30*time.Minute)
}

func runAppCaskUninstaller(ctx context.Context, app InstalledApp, home string, prefixes []string, metadata func(string) (*string, *string), run func(context.Context, string, string, string) (platform.ProcessResult, bool)) (bool, error) {
	if err := ctx.Err(); err != nil {
		return false, err
	}
	if app.InstallLocation == nil {
		return false, errors.New("application path unavailable")
	}
	selected := *app.InstallLocation
	var matched *caskInstallation
	for _, prefix := range prefixes {
		installs, err := caskInventory(prefix, home, app.RegistrySubpath)
		if err != nil {
			return false, err
		}
		for _, install := range installs {
			for _, p := range install.Scope.Apps {
				if p == selected {
					if matched != nil {
						return false, errors.New("multiple Homebrew installations claim this application")
					}
					copy := install
					matched = &copy
					break
				}
			}
		}
	}
	if matched == nil {
		return false, nil
	}
	install := *matched
	if !install.Supported || !install.Scope.Authorizes(selected) {
		return true, errors.New("Homebrew removal includes other apps, scripts, services or unknown scope; no fallback deletion performed")
	}
	if err := caskRequirePath(selected, selected); err != nil {
		return true, err
	}
	id, _ := metadata(selected)
	if id == nil || *id != app.RegistrySubpath {
		return true, errors.New("application identity changed before Homebrew uninstall")
	}
	identity, ok := CaptureIdentity(selected)
	if !ok {
		return true, errors.New("application identity unavailable")
	}
	brew, err := filepath.EvalSymlinks(filepath.Join(install.Prefix, "bin", "brew"))
	if err != nil {
		return true, err
	}
	confined, err := ConfinedRuleFile(install.Prefix, brew)
	if err != nil {
		return true, err
	}
	if !confined {
		return true, errors.New("Homebrew executable outside installation")
	}
	executable, ok := CaptureIdentity(brew)
	if !ok {
		return true, errors.New("Homebrew executable identity unavailable")
	}
	current, err := caskInventory(install.Prefix, home, app.RegistrySubpath)
	if err != nil {
		return true, err
	}
	same := false
	for _, record := range current {
		if record.Token == install.Token && record.Definition == install.Definition && reflect.DeepEqual(record.Files, install.Files) {
			same = true
		}
	}
	if !same || !identity.Recheck(selected) || !executable.Recheck(brew) {
		return true, errors.New("Homebrew installation changed; refresh before retrying")
	}
	if err := ctx.Err(); err != nil {
		return true, err
	}
	result, complete := run(ctx, brew, home, install.Token)
	if err := ctx.Err(); err != nil {
		return true, err
	}
	if !complete {
		return true, errors.New("Homebrew uninstall unavailable or timed out; no fallback deletion performed")
	}
	if !result.OK {
		return true, fmt.Errorf("Homebrew uninstall failed: %s", strings.TrimSpace(string(result.Stderr)))
	}
	if !caskAbsent(selected) || !caskAbsent(install.Receipt) || !caskAbsent(install.Definition) {
		return true, errors.New("Homebrew exited but application or installation records remain or cannot be verified")
	}
	return true, nil
}
