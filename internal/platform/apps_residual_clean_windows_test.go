package platform

import (
	"context"
	"errors"
	"fmt"
	"testing"

	"golang.org/x/sys/windows"
	"golang.org/x/sys/windows/registry"
)

// Cancellation must return before any native observation or mutation. These
// contracts are safe to run on a real Windows host without creating registry data.
func TestWindowsResidualCleanCancelledAdapter(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	r := WindowsResidualCleanAdapter{Context: ctx}
	if len(r.PendingRenameWords()) != 0 {
		t.Fatal("cancelled pending observation")
	}
	for _, err := range []error{
		r.DeleteRegistry("HKLM", `SOFTWARE\Fixture`, "", false),
		r.DeleteRegistry("HKCU", `Environment`, "Fixture", true),
		r.DeleteTask(`\Fixture`), r.StopService("Fixture"),
		r.TerminateUnder([]string{`C:\Fixture`}, func(string) bool { t.Fatal("cancelled termination"); return true }),
		r.MoveToTrash(`C:\Fixture`),
	} {
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("cancelled native call: %v", err)
		}
	}
	if absent, known := r.RegistryAbsent("HKLM", `SOFTWARE\Fixture`, "", false); absent || known {
		t.Fatal("cancelled registry observation authorized deletion")
	}
	if absent, known := r.TaskAbsent(`\Fixture`); absent || known {
		t.Fatal("cancelled task observation reported absence")
	}
}

func TestWindowsResidualCleanViewAndNativeAbsence(t *testing.T) {
	r := WindowsResidualCleanAdapter{Context: context.Background()}
	for root, want := range map[string]uint32{"HKLM": registry.WOW64_64KEY, "HKLM32": registry.WOW64_32KEY} {
		h, view, err := r.registryView(root)
		if err != nil || h != registry.LOCAL_MACHINE || view != want {
			t.Fatalf("wrong view for %s: %v %x %v", root, h, view, err)
		}
	}
	for _, err := range []error{windows.ERROR_FILE_NOT_FOUND, fmt.Errorf("wrapped: %w", windows.ERROR_PATH_NOT_FOUND)} {
		if absent, known := residualCleanRegistryAbsence(err); !absent || !known {
			t.Fatal("native absence lost", err)
		}
	}
	if absent, known := residualCleanRegistryAbsence(windows.ERROR_ACCESS_DENIED); absent || known {
		t.Fatal("access denied treated as absence")
	}
	if absent, known := residualCleanRegistryAbsence(nil); absent || !known {
		t.Fatal("existing target treated as absent")
	}
}
