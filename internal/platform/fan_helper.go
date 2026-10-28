package platform

import (
	"errors"
	"fmt"
	"io"
	"net"
	"strconv"
	"strings"
	"sync"
	"time"
)

const (
	fanHelperLabel   = "com.quickcleaner.fanhelper"
	fanHelperDir     = "/Library/PrivilegedHelperTools"
	fanHelperPath    = fanHelperDir + "/" + fanHelperLabel
	fanHelperStage   = fanHelperPath + ".new"
	fanLaunchdDir    = "/Library/LaunchDaemons"
	fanPlistPath     = fanLaunchdDir + "/" + fanHelperLabel + ".plist"
	fanSocketPath    = "/var/run/" + fanHelperLabel + ".sock"
	fanMaxLine       = 64
	fanReassert      = 3 * time.Second
	fanClientTimeout = 20 * time.Second
	// The image CDHash, not a release version string, is the upgrade authority.
	fanProtocolVersion = "1"
)

func fanOther(format string, args ...any) error {
	return FanError{Kind: FanOther, Detail: fmt.Sprintf(format, args...)}
}

func fanCommandLine(mode FanMode) string {
	if mode.Kind == FanAuto {
		return "auto\n"
	}
	if mode.Kind == FanPercent && mode.Percent == 60 {
		return "pct 60\n"
	}
	return "pct 100\n"
}

func fanHandshake(hash string) string { return "ok " + fanProtocolVersion + " " + hash }

func fanClassifyHandshake(reply, expected string) error {
	if reply != expected {
		return FanError{Kind: FanNeedsUpgrade}
	}
	return nil
}

// Read one byte at a time so an unbuffered long-lived connection never loses a
// subsequent reply. Neither a missing newline nor a hostile peer can grow memory.
func fanRequestLine(conn net.Conn, line string) (string, error) {
	if err := conn.SetDeadline(time.Now().Add(fanClientTimeout)); err != nil {
		return "", err
	}
	if _, err := io.WriteString(conn, line); err != nil {
		return "", fanOther("fan helper write: %v", err)
	}
	buf := make([]byte, 0, fanMaxLine*4)
	var b [1]byte
	for len(buf) < fanMaxLine*4 {
		if _, err := io.ReadFull(conn, b[:]); err != nil {
			return "", fanOther("fan helper incomplete reply: %v", err)
		}
		if b[0] == '\n' {
			return strings.TrimSpace(string(buf)), nil
		}
		buf = append(buf, b[0])
	}
	return "", fanOther("fan helper reply exceeds limit")
}

type fanHelperClient struct {
	sync.Mutex
	conn net.Conn
}

var fanClient fanHelperClient

func (c *fanHelperClient) dropLocked() {
	if c.conn != nil {
		_ = c.conn.Close()
		c.conn = nil
	}
}

func (c *fanHelperClient) drop() {
	c.Lock()
	defer c.Unlock()
	c.dropLocked()
}

func (c *fanHelperClient) control(mode FanMode, connect func() (net.Conn, error)) error {
	if mode.Kind != FanAuto && (mode.Kind != FanPercent || mode.Percent != 60 && mode.Percent != 100) {
		return fanOther("unsupported fan mode; expected auto, 60 or 100 percent")
	}
	c.Lock()
	defer c.Unlock()
	for attempt := 0; attempt < 2; attempt++ {
		if c.conn == nil {
			conn, err := connect()
			if err != nil {
				return err
			}
			c.conn = conn
		}
		reply, err := fanRequestLine(c.conn, fanCommandLine(mode))
		if err == nil {
			if reply == "ok" {
				return nil
			}
			// Protocol errors are definitive; retrying would write to SMC twice.
			return fanOther("%s", strings.TrimPrefix(reply, "err "))
		}
		c.dropLocked()
		if attempt == 1 {
			return err
		}
	}
	return fanOther("fan helper did not respond")
}

