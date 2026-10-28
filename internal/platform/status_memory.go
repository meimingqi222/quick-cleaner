package platform

import "math"

// StatusMemoryReading uses bytes and the Rust sysinfo 0.37.2 accounting rules.
type StatusMemoryReading struct {
	Used, Total, SwapUsed, SwapTotal uint64
}

func statusSaturatingSub(a, b uint64) uint64 {
	if a < b {
		return 0
	}
	return a - b
}

func statusSaturatingMul(a, b uint64) uint64 {
	if b != 0 && a > math.MaxUint64/b {
		return math.MaxUint64
	}
	return a * b
}

func statusWindowsSwap(commitTotal, commitLimit, physicalTotal, pageSize uint64) (used, total uint64) {
	return statusSaturatingMul(statusSaturatingSub(commitTotal, physicalTotal), pageSize),
		statusSaturatingMul(statusSaturatingSub(commitLimit, physicalTotal), pageSize)
}

func statusDarwinUsed(active, wired, compressor, speculative uint32, pageSize uint64) uint64 {
	// Widen before adding: these are four independent natural_t page counts.
	return statusSaturatingMul(uint64(active)+uint64(wired)+uint64(compressor)+uint64(speculative), pageSize)
}
