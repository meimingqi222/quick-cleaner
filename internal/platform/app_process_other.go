//go:build !windows

package platform

import "errors"

func RunUninstallShell(string, bool, bool) (uint32, error) {
	return 0, errors.New("Windows uninstall is unsupported on this platform")
}

func RunUninstallExecutable(string, []string) (uint32, error) {
	return 0, errors.New("Windows uninstall is unsupported on this platform")
}

func RunOfficialAppCommand(string, []string, string) (uint32, error) {
	return 0, errors.New("Windows official command is unsupported on this platform")
}

func HasRelatedAppProcess(string, string) bool   { return true }
func AppRegistrationPresent(string, string) bool { return true }