type fanSession struct {
	set      func(FanMode) error
	hash     func() (string, bool)
	sleep    func(time.Duration)
	interval time.Duration
	target   FanMode
}

func (s *fanSession) restore() {
	// Even during shutdown a failed reset must be retried. Do not clear the
	// logical manual target until hardware has actually accepted Auto.
	for s.set(FanMode{Kind: FanAuto}) != nil {
		s.sleep(s.interval)
	}
	s.target = FanMode{Kind: FanAuto}
}

func (s *fanSession) handle(line string) string {
	if line == "version" {
		if hash, ok := s.hash(); ok {
			return fanHandshake(hash) + "\n"
		}
		return "err fan helper has no code signature\n"
	}
	mode := FanMode{Kind: FanAuto}
	if line != "auto" {
		rest, ok := strings.CutPrefix(line, "pct ")
		if !ok {
			return "err unknown command\n"
		}
		// Match Rust's unsigned decimal parser (including its optional '+').
		digits := strings.TrimPrefix(rest, "+")
		if digits == "" || strings.IndexFunc(digits, func(r rune) bool { return r < '0' || r > '9' }) >= 0 {
			return "err duty not allowed\n"
		}
		p, err := strconv.ParseUint(digits, 10, 8)
		if err != nil || p != 60 && p != 100 {
			return "err duty not allowed\n"
		}
		mode = FanMode{Kind: FanPercent, Percent: uint8(p)}
	}
	if err := s.set(mode); err != nil {
		s.restore()
		if mode.Kind == FanAuto {
			return "ok\n"
		}
		// Errors must never introduce another protocol line.
		msg := strings.NewReplacer("\n", " ", "\r", " ").Replace(err.Error())
		if len(msg) > fanMaxLine*4-6 {
			msg = msg[:fanMaxLine*4-6]
		}
		return "err " + msg + "\n"
	}
	s.target = mode
	return "ok\n"
}

func (s *fanSession) serve(conn net.Conn, authorized func() bool, stopped func() bool) {
	defer conn.Close()
	if !authorized() {
		return
	}
	defer s.restore()
	pending := make([]byte, 0, fanMaxLine)
	var chunk [fanMaxLine]byte
	next := time.Now().Add(s.interval)
	for !stopped() && authorized() {
		// An absolute tick prevents a peer trickling partial input from starving
		// the safety reassertion or the console-user check.
		if err := conn.SetReadDeadline(next); err != nil {
			return
		}
		n, err := conn.Read(chunk[:])
		if n > 0 {
			if !authorized() || stopped() {
				return
			}
			pending = append(pending, chunk[:n]...)
			if len(pending) > fanMaxLine {
				return
			}
			for {
				pos := strings.IndexByte(string(pending), '\n')
				if pos < 0 {
					break
				}
				line := strings.TrimSpace(string(pending[:pos]))
				pending = pending[pos+1:]
				reply := s.handle(line)
				if err := conn.SetWriteDeadline(time.Now().Add(s.interval)); err != nil {
					return
				}
				if _, err := io.WriteString(conn, reply); err != nil {
					return
				}
			}
		}
		if err != nil {
			var timeout net.Error
			if !errors.As(err, &timeout) || !timeout.Timeout() {
				return
			}
		}
		if !time.Now().Before(next) {
			if !authorized() || stopped() {
				return
			}
			if s.target.Kind == FanPercent && s.set(s.target) != nil {
				return
			}
			next = time.Now().Add(s.interval)
		}
	}
}

func fanShellQuote(raw string) string { return "'" + strings.ReplaceAll(raw, "'", "'\\''") + "'" }

