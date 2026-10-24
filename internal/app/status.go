package app

import (
	"context"
	"fmt"
	"runtime"
	"time"

	"github.com/egoist/mygo"
	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func (r *Root) startMonitoring() {
	ctx := r.ctx
	refresh := make(chan struct{}, 1)
	r.statusRefresh = refresh
	go func() {
		sampler, err := core.NewStatusSampler(ctx)
		if err != nil {
			r.publish(func() { r.failure(err) })
			return
		}
		ticker := time.NewTicker(2 * time.Second)
		defer ticker.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
			case <-refresh:
			}
			snapshot, err := sampler.Sample(ctx)
			if err != nil {
				if ctx.Err() == nil {
					r.publish(func() { r.failure(err) })
				}
				continue
			}
			r.publish(func() {
				if r.dashboard.MonitoringPaused {
					return
				}
				r.updateStatus(snapshot)
			})
		}
	}()
	go func() {
		fda := runtime.GOOS == "darwin" && platform.HasFullDiskAccess()
		helper := platform.FanHelperInstalled()
		r.publish(func() {
			r.dashboard.FullDiskAccess = fda
			r.status.FanControlSupported = platform.FanControlSupported()
			r.status.FanHelperInstalled = helper
		})
	}()
}

func (r *Root) updateStatus(snapshot core.StatusSnapshot) {
	const historyLength = 24
	r.status.Snapshot = &snapshot
	r.dashboard.ProcessCount = snapshot.ProcessCount
	r.status.CPUHistory = append(r.status.CPUHistory, snapshot.CPUUsage)
	r.status.CPUHistory = r.status.CPUHistory[max(0, len(r.status.CPUHistory)-historyLength):]
	if r.status.GPUHistory == nil {
		r.status.GPUHistory = map[string][]float32{}
	}
	selectedPresent := false
	busiest, busiestUsage := "", float32(-1)
	for _, gpu := range snapshot.GPUs {
		selectedPresent = selectedPresent || gpu.ID == r.status.SelectedGPU
		usage := float32(0)
		if gpu.Utilization != nil {
			usage = *gpu.Utilization
			history := append(r.status.GPUHistory[gpu.ID], usage)
			r.status.GPUHistory[gpu.ID] = history[max(0, len(history)-historyLength):]
		}
		if usage >= busiestUsage {
			busiest, busiestUsage = gpu.ID, usage
		}
	}
	if !selectedPresent {
		r.status.SelectedGPU = busiest
	}
	if r.status.SelectedPID != nil {
		for _, p := range snapshot.Processes {
			if p.PID == *r.status.SelectedPID {
				r.status.SelectedCPUHistory = append(r.status.SelectedCPUHistory, p.CPU)
				r.status.SelectedCPUHistory = r.status.SelectedCPUHistory[max(0, len(r.status.SelectedCPUHistory)-historyLength):]
				r.status.SelectedMemHistory = append(r.status.SelectedMemHistory, p.MemBytes)
				r.status.SelectedMemHistory = r.status.SelectedMemHistory[max(0, len(r.status.SelectedMemHistory)-historyLength):]
				return
			}
		}
		r.selectProcess(nil)
	}
}

func (r *Root) selectProcess(pid *uint32) {
	if pid != nil && r.status.SelectedPID != nil && *pid == *r.status.SelectedPID {
		pid = nil
	}
	r.status.SelectedPID = pid
	r.status.SelectedCPUHistory, r.status.SelectedMemHistory = nil, nil
	if pid != nil && r.status.Snapshot != nil {
		for _, p := range r.status.Snapshot.Processes {
			if p.PID == *pid {
				r.status.SelectedCPUHistory = []float32{p.CPU}
				r.status.SelectedMemHistory = []uint64{p.MemBytes}
				break
			}
		}
	}
}

func (r *Root) refreshStatus() {
	if r.statusRefresh == nil {
		return
	}
	select {
	case r.statusRefresh <- struct{}{}:
	default:
	}
}

func (r *Root) copyProcessCommand(cmd string) {
	mygo.Clipboard.WriteText(cmd)
	r.status.CommandCopied = true
	r.changed()
	gen := r.generation
	time.AfterFunc(1500*time.Millisecond, func() {
		r.publish(func() {
			if gen == r.generation {
				r.status.CommandCopied = false
			}
		})
	})
}

func (r *Root) statusActions() view.StatusPageActions {
	return view.StatusPageActions{
		SetQuery: func(s string) { r.status.Query = s }, SetFilter: func(f core.ProcFilterTab) { r.status.Filter = f }, SetSort: func(s view.StatusSort) { r.status.Sort = s },
		ToggleGroup: func(s string) { r.status.ExpandedGroups[s] = !r.status.ExpandedGroups[s] }, SelectProcess: func(pid uint32) { r.selectProcess(&pid) }, CloseProcess: func() { r.selectProcess(nil) }, SelectGPU: func(s string) { r.status.SelectedGPU = s },
		RequestQuit: func(p core.ProcInfo) { r.requestTerminate(p, false) }, ForceQuit: func(p core.ProcInfo) { r.requestTerminate(p, true) }, RevealProcess: r.reveal, CopyCommand: r.copyProcessCommand,
		ApplyFanMode: r.requestFanMode, RemoveFanHelper: r.requestRemoveFanHelper,
	}
}

func (r *Root) requestTerminate(p core.ProcInfo, force bool) {
	if r.busy() {
		return
	}
	title := r.tr("退出进程？", "Quit process?")
	if force {
		title = r.tr("强制结束进程？", "Force quit process?")
	}
	r.askKind(view.ConfirmEndProcess, title, fmt.Sprintf("%s (PID %d)\n%s", p.Name, p.PID, r.tr("未保存的数据可能丢失。仅对采样时的进程身份执行操作。", "Unsaved data may be lost. Only the sampled process identity is targeted.")), func() {
		r.start(true, nil, func(context.Context) func() {
			var err error
			if force {
				err = platform.ForceTerminateProcess(p.PID, p.StartTime, p.UniqueID)
			} else {
				err = platform.TerminateProcess(p.PID, p.StartTime, p.UniqueID)
			}
			return func() {
				if err != nil {
					r.failure(err)
				} else {
					r.message(fmt.Sprintf("已向「%s」发送结束请求", p.Name), fmt.Sprintf("Terminate signal sent to \"%s\"", p.Name))
					r.refreshStatus()
				}
			}
		})
	})
}
