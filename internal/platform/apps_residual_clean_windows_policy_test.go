package platform

import "testing"

func TestWindowsResidualCleanTaskConfinement(t *testing.T) {
	for _, name := range []string{`\Vendor\Task`, `Vendor/Task`, `Task`} {
		if _, ok := residualCleanTaskRelative(name); !ok {
			t.Errorf("valid task rejected: %q", name)
		}
	}
	for _, name := range []string{"", `\`, `C:\Task`, `\Vendor\..\Task`, `\Vendor\.\Task`, `\Vendor\\Task`, "/Task", "Task\x00suffix", "Task\n"} {
		if relative, ok := residualCleanTaskRelative(name); ok {
			t.Errorf("unconfined task accepted: %q -> %q", name, relative)
		}
	}
}

func TestWindowsResidualCleanRegistrySyntax(t *testing.T) {
	for _, path := range []string{`Environment`, `SOFTWARE\Vendor\App`, `SYSTEM\CurrentControlSet\Services\Svc`} {
		if !residualCleanRegistryPath(path) {
			t.Errorf("valid key rejected: %q", path)
		}
	}
	for _, path := range []string{"", `\SOFTWARE`, `SOFTWARE\`, `SOFTWARE\\Vendor`, `SOFTWARE/Other`, `SOFTWARE\..\Other`, "key\x00suffix"} {
		if residualCleanRegistryPath(path) {
			t.Errorf("invalid key accepted: %q", path)
		}
	}
}

func TestWindowsResidualCleanProcessPathBoundaries(t *testing.T) {
	roots := []string{`C:\Apps\Selected\`}
	for _, image := range []string{`c:\apps\selected\bin\app.exe`, `C:/Apps/Selected/app.exe`, `C:\Apps\Selected`} {
		if !residualCleanProcessWithin(image, roots) {
			t.Errorf("selected process not matched: %q", image)
		}
	}
	for _, image := range []string{`C:\Apps\SelectedOther\app.exe`, `C:\Apps\Other\app.exe`, "app.exe", `C:\Apps\Selected-old.exe`} {
		if residualCleanProcessWithin(image, roots) {
			t.Errorf("unrelated process authorized: %q", image)
		}
	}
	if residualCleanProcessWithin(`C:\Windows\system.exe`, []string{`C:\`}) {
		t.Fatal("drive root authorized termination")
	}
}
