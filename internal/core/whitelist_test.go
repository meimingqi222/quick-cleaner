package core

import (
	"encoding/json"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
	"path/filepath"
	"sync"
	"testing"
)

func TestWhitelistRustRoundtrip(t *testing.T) {
	ReloadWhitelist(nil)
	t.Cleanup(func() { ReloadWhitelist(nil) })
	base := filepath.Join(t.TempDir(), "MixedCase")
	child := base + "/Keep"
	if !ReloadWhitelist([]string{child, child + "/"}) {
		t.Fatal("reload")
	}
	if len(CurrentWhitelist()) != 2 {
		t.Fatal("reload must retain duplicates")
	}
	list, ok := AddWhitelist(child + "/")
	if !ok || len(list) != 2 || list[0] != child {
		t.Fatal("dedup/display", list)
	}
	if !IsWhitelisted(child+"/x") || IsWhitelisted(base) || IsWhitelisted(child+"Extra") || !HasWhitelistEntryUnder(base) || HasWhitelistEntryUnder(child+"/x") {
		t.Fatal("boundary/ancestor semantics")
	}
	if !IsProtected(child) || !IsContentsProtected(child) {
		t.Fatal("central protection")
	}
	AddWhitelist(base)
	RemoveWhitelist(base)
	if !IsWhitelisted(child) {
		t.Fatal("remove must be exact")
	}
	list, ok = RemoveWhitelist(child)
	if !ok || len(list) != 0 || list == nil {
		t.Fatal("empty list must be nonnil")
	}
	data, _ := json.Marshal(CurrentWhitelist())
	if string(data) != "[]" {
		t.Fatal(string(data))
	}
	AddWhitelist(child)
	copy := CurrentWhitelist()
	copy[0] = "mutated"
	if CurrentWhitelist()[0] != child {
		t.Fatal("snapshot alias")
	}
}
func TestWhitelistTrustedHome(t *testing.T) {
	ReloadWhitelist(nil)
	t.Cleanup(func() { ReloadWhitelist(nil) })
	home, ok := platform.UserHome()
	if !ok {
		if ReloadWhitelist([]string{"~/x"}) {
			t.Fatal("unknown home")
		}
		return
	}
	if !ReloadWhitelist([]string{"~", "~/MixedCase"}) {
		t.Fatal("reload")
	}
	got := CurrentWhitelist()
	if got[0] != home || !IsWhitelisted(filepath.Join(home, "MixedCase", "x")) {
		t.Fatal(got)
	}
}
func TestWhitelistConcurrentSnapshots(t *testing.T) {
	ReloadWhitelist(nil)
	t.Cleanup(func() { ReloadWhitelist(nil) })
	var wg sync.WaitGroup
	for i := 0; i < 8; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for j := 0; j < 50; j++ {
				AddWhitelist("/fixture/Keep")
				IsWhitelisted("/fixture/Keep/x")
				HasWhitelistEntryUnder("/fixture")
				CurrentWhitelist()
				RemoveWhitelist("/fixture/Keep")
			}
		}()
	}
	wg.Wait()
}
