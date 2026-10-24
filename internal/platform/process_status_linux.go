package platform

import "fmt"

// Linux is an inspection/gallery host, not a supported product target.
func ProcessUniqueID(uint32) *uint64                      { return nil }
func ProcessPortsMap() map[uint32][]uint16                { return map[uint32][]uint16{} }
func ProcessThreadsMap() map[uint32]uint32                { return map[uint32]uint32{} }
func TerminateProcess(uint32, uint64, *uint64) error      { return fmt.Errorf("unsupported platform") }
func ForceTerminateProcess(uint32, uint64, *uint64) error { return fmt.Errorf("unsupported platform") }
