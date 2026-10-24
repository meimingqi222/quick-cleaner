package main

import (
	"fmt"
	"os"
	"runtime"
	"slices"

	"github.com/meimingqi222/quick-cleaner/internal/app"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func main() { os.Exit(run(os.Args[1:])) }

func run(args []string) int {
	// Helper dispatch must precede GUI creation. Original-user arguments are
	// deliberately not reparsed by flag: the platform adapter owns their contract.
	if len(args) == 1 {
		var err error
		switch args[0] {
		case "--fanhelper":
			err = platform.RunFanHelperDaemon()
		case "--fanhelper-install":
			err = platform.RunFanHelperInstallStep()
		case "--version":
			fmt.Println(app.Version)
			return 0
		default:
			goto product
		}
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			return 1
		}
		return 0
	}
product:
	if slices.Contains(args, "--fanhelper") || slices.Contains(args, "--fanhelper-install") {
		fmt.Fprintln(os.Stderr, "helper mode requires exactly one argument")
		return 2
	}
	if runtime.GOOS == "windows" {
		// Resolve the original foreground account before the elevation adapter
		// relaunches; invalid explicit context never becomes the admin's profile.
		_, _ = platform.UserHome()
		_, _ = platform.UserSID()
		if !slices.Contains(args, "--no-elevate") {
			platform.RelaunchAsAdminIfNeeded()
		}
	}
	if err := app.Run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 1
	}
	return 0
}
