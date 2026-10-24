package platform

import (
	"bytes"
	"errors"
	"fmt"
	"net"
	"os"
	"os/exec"
	"os/signal"
	"strings"
	"sync"
	"sync/atomic"
	"syscall"
	"time"

	"golang.org/x/sys/unix"
)

func FanHelperInstalled() bool {
	_, helperErr := os.Stat(fanHelperPath)
	_, plistErr := os.Stat(fanPlistPath)
	return helperErr == nil && plistErr == nil
}

func fanConnect() (net.Conn, error) {
	if !FanHelperInstalled() {
		return nil, FanError{Kind: FanNotInstalled}
	}
	conn, err := net.DialTimeout("unix", fanSocketPath, fanClientTimeout)
	if err != nil {
		return nil, fanOther("fan helper installed but connection failed: %v", err)
	}
	hash, signed := fanSelfCDHash()
	if !signed {
		_ = conn.Close()
		return nil, FanError{Kind: FanNotInstalled}
	}
	reply, err := fanRequestLine(conn, "version\n")
	if err == nil {
		err = fanClassifyHandshake(reply, fanHandshake(hash))
	}
	if err != nil {
		_ = conn.Close()
		return nil, err
	}
	return conn, nil
}

func ElevatedFanControl(mode FanMode) error { return fanClient.control(mode, fanConnect) }

func fanRunAppleScript(script string, args ...string) error {
	cmd := exec.Command("/usr/bin/osascript", append([]string{"-"}, args...)...)
	cmd.Stdin = strings.NewReader(script)
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	if err := cmd.Run(); err != nil {
		if stderr.Len() == 0 {
			return fanOther("osascript: %v", err)
		}
		return fanAppleScriptError(stderr.String())
	}
	return nil
}

func InstallFanHelper(prompt string) error {
	fanClient.drop()
	exe, err := os.Executable()
	if err != nil {
		return fanOther("locate executable: %v", err)
	}
	hash, ok := fanSelfCDHash()
	if !ok {
		return fanOther("installation requires a code signature; sign the executable with codesign -s -")
	}
	verify, err := fanVerifySnippet(fanHelperStage, hash)
	if err != nil {
		return err
	}
	if err := fanRunAppleScript(fanInstallScript, exe, fanHelperPath, fanHelperStage, fanHelperDir, verify, prompt); err != nil {
		return err
	}
	var last error = fanOther("fan helper not ready within 6 seconds")
	for i := 0; i < 60; i++ {
		conn, err := fanConnect()
		if err == nil {
			_ = conn.Close()
			return nil
		}
		last = err
		time.Sleep(100 * time.Millisecond)
	}
	return last
}

func UninstallFanHelper(prompt string) error {
	fanClient.drop()
	return fanRunAppleScript(fanUninstallScript, fanHelperLabel, fanHelperPath, fanPlistPath, fanSocketPath, prompt)
}

func fanConsoleUID() (uint32, bool) {
	var st unix.Stat_t
	if unix.Stat("/dev/console", &st) != nil || st.Uid == 0 {
		return 0, false
	}
	return st.Uid, true
}

func fanRestrictSocket() error {
	uid, ok := fanConsoleUID()
	if !ok {
		uid = 0
	}
	// Reset to root when no console user exists; never leave the last user
	// owning the socket after logout.
	if err := os.Chown(fanSocketPath, int(uid), -1); err != nil {
		return err
	}
	return os.Chmod(fanSocketPath, 0600)
}

func fanPeerUID(conn *net.UnixConn) (uint32, error) {
	raw, err := conn.SyscallConn()
	if err != nil {
		return 0, err
	}
	var uid uint32
	var peerErr error
	err = raw.Control(func(fd uintptr) {
		cred, e := unix.GetsockoptXucred(int(fd), unix.SOL_LOCAL, unix.LOCAL_PEERCRED)
		if e != nil {
			peerErr = e
			return
		}
		// XNU bsd/sys/ucred.h: XUCRED_VERSION=0 (not exported by x/sys).
		if cred.Version != 0 {
			peerErr = errors.New("unexpected peer credential version")
			return
		}
		uid = cred.Uid
	})
	if err != nil {
		return 0, err
	}
	return uid, peerErr
}

