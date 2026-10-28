package platform

import (
	"context"
	"testing"

	"golang.org/x/sys/windows/registry"
)

func TestWindowsResidualNativeReadViewsAndCancellation(t *testing.T) {
	for _, v := range []struct {
		root  string
		key   registry.Key
		flags uint32
	}{
		{"HKCU", registry.CURRENT_USER, registry.READ},
		{"HKLM", registry.LOCAL_MACHINE, registry.READ | registry.WOW64_64KEY},
		{"HKLM32", registry.LOCAL_MACHINE, registry.READ | registry.WOW64_32KEY},
	} {
		key, flags := residualRegistryView(v.root)
		if key != v.key || flags != v.flags {
			t.Fatalf("wrong native view: %s", v.root)
		}
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	r := WindowsResidualReader{Context: ctx}
	if r.KeyExists("HKCU", "Software") || len(r.Subkeys("HKCU", "Software")) != 0 || len(r.Values("HKCU", "Environment")) != 0 {
		t.Fatal("cancelled reader performed reads")
	}
}
