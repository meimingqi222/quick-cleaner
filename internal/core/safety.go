package core

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"unicode"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// Norm is the shared lexical comparison form; it deliberately does not resolve dots or links.
func Norm(s string) string {
	var b strings.Builder
	for _, r := range s {
		switch r {
		case '/':
			b.WriteByte('\\')
		case 'İ':
			b.WriteString("i\u0307")
		default:
			b.WriteRune(unicode.ToLower(r))
		}
	}
	return strings.TrimRight(b.String(), `\`)
}

// AtOrUnder reports whether s is base or a descendant in Norm form.
func AtOrUnder(s, base string) bool { return atOrUnder(s, base) }

func atOrUnder(s, base string) bool {
	return strings.HasPrefix(s, base) && (len(s) == len(base) || s[len(base)] == '\\')
}
func member(s string, list []string) bool {
	for _, v := range list {
		if s == v {
			return true
		}
	}
	return false
}
func beneath(s string, list []string) bool {
	for _, v := range list {
		if atOrUnder(s, v) {
			return true
		}
	}
	return false
}

var driveExact = []string{`\program files`, `\program files (x86)`, `\programdata`, `\users`}
var macPrefixes = []string{`\system`, `\library`, `\applications`, `\bin`, `\sbin`, `\usr`, `\private\etc`, `\private\var\db`, `\private\var\root`, `\private\var\vm`, `\private\var\protected`}
var homeExact = []string{"", `\desktop`, `\documents`, `\downloads`, `\pictures`, `\videos`, `\music`}
var profileExact = []string{`\appdata`, `\appdata\local`, `\appdata\locallow`, `\appdata\roaming`}

// safetyPolicy captures platform facts once, independently of the host running path-policy tests.
type safetyPolicy struct {
	goos, windows, home, processHome, public string
	known                                    []string
}

func newSafetyPolicy(goos, windows, home, processHome, public string, known []string) safetyPolicy {
	p := safetyPolicy{goos: goos, windows: Norm(windows), home: Norm(home), processHome: Norm(processHome), public: Norm(public)}
	for _, k := range known {
		p.known = append(p.known, Norm(k))
	}
	return p
}

var policyOnce sync.Once
var policy safetyPolicy

func defaultPolicy() safetyPolicy {
	policyOnce.Do(func() {
		home, _ := platform.UserHome()
		windows, ok := os.LookupEnv("SystemRoot")
		if !ok {
			windows = `C:\Windows`
		}
		processHome := ""
		public := ""
		var known []string
		if runtime.GOOS == "windows" {
			processHome = home
			public = os.Getenv("PUBLIC")
			known = platform.KnownFolders()
		}
		// Linux is a migration-test host, not a supported cleaner product platform.
		policy = newSafetyPolicy(runtime.GOOS, windows, home, processHome, public, known)
	})
	return policy
}
func asciiLower(s string) string {
	return strings.Map(func(r rune) rune {
		if r >= 'A' && r <= 'Z' {
			return r + ('a' - 'A')
		}
		return r
	}, s)
}
func IsNTFSMetaName(name string) bool {
	name = asciiLower(name)
	return strings.HasPrefix(name, "$mft") || strings.HasPrefix(name, "dumpstack.log") || member(name, []string{"$logfile", "$volume", "$attrdef", "$bitmap", "$boot", "$badclus", "$secure", "$upcase", "$extend", "pagefile.sys", "hiberfil.sys", "swapfile.sys", "system volume information"})
}
func (p safetyPolicy) systemRoot(path string) bool {
	n := Norm(path)
	return len(n) <= 3 || (p.goos == "darwin" && beneath(n, macPrefixes)) || n == p.windows || (len(n) >= 2 && member(n[2:], driveExact))
}
func IsSystemRootDir(path string) bool         { return defaultPolicy().systemRoot(path) }
func IsProtectedResidualPath(path string) bool { return IsSystemRootDir(path) || IsProtected(path) }
func IsProtected(path string) bool             { return defaultPolicy().protected(path, false) }
func IsContentsProtected(path string) bool     { return defaultPolicy().protected(path, true) }
func (p safetyPolicy) protected(path string, contents bool) bool {
	if IsManagedAgentWorktree(path) {
		return true
	}
	n := Norm(path)
	if len(n) <= 3 || IsWhitelisted(path) {
		return true
	}
	if p.goos == "darwin" && (beneath(n, macPrefixes) || (p.home != "" && member(n, []string{p.home, p.home + `\library`, p.home + `\library\application support`}))) {
		return true
	}
	if IsNTFSMetaName(n[strings.LastIndex(n, `\`)+1:]) {
		return true
	}
	if atOrUnder(n, p.windows) {
		rest := n[len(p.windows):]
		if rest == "" || (rest == `\temp` && !contents) || beneath(rest, []string{`\system32`, `\syswow64`, `\winsxs`, `\boot`, `\fonts`, `\servicing`}) {
			return true
		}
	}
	if len(n) >= 2 && (member(n[2:], driveExact) || beneath(n[2:], []string{`\system volume information`, `\$windows.~bt`, `\recovery`})) {
		return true
	}
	for _, h := range []string{p.home, p.processHome} {
		if h != "" && atOrUnder(n, h) && member(n[len(h):], homeExact) {
			return true
		}
	}
	if p.public != "" && atOrUnder(n, p.public) && (member(n[len(p.public):], homeExact) || n == p.public+`\libraries`) {
		return true
	}
	if member(n, p.known) {
		return true
	}
	for _, suffix := range profileExact {
		if strings.HasSuffix(n, suffix) {
			root := strings.TrimSuffix(n, suffix)
			if len(root) >= 2 && strings.HasPrefix(root[2:], `\users\`) {
				name := root[9:]
				if name != "" && !strings.Contains(name, `\`) {
					return true
				}
			}
		}
	}
	return false
}
func IsDevelopmentInstallationPath(path string) bool {
	return developmentInstallationPath(path, os.Stat)
}

// developmentInstallationPath is the install-ownership check. stat is os.Stat
// for callers, or a scan-local cache: one discovery walk otherwise stats the
// same ancestor files once per candidate.
func developmentInstallationPath(path string, stat func(string) (os.FileInfo, error)) bool {
	n := strings.ReplaceAll(Norm(path), `\`, "/") + "/"
	for _, part := range strings.Split(n, "/") {
		if strings.HasSuffix(part, ".app") || part == "appdata" {
			return true
		}
	}
	for _, prefix := range []string{"/library/application support/", "/library/extensions/", "/.nvm/", "/.fnm/", "/.volta/", "/.asdf/installs/", "/mise/installs/", "/.pyenv/versions/", "/.bun/install/", "/.local/share/uv/tools/", "/.local/share/pipx/", "/.local/pipx/", "/.conda/envs/", "/miniconda3/", "/anaconda3/", "/miniforge3/", "/.workbuddy/binaries/", "/lib/node_modules", "/pnpm/global/", "/.npm-global/"} {
		if strings.Contains(n, prefix) {
			return true
		}
	}
	for a := filepath.Dir(path); a != path; path, a = a, filepath.Dir(a) {
		if strings.EqualFold(filepath.Base(a), "node_modules") {
			return true
		}
		for _, m := range []string{"conda-meta", "pipx_metadata.json", "pyvenv.cfg"} {
			info, err := stat(filepath.Join(a, m))
			if err == nil && ((m == "conda-meta" && info.IsDir()) || (m != "conda-meta" && info.Mode().IsRegular())) {
				return true
			}
		}
	}
	return false
}
func IsManagedAgentWorktree(path string) bool {
	for {
		if strings.EqualFold(filepath.Base(path), "subagent-worktrees") {
			parent := filepath.Dir(path)
			if parent == path {
				return true
			}
			for _, m := range []string{".maka-storage-root.json", ".maka-host-composition.json", "runtime.sqlite"} {
				if _, err := os.Lstat(filepath.Join(parent, m)); !os.IsNotExist(err) {
					return true
				}
			}
			return false
		}
		parent := filepath.Dir(path)
		if parent == path {
			return false
		}
		path = parent
	}
}

var elevatedParents = []string{`\library\application support`, `\library\application scripts`, `\library\caches`, `\library\logs`, `\library\preferences`, `\library\launchagents`, `\library\launchdaemons`, `\library\privilegedhelpertools`, `\private\var\db\receipts`}

func (p safetyPolicy) elevated(path string) bool {
	if p.goos != "darwin" {
		return false
	}
	if info, err := os.Lstat(path); err == nil && info.Mode()&os.ModeSymlink != 0 {
		return false
	}
	// The macOS lexical grammar is used even on the migration test host.
	s := strings.TrimRight(path, "/")
	i := strings.LastIndex(s, "/")
	if i < 0 {
		return false
	}
	name := asciiLower(s[i+1:])
	if name == "com.apple" || strings.HasPrefix(name, "com.apple.") || member(name, []string{"systemconfiguration", "crashreporter", "keychains", "security", "preferencepanes", "systemprofiler", "logs", "caches", "managedpreferences", "systemmigration", "apple"}) {
		return false
	}
	return member(Norm(s[:i]), elevatedParents)
}
func IsElevatedResidualTarget(path string) bool { return defaultPolicy().elevated(path) }
func (p safetyPolicy) underAppSupport(path string) bool {
	root := p.home + `\library\application support`
	n := Norm(path)
	return p.goos == "darwin" && p.home != "" && n != root && atOrUnder(n, root)
}
func UnderHomeAppSupport(path string) bool { return defaultPolicy().underAppSupport(path) }
