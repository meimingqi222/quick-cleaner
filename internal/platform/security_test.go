package platform

import (
	"reflect"
	"testing"
)

func TestElevationShowCmdHidesOnlyConsolePrograms(t *testing.T) {
	if elevationShowCmd(3) != 0 {
		t.Fatal("console elevation shows a window")
	}
	if elevationShowCmd(2) != 1 {
		t.Fatal("gui elevation is started hidden")
	}
}

func TestQuoteWindowsArgumentMatchesReference(t *testing.T) {
	for _, tc := range []struct{ arg, want string }{
		{"", `""`}, {"hello", "hello"}, {"--no-elevate", "--no-elevate"},
		{"hello world", `"hello world"`}, {`C:\Program Files\App`, `"C:\Program Files\App"`},
		{`a"b`, `"a\"b"`}, {`a\"b`, `"a\\\"b"`}, {`C:\dir\`, `C:\dir\`},
		{`C:\dir with spaces\`, `"C:\dir with spaces\\"`}, {"a\nb", "\"a\nb\""},
		{"a\vb", "\"a\vb\""}, {"a\tb", "\"a\tb\""}, {"中文 路径", `"中文 路径"`},
	} {
		if got := QuoteWindowsArgument(tc.arg); got != tc.want {
			t.Fatalf("%q: got %q, want %q", tc.arg, got, tc.want)
		}
	}
}

func TestElevationForwardsOriginalAccountWithoutOverwritingFlags(t *testing.T) {
	captured := originalUser{home: `C:\Users\original user`, sid: "S-1-5-21-123", hasHome: true, hasSID: true}
	for _, tc := range []struct{ args, want []string }{
		{[]string{"--scan"}, []string{"--scan", "--orig-user-home", captured.home, "--orig-user-sid", captured.sid}},
		{[]string{"--orig-user-home", "already captured"}, []string{"--orig-user-home", "already captured", "--orig-user-sid", captured.sid}},
		{[]string{"--orig-user-sid", "existing", "--orig-user-home", "existing-home"}, []string{"--orig-user-sid", "existing", "--orig-user-home", "existing-home"}},
		{[]string{"--orig-user-home"}, []string{"--orig-user-home", "--orig-user-sid", captured.sid}},
	} {
		before := append([]string(nil), tc.args...)
		if got := elevationArgs(tc.args, captured); !reflect.DeepEqual(got, tc.want) {
			t.Fatalf("%v: got %v, want %v", tc.args, got, tc.want)
		}
		if !reflect.DeepEqual(tc.args, before) {
			t.Fatal("input arguments mutated")
		}
	}
	if got := elevationArgs([]string{"--scan"}, originalUser{}); !reflect.DeepEqual(got, []string{"--scan"}) {
		t.Fatal("unknown account invented", got)
	}
}
