package platform

import (
	"bufio"
	"errors"
	"io"
	"net"
	"os/exec"
	"reflect"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func testFanSession(set func(FanMode) error) *fanSession {
	return &fanSession{set: set, hash: func() (string, bool) { return strings.Repeat("a", 40), true }, sleep: func(time.Duration) {}, interval: time.Hour}
}

func TestFanHelperCommandSurface(t *testing.T) {
	calls := 0
	s := testFanSession(func(FanMode) error { calls++; return nil })
	for _, line := range []string{"", "pct", "pct ", "pct 0", "pct 30", "pct 101", "pct 256", "pct -1", "pct 60 extra", "pct60", "PCT 60", "auto; rm -rf /", "/bin/sh", "version extra", "pct ++60", "pct  60", "pct ６０", "pct 60\x00"} {
		if reply := s.handle(line); !strings.HasPrefix(reply, "err ") {
			t.Errorf("accepted %q: %q", line, reply)
		}
	}
	if calls != 0 || s.target.Kind != FanAuto {
		t.Fatal("invalid input reached hardware")
	}
	if got := s.handle("version"); got != fanHandshake(strings.Repeat("a", 40))+"\n" {
		t.Fatal(got)
	}
	if calls != 0 {
		t.Fatal("handshake touched hardware")
	}
	s.hash = func() (string, bool) { return "", false }
	if !strings.HasPrefix(s.handle("version"), "err ") {
		t.Fatal("unsigned helper accepted")
	}
	for _, item := range []struct {
		line string
		mode FanMode
	}{
		{"auto", FanMode{Kind: FanAuto}}, {"pct 60", FanMode{Kind: FanPercent, Percent: 60}},
		{"pct 100", FanMode{Kind: FanPercent, Percent: 100}}, {"pct +60", FanMode{Kind: FanPercent, Percent: 60}},
	} {
		if got := s.handle(item.line); got != "ok\n" || s.target != item.mode {
			t.Fatalf("%q: %q, %+v", item.line, got, s.target)
		}
	}
	for _, p := range []uint8{0, 30, 60, 100, 255} {
		want := "pct 100\n"
		if p == 60 {
			want = "pct 60\n"
		}
		if got := fanCommandLine(FanMode{Kind: FanPercent, Percent: p}); got != want {
			t.Fatal(got)
		}
	}
	if fanCommandLine(FanMode{}) != "auto\n" {
		t.Fatal("auto command")
	}
}

func TestFanHelperHandshakeUpgrade(t *testing.T) {
	expected := fanHandshake(strings.Repeat("a", 40))
	if err := fanClassifyHandshake(expected, expected); err != nil {
		t.Fatal(err)
	}
	for _, stale := range []string{"ok 0.0.16 " + strings.Repeat("a", 40), fanHandshake(strings.Repeat("b", 40)), "err unknown command", "ok"} {
		var fanErr FanError
		if err := fanClassifyHandshake(stale, expected); !errors.As(err, &fanErr) || fanErr.Kind != FanNeedsUpgrade {
			t.Fatalf("%q: %v", stale, err)
		}
	}
}

func TestFanHelperResetRetriesBeforeClearingTarget(t *testing.T) {
	manual := FanMode{Kind: FanPercent, Percent: 60}
	var calls []FanMode
	s := testFanSession(nil)
	s.target = manual
	sleeps := 0
	expectedTarget := manual
	s.sleep = func(time.Duration) {
		sleeps++
		if s.target != expectedTarget {
			t.Fatal("cleared manual state before hardware reset succeeded")
		}
	}
	s.set = func(mode FanMode) error {
		calls = append(calls, mode)
		if len(calls) <= 3 {
			return errors.New("write failed\nnot another reply")
		}
		return nil
	}
	if got := s.handle("pct 100"); !strings.HasPrefix(got, "err ") || strings.Count(got, "\n") != 1 {
		t.Fatal(got)
	}
	want := []FanMode{{Kind: FanPercent, Percent: 100}, {}, {}, {}}
	if !reflect.DeepEqual(calls, want) || sleeps != 2 || s.target.Kind != FanAuto {
		t.Fatalf("calls=%v sleeps=%d target=%v", calls, sleeps, s.target)
	}
	calls = nil
	expectedTarget = FanMode{Kind: FanAuto}
	if got := s.handle("auto"); got != "ok\n" {
		t.Fatalf("successful retry of Auto must be success: %q", got)
	}
}

func waitFanSession(t *testing.T, done <-chan struct{}) {
	t.Helper()
	select {
	case <-done:
	case <-time.After(2 * time.Second):
		t.Fatal("session did not finish")
	}
}

func TestFanHelperSessionDisconnectAndOversizeRestore(t *testing.T) {
	for _, reason := range []string{"eof", "oversize", "write failure"} {
		t.Run(reason, func(t *testing.T) {
			var calls []FanMode
			s := testFanSession(func(mode FanMode) error { calls = append(calls, mode); return nil })
			client, server := net.Pipe()
			defer client.Close()
			done := make(chan struct{})
			go func() { defer close(done); s.serve(server, func() bool { return true }, func() bool { return false }) }()
			if got, err := fanRequestLine(client, "pct 60\n"); err != nil || got != "ok" {
				t.Fatalf("%q %v", got, err)
			}
			switch reason {
			case "oversize":
				_, _ = io.WriteString(client, strings.Repeat("x", fanMaxLine+1))
			case "write failure":
				_, _ = io.WriteString(client, "version\n")
				_ = client.Close()
			default:
				_ = client.Close()
			}
			waitFanSession(t, done)
			want := []FanMode{{Kind: FanPercent, Percent: 60}, {Kind: FanAuto}}
			if !reflect.DeepEqual(calls, want) {
				t.Fatalf("%v", calls)
			}
		})
	}
}

func TestFanHelperSessionFragmentedAndPipelinedLines(t *testing.T) {
	var calls []FanMode
	s := testFanSession(func(mode FanMode) error { calls = append(calls, mode); return nil })
	client, server := net.Pipe()
	defer client.Close()
	done := make(chan struct{})
	go func() { defer close(done); s.serve(server, func() bool { return true }, func() bool { return false }) }()
	if err := client.SetDeadline(time.Now().Add(time.Second)); err != nil {
		t.Fatal(err)
	}
	for _, fragment := range []string{"pct ", "60\nversion\nauto\n"} {
		if _, err := io.WriteString(client, fragment); err != nil {
			t.Fatal(err)
		}
	}
	reader := bufio.NewReader(client)
	for _, want := range []string{"ok\n", fanHandshake(strings.Repeat("a", 40)) + "\n", "ok\n"} {
		if got, err := reader.ReadString('\n'); err != nil || got != want {
			t.Fatalf("got=%q want=%q err=%v", got, want, err)
		}
	}
	_ = client.Close()
	waitFanSession(t, done)
	want := []FanMode{{Kind: FanPercent, Percent: 60}, {}, {}}
	if !reflect.DeepEqual(calls, want) {
		t.Fatalf("hardware calls=%v", calls)
	}
}

func TestFanHelperSessionReassertFailureAndConsoleRevocation(t *testing.T) {
	for _, reason := range []string{"reassert failure", "console switch", "shutdown"} {
		t.Run(reason, func(t *testing.T) {
			var authorized atomic.Bool
			authorized.Store(true)
			var stopped atomic.Bool
			var calls []FanMode
			manualCalls := 0
			s := testFanSession(func(mode FanMode) error {
				calls = append(calls, mode)
				if mode.Kind == FanPercent {
					manualCalls++
					if manualCalls == 2 && reason == "reassert failure" {
						return errors.New("SMC unavailable")
					}
				}
				return nil
			})
			s.interval = 10 * time.Millisecond
			client, server := net.Pipe()
			defer client.Close()
			done := make(chan struct{})
			go func() { defer close(done); s.serve(server, authorized.Load, stopped.Load) }()
			if got, err := fanRequestLine(client, "pct 60\n"); err != nil || got != "ok" {
				t.Fatalf("%q %v", got, err)
			}
			if reason == "console switch" {
				authorized.Store(false)
			}
			if reason == "shutdown" {
				stopped.Store(true)
				_ = server.Close()
			}
			waitFanSession(t, done)
			if len(calls) < 2 || calls[len(calls)-1].Kind != FanAuto {
				t.Fatalf("no reset: %v", calls)
			}
			if reason == "reassert failure" && manualCalls != 2 {
				t.Fatalf("reassert not attempted: %v", calls)
			}
		})
	}
}

func TestFanHelperRejectsUnauthorizedSessionWithoutHardware(t *testing.T) {
	s := testFanSession(func(FanMode) error { t.Error("unauthorized peer touched SMC"); return nil })
	client, server := net.Pipe()
	defer client.Close()
	s.serve(server, func() bool { return false }, func() bool { return false })
}

func TestFanHelperClientRetriesOnlyTransportAndKeepsSession(t *testing.T) {
	for _, reply := range []string{"ok\n", "err no fan\n"} {
		t.Run(strings.TrimSpace(reply), func(t *testing.T) {
			client, dead := net.Pipe()
			_ = dead.Close()
			c := fanHelperClient{conn: client}
			defer c.drop()
			connects := 0
			connect := func() (net.Conn, error) {
				connects++
				client, server := net.Pipe()
				go func() {
					defer server.Close()
					r := bufio.NewReader(server)
					for {
						if _, err := r.ReadString('\n'); err != nil {
							return
						}
						if _, err := io.WriteString(server, reply); err != nil {
							return
						}
					}
				}()
				return client, nil
			}
			for i := 0; i < 2; i++ {
				err := c.control(FanMode{Kind: FanPercent, Percent: 60}, connect)
				if (err == nil) != (reply == "ok\n") {
					t.Fatalf("reply %q, err %v", reply, err)
				}
			}
			if connects != 1 {
				t.Fatalf("connected %d times; protocol errors must not retry", connects)
			}
		})
	}
	var c fanHelperClient
	connects := 0
	err := c.control(FanMode{}, func() (net.Conn, error) { connects++; a, b := net.Pipe(); _ = b.Close(); return a, nil })
	if err == nil || connects != 2 || c.conn != nil {
		t.Fatalf("retry bound: connects=%d err=%v", connects, err)
	}
	connects = 0
	err = c.control(FanMode{Kind: FanPercent, Percent: 30}, func() (net.Conn, error) { connects++; return nil, nil })
	if err == nil || connects != 0 {
		t.Fatal("unsupported mode connected")
	}
}

func TestFanHelperReplyBounds(t *testing.T) {
	for _, reply := range []string{"ok", strings.Repeat("x", fanMaxLine*4) + "\n", "  ok \r\n"} {
		client, server := net.Pipe()
		go func() {
			defer server.Close()
			_, _ = bufio.NewReader(server).ReadString('\n')
			_, _ = io.WriteString(server, reply)
		}()
		got, err := fanRequestLine(client, "auto\n")
		_ = client.Close()
		if reply == "  ok \r\n" {
			if err != nil || got != "ok" {
				t.Fatal(got, err)
			}
		} else if err == nil {
			t.Fatalf("accepted incomplete/oversized reply: %q", reply)
		}
	}
}

func TestFanHelperInstallSecurityOrdering(t *testing.T) {
	verify, err := fanVerifySnippet(fanHelperStage, strings.Repeat("a", 40))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(verify, "codesign --verify --strict") || !strings.Contains(verify, "CDHash=") || !strings.Contains(verify, "rm -f '"+fanHelperStage+"'") || strings.Contains(verify, "rm -f '"+fanHelperPath+"'") {
		t.Fatal("signature gate must verify content AND image identity, deleting only stage")
	}
	last := -1
	for _, fragment := range []string{"umask 077", "chown root:wheel", "chmod 755", "[ -L ", "ulimit -f", "cp -P -f", "[ -L ", "verifyCmd &", "chmod 544", "mv -f", "--fanhelper-install"} {
		pos := strings.Index(fanInstallScript[last+1:], fragment)
		if pos < 0 {
			t.Fatalf("missing or out of order: %q", fragment)
		}
		last += pos + 1
	}
	for _, bad := range []string{"", "xyz", strings.Repeat("A", 40), strings.Repeat("a", 39), strings.Repeat("a", 40) + "'; /bin/sh"} {
		if _, err := fanVerifySnippet(fanHelperStage, bad); err == nil {
			t.Fatalf("accepted hash %q", bad)
		}
	}
}

func TestFanHelperShellQuoteAndAppleScriptErrors(t *testing.T) {
	t.Run("shell quoting", func(t *testing.T) {
		if _, err := exec.LookPath("/bin/sh"); err != nil {
			t.Skip("POSIX shell unavailable")
		}
		for _, raw := range []string{"", "a'b", "$(touch /not-executed); \n\""} {
			out, err := exec.Command("/bin/sh", "-c", "printf '%s' "+fanShellQuote(raw)).Output()
			if err != nil || string(out) != raw {
				t.Fatalf("%q: %q %v", raw, out, err)
			}
		}
	})
	for _, item := range []struct {
		input    string
		kind     FanErrorKind
		contains string
	}{
		{"execution error: User canceled. (-128)", FanCanceled, ""},
		{"codesign: signature-invalid", FanOther, "signature verification failed"},
		{"cdhash-mismatch", FanOther, "restart"},
		{"source-is-symlink", FanOther, "symbolic link"},
		{" permission denied \n", FanOther, "permission denied"},
	} {
		err := fanAppleScriptError(item.input)
		var fanErr FanError
		if !errors.As(err, &fanErr) || fanErr.Kind != item.kind || !strings.Contains(err.Error(), item.contains) {
			t.Fatalf("%q: %v", item.input, err)
		}
	}
}
