package core

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"slices"
	"strings"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// ScanAppResiduals is read-only; discovered candidates do not authorize disposal.
func ScanAppResiduals(ctx context.Context, app InstalledApp) (ResidualScanResult, error) {
	home, known := platform.UserHome()
	if !known {
		return ResidualScanResult{AppName: app.Name, AppID: app.ID}, nil
	}
	snapshot, err := CurrentRules()
	if err != nil {
		return ResidualScanResult{}, err
	}
	evidence := macResidualEvidence{metadata: platform.ReadBundleMetadata}
	if app.InstallLocation != nil {
		run, ok := platform.RunCommandTreeContext(ctx, exec.Command("/usr/bin/codesign", "-d", "--entitlements", ":-", *app.InstallLocation), 3*time.Second)
		if ok {
			evidence.groups = parseApplicationGroups(string(run.Stdout))
		}
	}
	evidence.extensions, _ = listMacSystemExtensions(ctx)
	cache, _ := platform.DarwinUserCacheDir()
	temp, _ := platform.DarwinUserTempDir()
	result, err := scanMacAppResidualsIn(ctx, app, macResidualRoots{
		home: home, systemLibrary: "/Library", receipts: "/private/var/db/receipts", darwinCache: cache, darwinTemp: temp, applications: "/Applications",
	}, evidence, snapshot)
	if err == nil {
		result.Occupancy = DetectAppResidualOccupancy(ctx, app)
	}
	if err := ctx.Err(); err != nil {
		return ResidualScanResult{}, err
	}
	return result, err
}

func listMacSystemExtensions(ctx context.Context) ([]ResidualKind, bool) {
	run, ok := platform.RunCommandTreeContext(ctx, exec.Command("/usr/bin/systemextensionsctl", "list"), 3*time.Second)
	if !ok || !run.OK || len(run.Stdout) == 0 {
		return nil, false
	}
	return parseSystemExtensions(string(run.Stdout)), true
}

// VerifyAppResiduals refreshes only the previously observed candidates after
// official uninstall; it never discovers additional cleanup targets.
func VerifyAppResiduals(ctx context.Context, items []ResidualItem) ([]ResidualItem, error) {
	var extensions []ResidualKind
	known := true
	if slices.ContainsFunc(items, func(item ResidualItem) bool { return item.Kind.Type == ResidualSystemExtension }) {
		extensions, known = listMacSystemExtensions(ctx)
	}
	return verifyMacResidualsWith(ctx, items, extensions, known)
}

// Occupancy is explanatory evidence, not a replacement for the cleaner's live
// database/identity gates. A failed probe never invents a running process.
func DetectAppResidualOccupancy(ctx context.Context, app InstalledApp) ResidualOccupancy {
	var occupancy ResidualOccupancy
	id := asciiLower(strings.TrimSpace(app.RegistrySubpath))
	run, ok := platform.RunCommandTreeContext(ctx, exec.Command("/bin/ps", "-axo", "pid=,args="), 3*time.Second)
	if ok && run.OK && len(run.Stdout) > 0 && run.Stdout[len(run.Stdout)-1] == '\n' {
		usable := true
		for line := range strings.SplitSeq(string(run.Stdout), "\n") {
			fields := strings.Fields(line)
			if len(fields) == 0 {
				continue
			}
			if len(fields) < 2 || !macDecimal(fields[0]) {
				usable = false
				break
			}
			if macResidualProcessMatches(line, id, app.Name) {
				occupancy.Processes = append(occupancy.Processes, line)
			}
		}
		if !usable {
			occupancy.Processes = nil
		}
	}
	run, ok = platform.RunCommandTreeContext(ctx, exec.Command("/bin/launchctl", "print", fmt.Sprintf("gui/%d", os.Getuid())), 3*time.Second)
	if ok {
		occupancy.LaunchdLabels = parseLaunchdRegistered(string(run.Stdout), id)
	}
	return occupancy
}
