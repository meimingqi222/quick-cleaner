package ui

type productDialogCopy struct {
	cancel, delete, uninstall, kill, install, release, forceRelease, warning                                                string
	update, checking, download, downloading, verifying, restart, installing, ready, skip, later, releasePage, retry, failed string
	fdaTitle, fdaDesc, notice, settings, check, configureLater, dontAsk                                                     string
	steps                                                                                                                   [3][2]string
}

func productDialogStrings(l Language) productDialogCopy {
	if l == English {
		return productDialogCopy{
			cancel: "Cancel", delete: "Delete Permanently", uninstall: "Uninstall", kill: "End Process", install: "Install", release: "Quit apps and clean", forceRelease: "Force quit and clean",
			warning: "The target is under ~/Library/Application Support - application data lives here (chat history, password vaults, local databases) and cannot be recovered once permanently deleted. Make sure you know what these directories contain.",
			update:  "App update", checking: "Checking…", download: "Download", downloading: "Downloading…", verifying: "Verifying…", restart: "Restart to install", installing: "Installing…", ready: "The update is downloaded and verified. Restart to finish installing.", skip: "Skip this version", later: "Later", releasePage: "Open release page", retry: "Retry", failed: "Update failed",
			fdaTitle: "Grant Full Disk Access", fdaDesc: "macOS privacy protections restrict access to Safari caches, Mail, and system data. Full Disk Access enables QuickCleaner to perform a comprehensive deep clean.",
			notice: "💡 Even without authorization, you can still clean third-party caches and dev builds. Full access unlocks deep cleaning for Apple apps and system data.", settings: "Open System Settings", check: "Check Status", configureLater: "Configure Later", dontAsk: "Don't show automatically again",
			steps: [3][2]string{{"1. Open System Settings", "Click the button below to navigate directly to Privacy & Security → Full Disk Access"}, {"2. Locate QuickCleaner", "Find \"QuickCleaner\" in the application list (or click \"+\" to add it)"}, {"3. Enable the Switch", "Toggle the switch on, return here, and click \"Check Status\" to finish"}},
		}
	}
	return productDialogCopy{
		cancel: "取消", delete: "确认永久删除", uninstall: "确认卸载", kill: "结束进程", install: "安装", release: "退出应用后清理", forceRelease: "强制结束后清理",
		warning: "目标位于 ~/Library/Application Support：这里存放应用数据（聊天记录、密码库、本地数据库等），永久删除后无法恢复。请确认你了解这些目录的用途。",
		update:  "应用更新", checking: "正在检查…", download: "下载更新", downloading: "正在下载…", verifying: "正在校验…", restart: "重启安装", installing: "正在安装…", ready: "新版本已下载并校验通过，重启后即可完成安装。", skip: "跳过此版本", later: "稍后", releasePage: "打开发布页", retry: "重试", failed: "更新失败",
		fdaTitle: "授予「完全磁盘访问权限」", fdaDesc: "macOS 系统安全机制限制了对 Safari 缓存、邮件等系统目录的读取。开启完全磁盘访问权限后，QuickCleaner 可以进行更彻底的深度扫描与清理。",
		notice: "💡 即使暂不授权，您仍可正常清理第三方应用缓存和开发构建产物；开启后将解锁 Apple 自带应用与系统垃圾的深度清理。", settings: "一键打开系统设置", check: "检查授权状态", configureLater: "稍后配置", dontAsk: "不再自动弹出提示",
		steps: [3][2]string{{"1. 打开系统设置", "点击下方按钮，将自动直达「隐私与安全性 → 完全磁盘访问」页面"}, {"2. 找到 QuickCleaner", "在应用列表中找到「QuickCleaner」（若不在列表中可点击「+」添加）"}, {"3. 开启权限开关", "开启开关后返回本软件，点击「检查授权状态」即可完成配置"}},
	}
}
