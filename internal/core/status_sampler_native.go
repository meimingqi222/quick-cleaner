package core

import (
	"context"
	"fmt"
	"path/filepath"
	"runtime"
	"strings"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
	"github.com/shirou/gopsutil/v4/cpu"
	"github.com/shirou/gopsutil/v4/host"
	gnet "github.com/shirou/gopsutil/v4/net"
	"github.com/shirou/gopsutil/v4/process"
)

type systemStatusSource struct {
	osName      string
	initialized bool
}

func readSystemCPU(ctx context.Context) (total, busy float64, err error) {
	times, err := cpu.TimesWithContext(ctx, false)
	if err != nil {
		return 0, 0, fmt.Errorf("read CPU counters: %w", err)
	}
	if len(times) == 0 {
		return 0, 0, fmt.Errorf("read CPU counters: no counters available")
	}
	t := times[0]
	busy = t.User + t.System + t.Nice + t.Irq + t.Softirq + t.Steal
	if runtime.GOOS == "windows" {
		// Windows KernelTime already includes InterruptTime. Adding Irq again
		// would inflate both utilization and the process CPU denominator.
		busy = t.User + t.System
	}
	return busy + t.Idle + t.Iowait, busy, nil
}

func (s *systemStatusSource) ReadStatus(ctx context.Context) (StatusCounters, error) {
	var out StatusCounters
	var err error
	out.CPUTotal, out.CPUBusy, err = readSystemCPU(ctx)
	if err != nil {
		return out, err
	}
	memory, err := platform.ReadStatusMemory(ctx)
	if err != nil {
		return out, fmt.Errorf("read memory: %w", err)
	}
	interfaces, err := gnet.IOCountersWithContext(ctx, true)
	if err != nil {
		return out, fmt.Errorf("read network counters: %w", err)
	}
	for _, network := range interfaces {
		out.Networks = append(out.Networks, StatusNetworkCounter{Name: network.Name, Received: network.BytesRecv, Sent: network.BytesSent})
	}
	if !s.initialized {
		// Match Rust's constructor: prime CPU/network only. In particular, GPU
		// PDH queries must get their first collection on the first actual sample.
		if err := ctx.Err(); err != nil {
			return StatusCounters{}, err
		}
		s.initialized = true
		out.At = time.Now()
		return out, nil
	}
	all, err := process.ProcessesWithContext(ctx)
	if err != nil {
		return out, fmt.Errorf("enumerate processes: %w", err)
	}
	ports, threads := platform.ProcessPortsMap(), platform.ProcessThreadsMap()
	parentNames := make(map[uint32]string, len(all))
	for _, p := range all {
		if err := ctx.Err(); err != nil {
			return StatusCounters{}, err
		}
		if p.Pid < 0 || runtime.GOOS == "windows" && p.Pid == 0 {
			continue
		}
		name, _ := p.NameWithContext(ctx)
		name = strings.TrimSpace(name)
		if name == "" {
			continue
		}
		info := DefaultProcInfo()
		info.PID, info.Name = uint32(p.Pid), name
		parentNames[info.PID] = name
		info.UniqueID = platform.ProcessUniqueID(info.PID)
		if created, err := p.CreateTimeWithContext(ctx); err == nil && created > 0 {
			info.StartTime = uint64(created / 1000)
		}
		if executable, err := p.ExeWithContext(ctx); err == nil && executable != "" {
			info.ExePath = &executable
			info.Name, info.IconSource = statusProcessPresentation(name, executable)
		}
		if user, err := p.UsernameWithContext(ctx); err == nil && user != "" {
			if runtime.GOOS == "windows" {
				user = user[strings.LastIndexByte(user, '\\')+1:]
			}
			info.User = &user
		} else if runtime.GOOS == "windows" && info.PID == 4 {
			user := "SYSTEM"
			info.User = &user
		}
		info.Ports = ports[info.PID]
		if count, ok := threads[info.PID]; ok {
			info.Threads = count
		}
		if memory, err := p.MemoryInfoWithContext(ctx); err == nil {
			info.MemBytes, info.VirtualMemBytes = memory.RSS, memory.VMS
		}
		if runtime.GOOS == "windows" {
			// gopsutil's VMS is PagefileUsage, not sysinfo's PrivateUsage.
			info.VirtualMemBytes = 0
		}
		if private := platform.ProcessVirtualMemory(info.PID); private != nil {
			info.VirtualMemBytes = *private
		}
		if parent, err := p.PpidWithContext(ctx); err == nil && parent > 0 {
			pid := uint32(parent)
			info.ParentPID = &pid
		}
		if args, err := p.CmdlineSliceWithContext(ctx); err == nil && len(args) > 0 {
			for i, arg := range args {
				if strings.Contains(arg, " ") && !strings.HasPrefix(arg, `"`) {
					args[i] = `"` + arg + `"`
				}
			}
			command := strings.Join(args, " ")
			info.CmdLine = &command
		} else {
			info.CmdLine = info.ExePath
		}
		info.StatusText = platform.ProcessStatusText(ctx, info.PID)
		counter := StatusProcessCounter{Info: info}
		if times, err := p.TimesWithContext(ctx); err == nil {
			seconds := times.User + times.System
			counter.CPUSeconds = &seconds
		}
		if io, err := p.IOCountersWithContext(ctx); err == nil {
			read, written := io.ReadBytes, io.WriteBytes
			if runtime.GOOS == "darwin" {
				read, written = io.DiskReadBytes, io.DiskWriteBytes
			}
			counter.ReadBytes, counter.WrittenBytes = &read, &written
		}
		if info.UniqueID != nil {
			// Do not attach an old identity to metadata from a reused PID.
			currentID := platform.ProcessUniqueID(info.PID)
			if currentID == nil || *currentID != *info.UniqueID {
				continue
			}
		}
		out.Processes = append(out.Processes, counter)
	}
	for i := range out.Processes {
		info := &out.Processes[i].Info
		if info.ParentPID != nil {
			if name, ok := parentNames[*info.ParentPID]; ok {
				info.ParentName = &name
			}
		}
	}
	cores, err := cpu.CountsWithContext(ctx, true)
	if err != nil || cores <= 0 {
		cores = runtime.NumCPU()
	}
	if runtime.GOOS == "windows" {
		clock := out.CPUTotal / float64(cores)
		out.ProcessCPUClock = &clock
	}
	uptime, _ := platform.SystemUptimeSecs(ctx)
	if s.osName == "" {
		os, _, version, _ := host.PlatformInformationWithContext(ctx)
		switch runtime.GOOS {
		case "darwin":
			os = "macOS"
		case "windows":
			os = strings.TrimPrefix(os, "Microsoft ")
		}
		s.osName = ShortOSName(strings.TrimSpace(os + " " + version))
		if s.osName == "" {
			s.osName = "?"
		}
	}
	out.Snapshot = StatusSnapshot{
		CoreCount: cores, MemUsed: memory.Used, MemTotal: memory.Total,
		MemTotalLabelBytes: memory.Total, SwapUsed: memory.SwapUsed, SwapTotal: memory.SwapTotal,
		UptimeSecs: uptime, OSName: s.osName, Battery: platform.ReadBattery(),
		GPUs: platform.ReadGPUs(), Thermal: platform.ReadThermal(),
	}
	if err := ctx.Err(); err != nil {
		return StatusCounters{}, err
	}
	out.At = time.Now()
	return out, nil
}

func statusProcessPresentation(name, executable string) (string, *string) {
	if runtime.GOOS == "windows" {
		return name, &executable
	}
	if runtime.GOOS != "darwin" {
		return name, nil
	}
	var nearest, outer string
	for path := executable; ; path = filepath.Dir(path) {
		if strings.EqualFold(filepath.Ext(path), ".app") {
			if nearest == "" {
				nearest = path
			}
			outer = path
		}
		if parent := filepath.Dir(path); parent == path {
			break
		}
	}
	if outer == "" {
		return name, nil
	}
	app := strings.TrimSuffix(filepath.Base(outer), filepath.Ext(outer))
	var nested *string
	if nearest != outer {
		label := strings.TrimSuffix(filepath.Base(nearest), filepath.Ext(nearest))
		nested = &label
	}
	return ComposeProcessName(name, app, nested), &outer
}
