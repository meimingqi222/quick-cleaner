package main

import (
	"fmt"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	appui "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func trayFixture(shell appui.Dashboard, details bool) appui.TrayPopover {
	s := appui.TrayPopover{Language: shell.Language}
	if shell.HasJunk {
		temp := float32(52)
		s.Snapshot = appui.TrayOverviewSnapshot{TraySnapshot: core.TraySnapshot{CPUPct: 23, MemoryUsed: 8 << 30, MemoryTotal: 16 << 30, TempC: &temp}, Volumes: []appui.TrayVolumeInfo{{Label: "/", TotalBytes: 512 << 30, FreeBytes: 128 << 30}, {Label: "/Volumes/Data", TotalBytes: 1024 << 30, FreeBytes: 80 << 30}}}
		if details {
			for i := range 4 {
				s.Snapshot.Volumes = append(s.Snapshot.Volumes, appui.TrayVolumeInfo{Label: fmt.Sprintf("/Volumes/External-%d", i+1), TotalBytes: 256 << 30, FreeBytes: 200 << 30})
			}
		}
	}
	return s
}
