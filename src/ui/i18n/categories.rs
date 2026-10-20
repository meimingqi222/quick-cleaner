//! 清理类别的界面文案：名称、描述、图标与安全等级标签。
//!
//! 类别身份在 `core::categories::CategoryId`，策略（安全等级、开发类
//! 分组）在 `rules/engine.toml`，这里只放逐字搬迁的展示文案。

use crate::core::categories::CategoryId;
use crate::core::i18n::Language;

/// 类别显示名。
pub fn tr_category_name(lang: Language, id: CategoryId) -> &'static str {
    match lang {
        Language::Zh => match id {
            CategoryId::SystemTemp => "系统临时文件",
            CategoryId::UserTemp => "用户临时文件",
            CategoryId::UserCache => "应用缓存",
            CategoryId::BrowserCache => "浏览器缓存",
            CategoryId::PackageCache => "包管理缓存",
            CategoryId::UpdaterPackages => "应用更新包",
            CategoryId::Logs => "日志与崩溃转储",
            CategoryId::RecycleBin => "回收站 / 废纸篓",
            CategoryId::Thumbnails => "缩略图缓存",
            CategoryId::BrokenLoginItems => "损坏的登录项",
            CategoryId::AiAgents => "AI 编程助手缓存",
            CategoryId::DevBuild => "项目构建产物与依赖",
            CategoryId::DevWorktrees => "AI agent 临时 worktree",
            CategoryId::LocalSnapshots => "APFS 本地快照",
            CategoryId::IosBackup => "iOS 设备备份",
            CategoryId::OldIdeData => "旧版 IDE 数据",
            CategoryId::DockerImages => "冗余 Docker 镜像",
        },
        Language::En => match id {
            CategoryId::SystemTemp => "System Temp Files",
            CategoryId::UserTemp => "User Temp Files",
            CategoryId::UserCache => "Application Cache",
            CategoryId::BrowserCache => "Browser Cache",
            CategoryId::PackageCache => "Package Manager Cache",
            CategoryId::UpdaterPackages => "Application Update Packages",
            CategoryId::Logs => "Logs & Crash Dumps",
            CategoryId::RecycleBin => "Recycle Bin / Trash",
            CategoryId::Thumbnails => "Thumbnail Cache",
            CategoryId::BrokenLoginItems => "Broken Login Items",
            CategoryId::AiAgents => "AI Assistant Cache",
            CategoryId::DevBuild => "Build Artifacts & Deps",
            CategoryId::DevWorktrees => "AI Agent Git Worktrees",
            CategoryId::LocalSnapshots => "APFS Local Snapshots",
            CategoryId::IosBackup => "iOS Device Backup",
            CategoryId::OldIdeData => "Old IDE Version Data",
            CategoryId::DockerImages => "Redundant Docker Images",
        },
    }
}

/// 类别描述（卡片副标题）。
pub fn tr_category_desc(lang: Language, id: CategoryId) -> &'static str {
    match lang {
        Language::Zh => match id {
            CategoryId::SystemTemp => "系统临时文件与系统更新残留",
            CategoryId::UserTemp => "用户主目录下的应用临时文件",
            CategoryId::UserCache => "应用明确存放在缓存目录中的可重建数据",
            CategoryId::BrowserCache => "Chrome / Edge / Safari 等浏览器的缓存数据",
            CategoryId::PackageCache => "npm / pnpm / cargo / go 等包管理器缓存",
            CategoryId::UpdaterPackages => {
                "已下载的更新包与更新器暂存，删了最多多下一次更新，不丢任何数据"
            }
            CategoryId::Logs => "系统与应用日志、崩溃转储",
            CategoryId::RecycleBin => "回收站/废纸篓中已删除的文件",
            CategoryId::Thumbnails => "系统缩略图缓存，可安全重建",
            CategoryId::BrokenLoginItems => "引用目标已不存在或配置已失效的启动项",
            CategoryId::AiAgents => {
                "Claude Code / Codex / Trae / Cursor 等 AI 编程工具的会话记录与缓存"
            }
            CategoryId::DevBuild => {
                "代码目录下的 node_modules / target / .venv / bin·obj 等，可重新构建"
            }
            CategoryId::DevWorktrees => "AI agent 留下的临时 git worktree，可能含未提交改动",
            CategoryId::LocalSnapshots => "APFS 本地快照，macOS「磁盘莫名爆满」的头号原因",
            CategoryId::IosBackup => {
                "iTunes / Finder 创建的 iOS 设备完整备份，单个可达 100 GB+"
            }
            CategoryId::OldIdeData => {
                "Application Support 下 JetBrains 的按版本数据目录，当前版本之外的旧目录，移入废纸篓"
            }
            CategoryId::DockerImages => {
                "悬空镜像、未被任何容器使用的镜像与同仓库旧版本标签，经 docker image rm 释放"
            }
        },
        Language::En => match id {
            CategoryId::SystemTemp => "System temporary files and update leftovers",
            CategoryId::UserTemp => "Application temporary files under user profile",
            CategoryId::UserCache => "Rebuildable data stored in application cache directories",
            CategoryId::BrowserCache => "Cache files from Chrome, Edge, Firefox, Safari",
            CategoryId::PackageCache => "Caches from npm, pnpm, Cargo, Go, pip, etc.",
            CategoryId::UpdaterPackages => {
                "Downloaded update packages and updater staging; costs a re-download, loses no data"
            }
            CategoryId::Logs => "System and application event logs and crash dumps",
            CategoryId::RecycleBin => "Deleted files in Recycle Bin or Trash",
            CategoryId::Thumbnails => "System thumbnail cache, safe to rebuild",
            CategoryId::BrokenLoginItems => {
                "Startup entries whose executable is missing or configuration is invalid"
            }
            CategoryId::AiAgents => {
                "Session records and caches from Claude, Cursor, Trae, etc."
            }
            CategoryId::DevBuild => {
                "node_modules, target, .venv, bin/obj in projects, rebuildable"
            }
            CategoryId::DevWorktrees => {
                "Temporary worktrees created by AI agents, may contain uncommitted edits"
            }
            CategoryId::LocalSnapshots => {
                "APFS local snapshots, the #1 cause of mysterious disk-full on macOS"
            }
            CategoryId::IosBackup => {
                "Full iOS device backups created by iTunes / Finder, can be 100 GB+ each"
            }
            CategoryId::OldIdeData => {
                "Per-version JetBrains data dirs under Application Support, excluding the newest, moved to Trash"
            }
            CategoryId::DockerImages => {
                "Dangling images, tags unused by any container and old versions, freed via docker image rm"
            }
        },
    }
}