// RunFanHelperDaemon is the root-only --fanhelper entry, before any GUI setup.
func RunFanHelperDaemon() error {
	if os.Geteuid() != 0 {
		return FanError{Kind: FanNeedsRoot, Detail: "fan helper must run as root via launchd"}
	}
	var stopped atomic.Bool
	signals := make(chan os.Signal, 1)
	signal.Notify(signals, syscall.SIGTERM, syscall.SIGINT)
	defer signal.Stop(signals)
	var mu sync.Mutex
	var listener *net.UnixListener
	var active *net.UnixConn
	done := make(chan struct{})
	defer close(done)
	go func() {
		select {
		case <-signals:
			stopped.Store(true)
			mu.Lock()
			if listener != nil {
				_ = listener.Close()
			}
			if active != nil {
				_ = active.Close()
			}
			mu.Unlock()
		case <-done:
		}
	}()
	session := fanSession{set: SetFanMode, hash: fanSelfCDHash, sleep: time.Sleep, interval: fanReassert}
	session.restore()
	if stopped.Load() {
		return nil
	}
	if err := os.Remove(fanSocketPath); err != nil && !os.IsNotExist(err) {
		return err
	}
	// This entry is a dedicated process; a restrictive process-wide umask is
	// intentional, so no goroutine can create a world-connectable socket.
	unix.Umask(0177)
	bound, err := net.ListenUnix("unix", &net.UnixAddr{Name: fanSocketPath, Net: "unix"})
	if err != nil {
		return fanOther("listen on fan socket: %v", err)
	}
	// Closing Accept on SIGTERM must not unlink the readiness marker before
	// reset retries finish: an upgrade waits for this socket to disappear.
	bound.SetUnlinkOnClose(false)
	defer func() {
		session.restore()
		_ = bound.Close()
		_ = os.Remove(fanSocketPath)
	}()
	mu.Lock()
	listener = bound
	mu.Unlock()
	for !stopped.Load() {
		if err := fanRestrictSocket(); err != nil {
			return fanOther("restrict fan socket: %v", err)
		}
		// Refresh ownership even with no pending connection (fast user switch).
		_ = bound.SetDeadline(time.Now().Add(fanReassert))
		conn, err := bound.AcceptUnix()
		if err != nil {
			if stopped.Load() {
				break
			}
			var timeout net.Error
			if errors.As(err, &timeout) && timeout.Timeout() {
				continue
			}
			return err
		}
		mu.Lock()
		active = conn
		stopping := stopped.Load()
		mu.Unlock()
		if stopping {
			_ = conn.Close()
			break
		}
		uid, err := fanPeerUID(conn)
		if err == nil {
			session.serve(conn, func() bool { current, ok := fanConsoleUID(); return ok && current == uid }, stopped.Load)
		} else {
			_ = conn.Close()
		}
		mu.Lock()
		active = nil
		mu.Unlock()
	}
	return nil
}

func fanSecureLaunchdDirectory() error {
	st, err := os.Lstat(fanLaunchdDir)
	if err != nil {
		return err
	}
	if !st.IsDir() || st.Mode()&os.ModeSymlink != 0 {
		return errors.New("launchd directory must be a real directory")
	}
	if err := os.Chown(fanLaunchdDir, 0, 0); err != nil {
		return err
	}
	return os.Chmod(fanLaunchdDir, 0755)
}

func fanBootoutExistingJob() {
	_ = exec.Command("/bin/launchctl", "bootout", "system/"+fanHelperLabel).Run()
	for i := 0; i < 80; i++ {
		loaded := exec.Command("/bin/launchctl", "print", "system/"+fanHelperLabel).Run() == nil
		_, socketErr := os.Lstat(fanSocketPath)
		if !loaded && os.IsNotExist(socketErr) {
			return
		}
		time.Sleep(100 * time.Millisecond)
	}
}

// RunFanHelperInstallStep writes only the embedded plist, never a user-supplied
// path or body. A fresh exclusive temp file also rejects stale plist symlinks.
func RunFanHelperInstallStep() error {
	if os.Geteuid() != 0 {
		return FanError{Kind: FanNeedsRoot, Detail: "fan helper installation must run as root"}
	}
	if err := fanSecureLaunchdDirectory(); err != nil {
		return fanOther("secure launchd directory: %v", err)
	}
	f, err := os.CreateTemp(fanLaunchdDir, ".quickcleaner-fanhelper-*.plist")
	if err != nil {
		return err
	}
	defer os.Remove(f.Name())
	_, writeErr := f.WriteString(fanPlistBody)
	if writeErr == nil {
		writeErr = f.Chown(0, 0)
	}
	if writeErr == nil {
		writeErr = f.Chmod(0644)
	}
	if writeErr == nil {
		writeErr = f.Sync()
	}
	closeErr := f.Close()
	if writeErr != nil {
		return writeErr
	}
	if closeErr != nil {
		return closeErr
	}
	if err := os.Rename(f.Name(), fanPlistPath); err != nil {
		return err
	}
	fanBootoutExistingJob()
	out, err := exec.Command("/bin/launchctl", "bootstrap", "system", fanPlistPath).CombinedOutput()
	if err != nil {
		return fmt.Errorf("fan helper launchctl bootstrap: %w: %s", err, strings.TrimSpace(string(out)))
	}
	return nil
}
