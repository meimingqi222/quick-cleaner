package core

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"unicode/utf8"
)

type sourceInstallPlan struct {
	rule        SourceInstallRule
	home        string
	paths       []string
	state       string
	sharedTools bool
	startup     []installationArtifact
}

func sourceInstallKey(code string) string {
	digest := sha256.Sum256([]byte(code))
	return hex.EncodeToString(digest[:])[:16]
}

func canonicalSourceTarget(path string, canonical func(string) (string, bool)) (string, bool) {
	var missing []string
	for range 256 {
		if resolved, ok := canonical(path); ok {
			for i := len(missing) - 1; i >= 0; i-- {
				resolved = filepath.Join(resolved, missing[i])
			}
			return resolved, true
		}
		if _, err := os.Lstat(path); !os.IsNotExist(err) {
			return "", false
		}
		name, parent := filepath.Base(path), filepath.Dir(path)
		if name == "." || name == ".." || parent == path {
			return "", false
		}
		missing = append(missing, name)
		path = parent
	}
	return "", false
}

func sourceOwnedPath(home, path string) bool {
	rel, err := filepath.Rel(home, path)
	if err != nil || rel == "." || rel == ".." || filepath.IsAbs(rel) || strings.HasPrefix(rel, ".."+string(filepath.Separator)) || IsProtectedResidualPath(path) {
		return false
	}
	current := home
	for _, part := range strings.Split(rel, string(filepath.Separator)) {
		if part == "." || part == ".." || part == "" {
			return false
		}
		current = filepath.Join(current, part)
		info, err := os.Lstat(current)
		if err != nil && !os.IsNotExist(err) {
			return false
		}
		if err == nil && ruleEvidenceLink(info) {
			return false
		}
	}
	return true
}

func sourceOtherEntries(root, own string) bool {
	info, err := os.Lstat(root)
	if err != nil {
		return !os.IsNotExist(err)
	}
	if !info.IsDir() || ruleEvidenceLink(info) {
		return true
	}
	f, err := os.Open(root)
	if err != nil {
		return true
	}
	defer f.Close()
	entries, err := f.ReadDir(257)
	if err != nil && err != io.EOF {
		return true
	}
	if len(entries) > 256 {
		return true
	}
	for _, entry := range entries {
		if own != "" && Norm(filepath.Join(root, entry.Name())) == Norm(own) {
			continue
		}
		info, err := entry.Info()
		if err != nil || info.IsDir() || ruleEvidenceLink(info) {
			return true
		}
	}
	return false
}

func sourceReadFile(path string, limit int64) ([]byte, error) {
	info, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || ruleEvidenceLink(info) || info.Size() > limit {
		return nil, errors.New("invalid source installation evidence")
	}
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	data, err := io.ReadAll(io.LimitReader(f, limit+1))
	if int64(len(data)) > limit {
		return nil, errors.New("source installation evidence limit")
	}
	return data, err
}

func sourceReadJSON(path string) (any, error) {
	data, err := sourceReadFile(path, 4*1024*1024)
	if err != nil {
		return nil, err
	}
	if !utf8.Valid(data) {
		return nil, errors.New("invalid source installation JSON UTF-8")
	}
	var value any
	err = json.Unmarshal(bytes.TrimPrefix(data, []byte("\xef\xbb\xbf")), &value)
	return value, err
}

