package ui

// Copy follows the currently rendered Rust tray, not unused legacy tray strings.
type trayPopoverCopy struct {
	title, disks, hardware, loading, free, volumes, memory, unavailable string
	topMemory, analyze, processes, tools, main, quit, close             string
}

func trayPopoverStrings(l Language) trayPopoverCopy {
	if l == English {
		return trayPopoverCopy{
			title: "QuickCleaner Space Lens", disks: "Disk Space Usage", hardware: "Hardware Status",
			loading: "Loading…", free: "Free %s", volumes: "(%d vols)", memory: "RAM %d%%",
			unavailable: "Temperature unavailable", topMemory: "Top Memory",
			analyze: "Analyze", processes: "Processes",
			tools: "Quick Tools", main: "Open QuickCleaner", quit: "Quit QuickCleaner", close: "Close Window",
		}
	}
	return trayPopoverCopy{
		title: "QuickCleaner 空间透镜", disks: "磁盘空间占用", hardware: "硬件核心状态",
		loading: "正在读取…", free: "剩余 %s", volumes: "(%d 卷)", memory: "内存 %d%%",
		unavailable: "温度不可用", topMemory: "内存排行", analyze: "分析", processes: "查看进程", tools: "快捷功能",
		main: "打开主界面", quit: "退出程序", close: "关闭窗口",
	}
}
