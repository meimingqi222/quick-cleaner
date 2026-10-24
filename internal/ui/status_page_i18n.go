package ui

import (
	"fmt"
	"time"
)

// Verbatim product copy from src/ui/i18n/mod.rs and views.rs.
var statusCopy = map[string][2]string{
	"loading":     {"正在采样…", "Sampling…"},
	"overview":    {"硬件概览", "Hardware Overview"},
	"overviewSub": {"实时监测系统硬件健康、资源占用与温度转速", "Live monitoring of hardware health, resource usage, and thermals"},
	"manager":     {"进程管理", "Process Manager"},
	"running":     {"%d 个运行中", "%d running"},
	"threadCount": {"%d 个线程", "%d threads"},
	"showing":     {"显示 %d 个进程，共 %d 个", "Showing %d of %d processes"},
	"matched":     {"%d 个运行中 · 匹配 %d 个", "%d running · %d matched"},
	"health":      {"健康概览", "Health Overview"},
	"healthShort": {"健康", "HEALTH"},
	"cpu":         {"处理器", "CPU"},
	"memory":      {"内存", "Memory"},
	"disk":        {"磁盘", "Disk"},
	"network":     {"网络", "Network"},
	"fan":         {"风扇与温度", "Fan & Temp"},
	"battery":     {"电池", "Battery"},
	"good":        {"良好", "Good"}, "fair": {"一般", "Fair"}, "high": {"紧张", "High Load"},
	"available": {"可用", "Available"}, "used": {"已用", "Used"}, "total": {"共", "Total"},
	"uptime": {"运行时长", "Uptime"}, "cores": {"核心", "Cores"}, "swap": {"交换区", "Swap"},
	"liveRates": {"实时速率", "Live rates"}, "noFan": {"未检测到风扇", "No fan detected"},
	"fans": {"风扇", "Fans"}, "managed": {"由系统托管调速", "OS-managed"}, "manual": {"已接管", "Manual"},
	"auto": {"自动", "Auto"}, "cool": {"降温", "Cool"}, "full": {"全速", "Full Speed"},
	"removeHelper": {"移除风扇控制组件", "Remove fan control component"},
	"gpuUnknown":   {"GPU 数据不可用", "GPU data unavailable"}, "renderer": {"渲染器", "Renderer"}, "vram": {"显存占用", "GPU memory"},
	"cycles": {"循环次数", "Cycles"}, "batteryHealth": {"健康度", "Health"},
	"charged": {"已充满", "Fully charged"}, "charging": {"充电中", "Charging"}, "plugged": {"已接通电源", "Plugged in"}, "onBattery": {"使用电池", "On battery"},
	"minutes": {"%d 分钟", "%d min"}, "hoursMinutes": {"%d 小时 %d 分", "%d h %d min"},
	"toFull": {"充满还需 %s", "%s to full"}, "left": {"剩余 %s", "%s left"},
	"uptimeDays": {"%d 天 %d 小时", "%dd %dh"}, "uptimeHours": {"%d 小时 %d 分", "%dh %dm"}, "uptimeMinutes": {"%d 分钟", "%dm"},
	"clear":        {"各项指标均在正常范围", "All metrics within normal range"},
	"factorDisk":   {"主因：磁盘空间不足", "Main: Low disk space"},
	"factorSwap":   {"主因：内存不够用，正在频繁换页", "Main: Memory exhausted, swapping heavily"},
	"factorMemory": {"主因：内存占用偏高", "Main: High memory usage"},
	"factorCPU":    {"主因：CPU 持续高负载", "Main: Sustained high CPU load"},
	"factorUptime": {"主因：已经很久没重启", "Main: Up for a long time without a restart"},
	"processes":    {"活动进程", "Active Processes"}, "search": {"搜索进程、:端口、用户…", "Search processes, :port, user…"},
	"all": {"全部", "All"}, "apps": {"App", "Apps"}, "myProcesses": {"我的进程", "My Processes"}, "system": {"系统", "System"},
	"name": {"名称", "Name"}, "threads": {"线程", "Threads"}, "cpuTime": {"CPU 时间", "CPU Time"}, "user": {"用户", "User"},
	"searchHelp":  {"按名称、路径、PID、端口或用户搜索", "Search by name, path, PID, port, or user"},
	"searchQuery": {"搜索“%s”", "Search “%s”"}, "processCount": {"%d 个进程", "%d processes"},
	"ports": {"端口", "Ports"}, "noPorts": {"当前无进程占用监听端口", "No processes listening on ports"},
	"noMatchingPorts": {"未找到占用端口 :%s 的进程", "No process listening on :%s"},
	"portTip":         {"点击条目快速筛选 · 支持 cpu>10, mem>1GB, -排除", "Click to filter · Supports cpu>10, mem>1GB, -exclude"},
	"hintMulti":       {"同时搜索多个", "Search multiple"}, "hintCPU": {"填写 CPU 阈值，默认单位 %", "Threshold in %"},
	"hintMem": {"阈值默认 MB，支持 m / g", "Value in MB; m / g accepted"}, "hintUser": {"填写运行用户", "Enter process owner"},
	"hintPort": {"监听 3000 端口", "Listen on port 3000"}, "hintExclude": {"排除 helper", "Exclude helper"},
	"quit": {"退出", "Quit"}, "forceQuit": {"强制退出", "Force Quit"}, "reveal": {"打开文件所在位置", "Open File Location"},
	"details": {"进程详情", "Process Details"}, "rss": {"常驻内存", "Physical Memory (RSS)"}, "vms": {"虚拟内存", "Virtual Memory"},
	"read": {"磁盘读取", "Disk Read"}, "write": {"磁盘写入", "Disk Write"}, "started": {"启动时间", "Start Time"}, "state": {"状态", "Status"},
	"parent": {"父进程", "Parent Process"}, "command": {"命令", "Command Line"}, "copy": {"复制命令行", "Copy Command"}, "copied": {"已复制", "Copied"},
	"stateRunning": {"运行中", "Running"}, "stateSleeping": {"休眠", "Sleeping"}, "stateIdle": {"空闲", "Idle"},
	"stateStopped": {"已暂停", "Stopped"}, "stateZombie": {"僵尸", "Zombie"}, "stateNormal": {"正常", "Normal"},
	"justNow": {"刚刚", "just now"}, "minutesAgo": {"%d分钟前", "%dm ago"}, "hoursAgo": {"%d小时前", "%dh ago"}, "daysAgo": {"%d天前", "%dd ago"},
	"stamp": {"01月02日 15:04", "Jan 02, 15:04"},
}

