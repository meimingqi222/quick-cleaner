package platform

import (
	"encoding/binary"
	"reflect"
	"testing"
)

func TestAppActivityIndex(t *testing.T) {
	var index AppActivityIndex
	if index.Lookup(nil) != 0 || index.LookupOne("missing") != 0 {
		t.Fatal("nonempty zero value")
	}
	index.Insert(`c:\program files\google\chrome\application\chrome.exe`, 100)
	index.Insert(`d:/chrome/chrome.exe`, 80)
	index.Insert(`d:/other/chrome.exe`, 200)
	for _, key := range []string{"chrome.exe", "chrome", "other", `d:/other/chrome.exe`} {
		if index.LookupOne(key) != 200 {
			t.Errorf("key %q", key)
		}
	}
	if index.LookupOne("google") != 100 || index.LookupOne("chrom") != 0 || index.LookupOne("Chrome") != 0 || index.Lookup([]string{"missing", "google", "chrome"}) != 200 {
		t.Fatal("lookup semantics changed")
	}
}

func TestAppActivityNativeBlobs(t *testing.T) {
	const ts = uint64(1704067200)
	for _, tc := range []struct{ size, offset int }{{8, 0}, {15, 0}, {16, 8}, {67, 8}, {68, 60}, {72, 60}} {
		data := make([]byte, tc.size)
		binary.LittleEndian.PutUint64(data[tc.offset:], ts*10000000+116444736000000000)
		if got := appRunBlobUnix(data); got != ts {
			t.Errorf("size %d: %d", tc.size, got)
		}
	}
	for n := 0; n < 8; n++ {
		if appRunBlobUnix(make([]byte, n)) != 0 {
			t.Fatal("truncated accepted")
		}
	}
	for _, seconds := range []uint64{0, 946684799, 4102444800, 9000000000} {
		if appFiletimeUnix(seconds*10000000+116444736000000000) != 0 {
			t.Fatal("out-of-range time accepted")
		}
	}
	for _, seconds := range []uint64{946684800, 4102444799} {
		if appFiletimeUnix(seconds*10000000+116444736000000000) != seconds {
			t.Fatal("valid boundary rejected")
		}
	}
	if appFiletimeUnix(0) != 0 {
		t.Fatal("zero accepted")
	}
}

func TestAppActivityNamesAndPaths(t *testing.T) {
	for raw, want := range map[string]string{`C:\Program Files\App.exe`: `c:\program files\app.exe`, `{GUID}:C:\App.exe`: `c:\app.exe`, "UEME_RUNPATH": "ueme_runpath", "中文.exe": "中文.exe"} {
		if got := appUserAssistName(appROT13(raw)); got != want {
			t.Errorf("%q = %q", raw, got)
		}
	}
	dos := map[string]string{`\device\harddiskvolume3`: "c:", `\device\harddiskvolume30`: "d:"}
	for raw, want := range map[string]string{`\Device\HarddiskVolume30\App.exe`: `d:\app.exe`, `\Device\HarddiskVolume3\App.exe`: `c:\app.exe`, `\Device\HarddiskVolume300\App.exe`: `\device\harddiskvolume300\app.exe`, `\??\C:\App.exe`: `c:\app.exe`, `\\?\C:\App.exe`: `c:\app.exe`} {
		if got := appNormalizeNTPath(raw, dos); got != want {
			t.Errorf("%q = %q", raw, got)
		}
	}
	for raw, want := range map[string]string{"CHROME.EXE-A8C2E3F1.pf": "chrome.exe", "SETUP-HELPER.EXE-DEADBEEF.PF": "setup-helper.exe", "readme.txt": "", "APP.EXE-1234567Z.pf": "", "-12345678.pf": ""} {
		if got := appPrefetchExe(raw); got != want {
			t.Errorf("%q = %q", raw, got)
		}
	}
}

func TestAppActivityTokens(t *testing.T) {
	for name, want := range map[string][]string{
		"PotPlayer-64 bit": {"potplayer"}, "PowerShell 7.6.3.0-x64": {"powershell"},
		"Microsoft Windows Corporation Setup Installer Version 64bit 32bit x86": nil,
		"中文-工具 αβ A é": {"中文", "工具", "αβ"}, "abc abc": {"abc", "abc"}, "软件。工具": {"软件", "工具"},
		"软件🚀工具":                  {"软件🚀工具"},
		"İstanbul ΟΣ ab\u0345cd": {"stanbul", "ος", "ab\u0345cd"},
	} {
		if got := ExtractAppTokens(name); !reflect.DeepEqual(got, want) {
			t.Errorf("%q: %v != %v", name, got, want)
		}
	}
}
