package platform

import "context"

// Rust sysinfo reports Run for Windows processes, independent of thread state.
func ProcessStatusText(context.Context, uint32) string { return "Run" }
