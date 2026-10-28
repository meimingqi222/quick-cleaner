//go:build !darwin

package platform

func ElevatedFanControl(FanMode) error { return fanOther("fan helper is only supported on macOS") }
func FanHelperInstalled() bool         { return false }
func InstallFanHelper(string) error    { return fanOther("fan helper is only supported on macOS") }
func UninstallFanHelper(string) error  { return fanOther("fan helper is only supported on macOS") }
func RunFanHelperDaemon() error        { return fanOther("fan helper is only supported on macOS") }
func RunFanHelperInstallStep() error   { return fanOther("fan helper is only supported on macOS") }
