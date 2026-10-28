package platform

// WindowsResidualPrefetchExecutable shares the inventory parser's exact hash
// validation; a similarly named arbitrary .pf file is not prefetch evidence.
func WindowsResidualPrefetchExecutable(filename string) string { return appPrefetchExe(filename) }