func sourceGatewayReference(text, home string, rule SourceInstallRule) bool {
	text = strings.ReplaceAll(appLower(text), "/", `\`)
	for _, dir := range []string{rule.GatewayDir, rule.Source} {
		path := strings.ReplaceAll(Norm(filepath.Join(home, filepath.FromSlash(dir))), "/", `\`)
		if strings.Contains(text, path+`\`) {
			return true
		}
	}
	return false
}

// Planning is read-only. Existing source signatures or an exact dependency
// receipt establish program ownership; names and shortcut locations never do.
func buildSourceInstallPlan(code string, sourceOwned bool, rule SourceInstallRule, canonical func(string) (string, bool), roaming string) (*sourceInstallPlan, error) {
	code, ok := canonicalSourceTarget(code, canonical)
	if !ok {
		return nil, errors.New("invalid source installation path")
	}
	home, ok := canonical(filepath.Dir(code))
	if !ok {
		return nil, errors.New("invalid source installation home")
	}
	if IsProtectedResidualPath(home) || !sourceOwnedPath(home, code) {
		return nil, errors.New("redirected or protected source installation")
	}
	rule = clonePlanValue(rule)
	p := &sourceInstallPlan{rule: rule, home: home, paths: []string{code}}
	join := func(parts ...string) string {
		for i, part := range parts {
			parts[i] = filepath.FromSlash(part)
		}
		return filepath.Join(append([]string{home}, parts...)...)
	}
	state := join(rule.StateDir, sourceInstallKey(code))
	if _, err := os.Lstat(state); err == nil {
		factsPath := filepath.Join(state, filepath.FromSlash(rule.StateFacts))
		if !sourceOwnedPath(home, state) || !sourceOwnedPath(home, factsPath) {
			return nil, errors.New("redirected installation receipt")
		}
		facts, err := sourceReadJSON(factsPath)
		if err != nil {
			return nil, fmt.Errorf("unreadable installation receipt: %w", err)
		}
		value, _ := jsonPointer(facts, rule.EnvironmentPointer)
		environment, ok := value.(string)
		if !ok {
			return nil, errors.New("missing environment receipt")
		}
		environment, ok = canonicalSourceTarget(environment, canonical)
		if !ok || !atOrUnder(Norm(environment), Norm(filepath.Join(state, rule.Environments))) || !sourceOwnedPath(home, environment) || filepath.Base(environment) != rule.EnvironmentName {
			return nil, errors.New("environment escapes its installation")
		}
		p.state = state
		p.paths = append(p.paths, state)
	} else if !os.IsNotExist(err) {
		return nil, fmt.Errorf("unreadable installation receipt: %w", err)
	}
	if !sourceOwned && p.state == "" {
		return nil, errors.New("no source or dependency ownership evidence")
	}
	if sourceOtherEntries(join(rule.Profiles), "") {
		return nil, errors.New("source is shared by named profiles")
	}
	p.sharedTools = sourceOtherEntries(join(rule.StateDir), p.state)
	if !p.sharedTools {
		for _, launcher := range rule.Launchers {
			p.paths = append(p.paths, join(launcher))
		}
		toolFacts := join(rule.ToolsDir, rule.ToolFacts)
		if !sourceOwnedPath(home, toolFacts) {
			return nil, errors.New("redirected tools receipt")
		}
		if _, err := os.Lstat(toolFacts); err == nil {
			facts, err := sourceReadJSON(toolFacts)
			if err != nil {
				return nil, fmt.Errorf("unreadable tool ownership receipt: %w", err)
			}
			value, _ := jsonPointer(facts, "/packages")
			packages, ok := value.(map[string]any)
			if !ok {
				return nil, errors.New("invalid tool ownership receipt")
			}
			for name, value := range packages {
				fact, ok := value.(map[string]any)
				if !ok {
					return nil, errors.New("invalid tool ownership entry")
				}
				entry, _ := fact["entry"].(string)
				digest, _ := fact["digest"].(string)
				_, hexErr := hex.DecodeString(digest)
				if entry == "" || entry == "." || entry == ".." || strings.ContainsAny(entry, `/\`) || !strings.HasPrefix(entry, name+"-") || len(digest) != 64 || hexErr != nil {
					return nil, errors.New("invalid tool ownership receipt")
				}
				p.paths = append(p.paths, join(rule.ToolsDir, entry))
			}
			p.paths = append(p.paths, toolFacts, join(rule.ToolsDir, rule.ToolLock))
		} else if !os.IsNotExist(err) {
			return nil, fmt.Errorf("unreadable tools receipt: %w", err)
		}
		setup, bootstrap := join(rule.Installer), join(rule.BootstrapDir)
		if !sourceOwnedPath(home, setup) || !sourceOwnedPath(home, bootstrap) {
			return nil, errors.New("redirected bootstrap files")
		}
		if data, err := sourceReadFile(setup, 16*1024*1024); err == nil && bytes.HasPrefix(data, []byte("MZ")) && bytes.Contains(data, []byte(rule.InstallerSignature)) {
			p.paths = append(p.paths, setup)
		}
		if f, err := os.Open(bootstrap); err == nil {
			entries, readErr := f.ReadDir(256)
			f.Close()
			if readErr != nil && readErr != io.EOF {
				return nil, readErr
			}
			for _, entry := range entries {
				name := entry.Name()
				if !strings.HasPrefix(name, rule.BootstrapPrefix) || !slices.ContainsFunc(rule.BootstrapExtensions, func(ext string) bool { return strings.HasSuffix(name, "."+ext) }) {
					continue
				}
				path := filepath.Join(bootstrap, name)
				if !sourceOwnedPath(home, path) {
					return nil, errors.New("redirected bootstrap script")
				}
				data, err := sourceReadFile(path, 4*1024*1024)
				if err != nil || !utf8.Valid(data) {
					return nil, errors.New("invalid bootstrap script")
				}
				if strings.Contains(string(data), rule.Source) {
					p.paths = append(p.paths, path)
				}
			}
		}
	}
	gateway := join(rule.GatewayDir)
	if !sourceOwnedPath(home, gateway) {
		return nil, errors.New("redirected Gateway")
	}
	if !p.sharedTools {
		if info, err := os.Stat(gateway); err == nil && info.IsDir() {
			for _, name := range rule.GatewayFiles {
				data, err := sourceReadFile(filepath.Join(gateway, name), 4*1024*1024)
				if err == nil && utf8.Valid(data) && sourceGatewayReference(string(data), home, rule) {
					p.paths = append(p.paths, gateway)
					break
				}
			}
		}
		if roaming != "" {
			for _, name := range rule.GatewayFiles {
				path := filepath.Join(roaming, filepath.FromSlash("Microsoft/Windows/Start Menu/Programs/Startup"), name)
				data, err := sourceReadFile(path, 4*1024*1024)
				if err != nil || !utf8.Valid(data) || !sourceGatewayReference(string(data), home, rule) {
					continue
				}
				artifact, err := captureInstallationArtifact(path)
				if err != nil || artifact.identity == nil {
					return nil, errors.New("unverified Gateway Startup entry")
				}
				p.startup = append(p.startup, artifact)
			}
		}
	}
	slices.Sort(p.paths)
	p.paths = slices.Compact(p.paths)
	for _, path := range p.paths {
		if !sourceOwnedPath(home, path) {
			return nil, errors.New("redirected program artifact")
		}
		for _, keep := range rule.Preserve {
			keep = Norm(join(keep))
			pathNorm := Norm(path)
			if atOrUnder(pathNorm, keep) || atOrUnder(keep, pathNorm) {
				return nil, errors.New("preserved installation target overlap")
			}
		}
	}
	return p, nil
}

func (p *sourceInstallPlan) observedPaths() []string {
	paths := append([]string(nil), p.paths...)
	for _, entry := range p.startup {
		paths = append(paths, entry.path)
	}
	return paths
}

func (p *sourceInstallPlan) recheckShared() error {
	if sourceOtherEntries(filepath.Join(p.home, p.rule.Profiles), "") || !p.sharedTools && sourceOtherEntries(filepath.Join(p.home, p.rule.StateDir), p.state) {
		return errors.New("installation dependencies gained another owner")
	}
	return nil
}
