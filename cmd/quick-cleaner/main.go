package main

import (
	"fmt"
	"os"
	"runtime"
	"runtime/debug"
	"slices"

	"github.com/meimingqi222/quick-cleaner/internal/app"
	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func main() {
	defer func() {
		if rec := recover(); rec != nil {
			core.LogPanic("main", rec, debug.Stack())
			panic(rec)
		}
	}()
	// Windows detaches the startup console in init. --version attaches there
	// and must not detach again.
	os.Exit(run(os.Args[1:]))
}

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
		// Before elevating: a second launch wakes the running instance
		// instead of prompting for UAC and opening another window.
		if platform.InstanceRunning() {
			platform.WakeRunningInstance()
			return 0
		}
		if !slices.Contains(args, "--no-elevate") {
			platform.RelaunchAsAdminIfNeeded()
		}
		// Two launches can pass the check together; the loser wakes the winner.
		if !platform.AcquireInstance() {
			platform.WakeRunningInstance()
			return 0
		}
	}
	core.InitProductLog(app.Version, platform.IsElevated(), os.Getpid())
	if err := app.Run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 1
	}
	return 0
}
