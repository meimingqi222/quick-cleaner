package main

import (
	"github.com/meimingqi222/quick-cleaner/internal/core"
	appui "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func statusFixture(shell appui.Dashboard, processes, details bool) (*appui.StatusPage, appui.StatusPageActions) {
	user, system, exe, command := "example", "root", "/Applications/Example.app/Contents/MacOS/Example", "example --fixture"
	start, parent, selected := uint64(12345), uint32(50), uint32(101)
	s := &appui.StatusPage{Language: shell.Language, DiskTotal: 1_000_000_000_000, DiskFree: 300_000_000_000,
		CPUHistory: []float32{10, 32, 20, 41, 24, 35, 23}, CurrentUser: &user, ExpandedGroups: map[string]bool{}}
	if processes {
		s.Tab = appui.StatusProcesses
	}
	if shell.Scanned {
		s.Snapshot = &core.StatusSnapshot{CPUUsage: 23, CoreCount: 8, MemUsed: 4 << 30, MemTotal: 16 << 30,
			ProcessCount: 3, OSName: "macOS 15.7", UptimeSecs: 90000,
			Processes: []core.ProcInfo{
				{PID: 101, StartTime: start, Name: "Example", CPU: 18, MemBytes: 1 << 30, Threads: 8, User: &user, ExePath: &exe,
					CmdLine: &command, ParentPID: &parent, StatusText: "Running", Ports: []uint16{3000, 8080}, CPUTimeMS: 123456},
				{PID: 102, StartTime: start + 1, Name: "Example", CPU: 4, MemBytes: 512 << 20, Threads: 4, User: &user, ExePath: &exe, StatusText: "Running"},
				{PID: 50, StartTime: start - 100, Name: "system-service", CPU: 1, MemBytes: 32 << 20, Threads: 2, User: &system, StatusText: "Running"},
			}}
	}
	if details && processes {
		s.SelectedPID = &selected
		s.ExpandedGroups["example"] = true
		s.SelectedCPUHistory = []float32{8, 18, 12, 23, 18}
		s.SelectedMemHistory = []uint64{512 << 20, 768 << 20, 1 << 30}
	}
	return s, appui.StatusPageActions{
		SetQuery:      func(q string) { s.Query = q },
		SetFilter:     func(tab core.ProcFilterTab) { s.Filter = tab },
		SetSort:       func(sort appui.StatusSort) { s.Sort = sort },
		ToggleGroup:   func(key string) { s.ExpandedGroups[key] = !s.ExpandedGroups[key] },
		SelectProcess: func(pid uint32) { s.SelectedPID = &pid },
		CloseProcess:  func() { s.SelectedPID = nil },
		SelectGPU:     func(id string) { s.SelectedGPU = id },
	}
}
