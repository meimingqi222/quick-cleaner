package platform

import "fmt"

func IsPackagedInstall() bool            { return false }
func CleanupPreviousUpdateLeftovers()    {}
func OpenURL(string) error               { return fmt.Errorf("unsupported platform") }
func ApplyUpdateAndRestart(string) error { return fmt.Errorf("unsupported platform for app update") }
