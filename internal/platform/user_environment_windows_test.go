package platform

import (
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
)

type envPath struct {
	Path string
	OK   bool
}

func TestPublicWindowsEnvironment(t *testing.T) {
	if os.Getenv("QC_ENV_CHILD") == "1" {
		var result []envPath
		for _, f := range []func() (string, bool){UserHome, UserCacheDir, UserDataDir, UserTempDir, UserSID} {
			p, ok := f()
			result = append(result, envPath{p, ok})
		}
		json.NewEncoder(os.Stdout).Encode(result)
		os.Exit(0)
	}
	home := t.TempDir()
	file := filepath.Join(home, "file")
	if err := os.WriteFile(file, nil, 0600); err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{home, file, filepath.Join(home, "missing"), ""} {
		cmd := exec.Command(os.Args[0], "-test.run=^TestPublicWindowsEnvironment$", "--", "--orig-user-home", "ignored", "--orig-user-home", path, "--orig-user-sid", "verbatim")
		cmd.Env = append(os.Environ(), "QC_ENV_CHILD=1", "APPDATA=wrong", "LOCALAPPDATA=wrong", "TEMP=wrong", "USERPROFILE="+home)
		out, err := cmd.Output()
		if err != nil {
			t.Fatal(err)
		}
		var got []envPath
		if err := json.Unmarshal(out, &got); err != nil {
			t.Fatalf("%s: %v", out, err)
		}
		valid := path == home || path == file
		for i, suffix := range []string{"", filepath.Join("AppData", "Local"), filepath.Join("AppData", "Roaming"), filepath.Join("AppData", "Local", "Temp")} {
			want := ""
			if valid {
				want = filepath.Join(path, suffix)
			}
			if got[i] != (envPath{want, valid}) {
				t.Fatalf("%q field %d: %+v", path, i, got[i])
			}
		}
		if got[4] != (envPath{"verbatim", true}) {
			t.Fatal(got[4])
		}
	}
}

func TestNativeLanguageSelection(t *testing.T) {
	for _, tc := range []struct {
		user, system uintptr
		want         string
	}{{0x804, 0, "zh"}, {0x404, 0, "zh"}, {0x409, 0x804, "en"}, {0, 0xc04, "zh"}, {0, 0, "en"}} {
		if got := uiLanguageTag(func() uintptr { return tc.user }, func() uintptr {
			if tc.user != 0 {
				t.Fatal("unnecessary fallback")
			}
			return tc.system
		}); got != tc.want {
			t.Fatal(tc, got)
		}
	}
	if got := SystemLanguageTag(); got != "zh" && got != "en" {
		t.Fatal(got)
	}
}