func (s *StatusPage) tr(key string, args ...any) string {
	pair, ok := statusCopy[key]
	if !ok {
		panic("missing status copy: " + key)
	}
	v := pair[0]
	if s.Language == English {
		v = pair[1]
	}
	if len(args) != 0 {
		return fmt.Sprintf(v, args...)
	}
	return v
}

func (s *StatusPage) uptime(seconds uint64) string {
	if seconds >= 86400 {
		return s.tr("uptimeDays", seconds/86400, seconds%86400/3600)
	}
	if seconds >= 3600 {
		return s.tr("uptimeHours", seconds/3600, seconds%3600/60)
	}
	return s.tr("uptimeMinutes", seconds/60)
}

func (s *StatusPage) startTime(start uint64, now time.Time) string {
	if start == 0 {
		return "-"
	}
	seconds := uint64(max(int64(0), now.Unix()-int64(start)))
	ago := s.tr("justNow")
	if seconds >= 86400 {
		ago = s.tr("daysAgo", seconds/86400)
	} else if seconds >= 3600 {
		ago = s.tr("hoursAgo", seconds/3600)
	} else if seconds >= 60 {
		ago = s.tr("minutesAgo", seconds/60)
	}
	return time.Unix(int64(start), 0).Local().Format(s.tr("stamp")) + " · " + ago
}