func fanVerifySnippet(helper, hash string) (string, error) {
	if len(hash) != 40 || strings.IndexFunc(hash, func(r rune) bool { return !(r >= '0' && r <= '9' || r >= 'a' && r <= 'f') }) >= 0 {
		return "", fanOther("invalid code signature identifier")
	}
	h := fanShellQuote(helper)
	return fmt.Sprintf("if ! /usr/bin/codesign --verify --strict %s; then /bin/rm -f %s; echo signature-invalid >&2; exit 3; fi; installed=$(/usr/bin/codesign -d --verbose=4 %s 2>&1 | /usr/bin/awk -F= '/^CDHash=/{print $2}'); if [ \"$installed\" != %s ]; then /bin/rm -f %s; echo cdhash-mismatch >&2; exit 3; fi;", h, h, h, fanShellQuote(hash), h), nil
}

// Staging remains root-only until both checks pass. Reject source and staged
// links; do not trust a disk path as the identity of the running application.
const fanInstallScript = `on run argv
	set exePath to item 1 of argv
	set helperPath to item 2 of argv
	set stagePath to item 3 of argv
	set helperDir to item 4 of argv
	set verifyCmd to item 5 of argv
	set promptText to item 6 of argv
	set cmd to "set -e; umask 077; " & ¬
		"if [ -L " & quoted form of helperDir & " ]; then exit 3; fi; " & ¬
		"/bin/mkdir -p " & quoted form of helperDir & "; " & ¬
		"/usr/sbin/chown root:wheel " & quoted form of helperDir & "; " & ¬
		"/bin/chmod 755 " & quoted form of helperDir & "; " & ¬
		"if [ -L " & quoted form of exePath & " ] || [ ! -f " & quoted form of exePath & " ]; then echo source-is-symlink >&2; exit 3; fi; " & ¬
		"ulimit -f 409600; " & ¬
		"/bin/rm -f " & quoted form of stagePath & "; " & ¬
		"/bin/cp -P -f " & quoted form of exePath & " " & quoted form of stagePath & "; " & ¬
		"if [ -L " & quoted form of stagePath & " ] || [ ! -f " & quoted form of stagePath & " ]; then /bin/rm -f " & quoted form of stagePath & "; echo source-is-symlink >&2; exit 3; fi; " & ¬
		verifyCmd & " " & ¬
		"/usr/sbin/chown root:wheel " & quoted form of stagePath & "; " & ¬
		"/bin/chmod 544 " & quoted form of stagePath & "; " & ¬
		"/bin/mv -f " & quoted form of stagePath & " " & quoted form of helperPath & "; " & ¬
		quoted form of helperPath & " --fanhelper-install"
	do shell script cmd with administrator privileges with prompt promptText
end run`

const fanUninstallScript = `on run argv
	set label to item 1 of argv
	set helperPath to item 2 of argv
	set plistPath to item 3 of argv
	set sockPath to item 4 of argv
	set promptText to item 5 of argv
	set cmd to "/bin/launchctl bootout system/" & quoted form of label & " 2>/dev/null; " & ¬
		"/bin/rm -f " & quoted form of plistPath & " " & quoted form of helperPath & " " & quoted form of sockPath & "; exit 0"
	do shell script cmd with administrator privileges with prompt promptText
end run`

func fanAppleScriptError(stderr string) error {
	switch {
	case strings.Contains(stderr, "User canceled"), strings.Contains(stderr, "(-128)"):
		return FanError{Kind: FanCanceled}
	case strings.Contains(stderr, "signature-invalid"):
		return fanOther("code signature verification failed; installation aborted; download a fresh copy")
	case strings.Contains(stderr, "cdhash-mismatch"):
		return fanOther("the executable differs from the running image; installation aborted; restart after rebuilding or updating")
	case strings.Contains(stderr, "source-is-symlink"):
		return fanOther("the executable is a symbolic link or not a regular file; installation aborted")
	default:
		return fanOther("%s", strings.TrimSpace(stderr))
	}
}

const fanPlistBody = `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>com.quickcleaner.fanhelper</string>
<key>ProgramArguments</key><array>
<string>/Library/PrivilegedHelperTools/com.quickcleaner.fanhelper</string>
<string>--fanhelper</string></array>
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><true/>
</dict></plist>
`
