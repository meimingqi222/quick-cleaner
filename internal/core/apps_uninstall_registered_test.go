package core

import (
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func registeredAppFixture(t *testing.T) (InstalledApp, registeredUninstallRuntime, *time.Duration) {
	t.Helper()
	location := filepath.Join(t.TempDir(), "Vendor")
	if err := os.Mkdir(location, 0700); err != nil {
		t.Fatal(err)
	}
	command := `"C:\Program Files\Vendor\unins000.exe" /keep-data`
	app := InstalledApp{Name: "Vendor", RegistryRoot: AppHKCU, RegistrySubpath: `Software\Fixture`, InstallLocation: &location, UninstallString: &command}
	elapsed := new(time.Duration)
	start := time.Unix(1000, 0)
	r := nativeRegisteredUninstallRuntime()
	r.now = func() time.Time { return start.Add(*elapsed) }
	r.sleep = func(d time.Duration) { *elapsed += d }
	r.elevated = func() bool { return true }
	r.registered = func(InstalledApp) bool { return true }
	r.processes = func(string, string) bool { return false }
	r.executable = func(string, []string) (uint32, error) { return 0, nil }
	r.shell = func(string, bool, bool) (uint32, error) { return 0, nil }
	return app, r, elapsed
}

func TestRegisteredUninstallSettlement(t *testing.T) {
	for _, tc := range []struct {
		name                              string
		code                              uint32
		processUntil, unregisterAt, dirAt time.Duration
		wantOK                            bool
		want                              time.Duration
	}{
		{"successful-parent-UAC-gap", 0, 0, -1, -1, false, 30 * time.Minute},
		{"failed-command", 9, 0, -1, -1, false, 15 * time.Second},
		{"second-stage-UAC-gap", 0, time.Second, -1, -1, false, 121 * time.Second},
		{"nonzero-but-confirmed-removal", 9, 0, 2 * time.Second, -1, true, 6500 * time.Millisecond},
		{"directory-gone-registration-leftover", 0, 0, -1, time.Second, true, 2500 * time.Millisecond},
		{"directory-gone-process-active", 0, 5 * time.Second, -1, time.Second, true, 6500 * time.Millisecond},
		{"slow-UAC-eventual-success", 0, time.Second, 90 * time.Second, -1, true, 94500 * time.Millisecond},
	} {
		t.Run(tc.name, func(t *testing.T) {
			app, r, elapsed := registeredAppFixture(t)
			r.executable = func(exe string, args []string) (uint32, error) {
				if exe != `C:\Program Files\Vendor\unins000.exe` || !reflect.DeepEqual(args, []string{"/keep-data"}) {
					t.Fatalf("literal command: %q %q", exe, args)
				}
				return tc.code, nil
			}
			r.processes = func(_, stem string) bool {
				if stem != "unins000" {
					t.Fatalf("stem=%q", stem)
				}
				return *elapsed < tc.processUntil
			}
			r.registered = func(InstalledApp) bool { return tc.unregisterAt < 0 || *elapsed < tc.unregisterAt }
			r.sleep = func(d time.Duration) {
				*elapsed += d
				if tc.dirAt >= 0 && *elapsed >= tc.dirAt {
					if err := os.Remove(*app.InstallLocation); err != nil && !os.IsNotExist(err) {
						t.Fatal(err)
					}
				}
			}
			err := runRegisteredAppUninstaller(app, r)
			if (err == nil) != tc.wantOK || *elapsed != tc.want {
				t.Fatalf("err=%v elapsed=%s want success=%v at %s", err, *elapsed, tc.wantOK, tc.want)
			}
		})
	}
}

func TestRegisteredUninstallResetsRegistrationGraceAndStableStreak(t *testing.T) {
	app, r, elapsed := registeredAppFixture(t)
	r.registered = func(InstalledApp) bool { return *elapsed == 2*time.Second }
	r.processes = func(string, string) bool { return *elapsed == 6*time.Second }
	if err := runRegisteredAppUninstaller(app, r); err != nil {
		t.Fatal(err)
	}
	if *elapsed != 8*time.Second {
		t.Fatalf("must restart grace and four-sample streak, elapsed=%s", *elapsed)
	}
}

func TestRegisteredUninstallCommandRouting(t *testing.T) {
	for _, tc := range []struct {
		raw             string
		root            AppRegRoot
		hidden, demoted bool
		want            string
	}{
		{"winget uninstall --id Foo", AppHKCU, true, true, "winget uninstall --id Foo --scope user --disable-interactivity --accept-source-agreements --silent"},
		{"winget uninstall --scope user -h --disable-interactivity --accept-source-agreements", AppHKLM, true, false, "winget uninstall --scope user -h --disable-interactivity --accept-source-agreements"},
		{"choco uninstall Foo", AppHKLM32, true, false, "choco uninstall Foo -y"},
		{"chocolatey uninstall Foo --yes", AppHKCU, true, true, "chocolatey uninstall Foo --yes"},
		{"powershell -File remove.ps1", AppHKCU, false, false, "powershell -File remove.ps1"},
		{`"C:\Vendor\remove.cmd"`, AppHKLM, false, false, `"C:\Vendor\remove.cmd"`},
		{"MsiExec.exe /X{123}", AppHKLM, true, false, "MsiExec.exe /X{123}"},
	} {
		t.Run(tc.raw, func(t *testing.T) {
			app, r, _ := registeredAppFixture(t)
			app.QuietUninstallString, app.RegistryRoot = &tc.raw, tc.root
			r.registered = func(InstalledApp) bool { return false }
			r.shell = func(command string, hidden, demoted bool) (uint32, error) {
				if command != tc.want || hidden != tc.hidden || demoted != tc.demoted {
					t.Fatalf("route=%q hidden=%v desktop=%v", command, hidden, demoted)
				}
				return 0, nil
			}
			r.executable = func(string, []string) (uint32, error) { t.Fatal("shell command sent to direct runner"); return 0, nil }
			r.processes = func(_, stem string) bool {
				if appGenericHost(appCommandStem(tc.raw)) && stem != "" {
					t.Fatal("generic host must not match unrelated processes")
				}
				return false
			}
			if err := runRegisteredAppUninstaller(app, r); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestRegisteredUninstallRetryAndLaunchFailure(t *testing.T) {
	for _, alreadyDemoted := range []bool{false, true} {
		app, r, _ := registeredAppFixture(t)
		command := "winget uninstall --id Foo"
		app.UninstallString = &command
		if !alreadyDemoted {
			app.RegistryRoot = AppHKLM
		}
		attempts := 0
		r.shell = func(_ string, _, demoted bool) (uint32, error) {
			attempts++
			if demoted != (alreadyDemoted || attempts == 2) {
				t.Fatal("wrong desktop-user routing")
			}
			return platform.WingetAdminContextProhibited, nil
		}
		if err := runRegisteredAppUninstaller(app, r); err == nil || !strings.Contains(err.Error(), "winget") {
			t.Fatalf("err=%v", err)
		}
		want := 2
		if alreadyDemoted {
			want = 1
		}
		if attempts != want {
			t.Fatalf("attempts=%d", attempts)
		}
	}
	app, r, elapsed := registeredAppFixture(t)
	launchErr := errors.New("native launch refused")
	r.executable = func(string, []string) (uint32, error) { return 0, launchErr }
	if !errors.Is(runRegisteredAppUninstaller(app, r), launchErr) || *elapsed != 0 {
		t.Fatal("launch error lost or incorrectly entered settlement")
	}
	app.Discovery = &AppDiscovery{}
	r.executable = func(string, []string) (uint32, error) { t.Fatal("discovery must use frozen operation"); return 0, nil }
	if err := runRegisteredAppUninstaller(app, r); err == nil {
		t.Fatal("discovery authorized through registration")
	}
}

func TestRegisteredUninstallReportsDependenciesAndVerification(t *testing.T) {
	for _, scenario := range []string{"success", "failed-command", "registration-remains"} {
		t.Run(scenario, func(t *testing.T) {
			app, _, _ := registeredAppFixture(t)
			out := runRegisteredAppUninstallerReported(app, func(InstalledApp) error {
				if scenario == "failed-command" {
					return errors.New("official refused")
				}
				return nil
			}, func(InstalledApp) bool { return scenario == "registration-remains" })
			if (out.Result == nil) != (scenario == "success") {
				t.Fatalf("err=%v", out.Result)
			}
			if len(out.PlanExecutions) != 1 {
				t.Fatalf("reports=%v", out.PlanExecutions)
			}
			steps := out.PlanExecutions[0].Steps
			if len(steps) != 3 || steps[0].Step.Action.Kind != "revalidate" || steps[1].Step.Action.Kind != "apply" || steps[2].Step.Action.Kind != "verify" {
				t.Fatalf("steps=%v", steps)
			}
			if scenario == "failed-command" && (steps[1].Reason != "official refused" || steps[2].Status != StepBlocked) {
				t.Fatalf("lost failure/dependency: %v", steps)
			}
			if scenario == "registration-remains" && steps[2].Status != StepFailed {
				t.Fatalf("exit success replaced registration check: %v", steps)
			}
		})
	}
}
