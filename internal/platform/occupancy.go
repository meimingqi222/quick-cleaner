package platform

// Occupant names a process holding files under a cleanup target. It is
// reporting evidence only: it never grants deletion or termination authority.
// System marks services, critical processes and other users' processes, which
// the cache-release path already refuses to terminate.
type Occupant struct {
	PID        uint32
	Name       string
	Executable string
	System     bool
}

// OccupancyOwners identifies who holds files under path. An error means the
// answer is unknown, never that the path is vacant. Callers must keep the
// fail-closed decision they already made from the occupancy spot check.
func OccupancyOwners(path string) ([]Occupant, error) { return occupancyOwners(path) }
