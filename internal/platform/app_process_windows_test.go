package platform

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"golang.org/x/sys/windows"
	"golang.org/x/sys/windows/registry"
)

func TestAppProcessNativeChild(t *testing.T) {
	marker := -1
	for i, arg := range os.Args {
		if arg == "--app-native-fixture" {
			marker = i
			break
		}
	}
	if marker < 0 {
		return
	}
	cwd, err := os.Getwd()
	if err != nil {
		os.Exit(81)
	}
	data, err := json.Marshal(struct {
		Args []string
		CWD  string
	}{os.Args[marker+2:], cwd})
	if err != nil || os.WriteFile(os.Args[marker+1], data, 0600) != nil {
		os.Exit(82)
	}
	os.Exit(37)
}

func TestAppProcessNativeLiteralArguments(t *testing.T) {
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	cwd := t.TempDir()
	receipt := filepath.Join(cwd, "receipt.json")
	args := []string{"", `quoted"value`, `ends in slash\`, "中文", "literal & | %PATH% $env:PATH"}
	argv := append([]string{"-test.run=^TestAppProcessNativeChild$", "--", "--app-native-fixture", receipt}, args...)
	code, err := appRunNative(exe, appExecutableLine(exe, argv), cwd, true, false, false)
	if err != nil || code != 37 {
		t.Fatalf("code=%d err=%v", code, err)
	}
	data, err := os.ReadFile(receipt)
	if err != nil {
		t.Fatal(err)
	}
	var got struct {
		Args []string
		CWD  string
	}
	if err := json.Unmarshal(data, &got); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(got.Args, args) || !strings.EqualFold(got.CWD, cwd) {
		t.Fatalf("receipt=%+v", got)
	}
	if IsElevated() {
		t.Log("desktop-user demotion requires interactive signed-in account; literal native path tested without demotion")
	}
}

func TestAppProcessNativeRegistryPresence(t *testing.T) {
	path := fmt.Sprintf(`Software\QuickCleanerTest-%d`, os.Getpid())
	key, existed, err := registry.CreateKey(registry.CURRENT_USER, path, registry.ALL_ACCESS)
	if err != nil {
		t.Fatal(err)
	}
	key.Close()
	if existed {
		t.Fatal("fixture key unexpectedly existed; left untouched")
	}
	defer registry.DeleteKey(registry.CURRENT_USER, path)
	if !AppRegistrationPresent("HKCU", path) {
		t.Fatal("own fixture registration not detected")
	}
	if err := registry.DeleteKey(registry.CURRENT_USER, path); err != nil {
		t.Fatal(err)
	}
	if AppRegistrationPresent("HKCU", path) {
		t.Fatal("removed fixture registration still present")
	}
}

func TestAppProcessNativeMatchingAndStagingLinks(t *testing.T) {
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	if !HasRelatedAppProcess(filepath.Dir(exe), "") {
		t.Fatal("own process missing")
	}
	if !HasRelatedAppProcess("", strings.TrimSuffix(filepath.Base(exe), filepath.Ext(exe))) {
		t.Fatal("own stem missing")
	}
	root := t.TempDir()
	if err := appRejectStagingLinks(filepath.Join(root, "new", "path")); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(root, "link")
	if err := os.Symlink(root, link); err != nil {
		t.Skipf("symlink privilege unavailable: %v", err)
	}
	if err := appRejectStagingLinks(filepath.Join(link, "new")); err == nil {
		t.Fatal("linked ancestor accepted")
	}
	token, err := windows.OpenCurrentProcessToken()
	if err != nil {
		t.Fatal(err)
	}
	defer token.Close()
	if !appExpectedSID(token) {
		t.Fatal("test process SID mismatch")
	}
}
