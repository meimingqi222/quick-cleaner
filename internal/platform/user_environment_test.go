package platform

import (
	"os"
	"path/filepath"
	"testing"
)

func TestOriginalUserArgumentContract(t *testing.T) {
	for _, tc := range []struct {
		args []string
		want originalUser
	}{
		{[]string{"app", "--orig-user-home"}, originalUser{}},
		{[]string{"app", "--orig-user-home=x", "--orig-user-sid=y"}, originalUser{}},
		{[]string{"app", "--orig-user-home", "a", "--orig-user-home", "b", "--orig-user-sid", "one", "--orig-user-sid", "two"}, originalUser{"b", "two", true, true}},
		{[]string{"app", "--orig-user-home", "--orig-user-sid", "value"}, originalUser{home: "--orig-user-sid", hasHome: true}},
		{[]string{"app", "--orig-user-home", "a", "--orig-user-home"}, originalUser{home: "a", hasHome: true}},
		{[]string{"app", "--orig-user-sid", ""}, originalUser{sid: "", hasSID: true}},
	} {
		if got := parseOriginalUser(tc.args); got != tc.want {
			t.Errorf("%q: got %+v want %+v", tc.args, got, tc.want)
		}
	}
}

func TestOriginalHomeTrustContract(t *testing.T) {
	base := t.TempDir()
	file := filepath.Join(base, "existing-file")
	if err := os.WriteFile(file, nil, 0600); err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{base, file, filepath.Join(base, "missing"), ""} {
		got, ok := originalHome(originalUser{home: path, hasHome: true}, func() (string, bool) { t.Fatal("explicit home used administrator fallback"); return "", false })
		wantOK := path == base || path == file
		if ok != wantOK || (ok && got != path) {
			t.Errorf("%q: %q %v", path, got, ok)
		}
	}
}
