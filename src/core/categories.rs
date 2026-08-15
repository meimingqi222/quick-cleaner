//! 垃圾清理类别与扫描目标规则定义

use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Safety {
    Safe,
    Caution,
    Danger,
}

impl Safety {
    pub fn label(&self) -> &'static str {
        match self {
            Safety::Safe => "安全清理",
            Safety::Caution => "注意",
            Safety::Danger => "危险",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CategoryId {
    SystemTemp,
    UserTemp,
    BrowserCache,
    PackageCache,
    Logs,
    RecycleBin,
    Thumbnails,
}

impl CategoryId {
    pub const ALL: [CategoryId; 7] = [
        CategoryId::SystemTemp,
        CategoryId::UserTemp,
        CategoryId::BrowserCache,
        CategoryId::PackageCache,
        CategoryId::Logs,
        CategoryId::RecycleBin,
        CategoryId::Thumbnails,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            CategoryId::SystemTemp => "系统临时文件",
            CategoryId::UserTemp => "用户临时文件",
            CategoryId::BrowserCache => "浏览器缓存",
            CategoryId::PackageCache => "包管理缓存",
            CategoryId::Logs => "日志与崩溃转储",
            CategoryId::RecycleBin => "回收站 / 废纸篓",
            CategoryId::Thumbnails => "缩略图缓存",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            CategoryId::SystemTemp => "🗑",
            CategoryId::UserTemp => "📂",
            CategoryId::BrowserCache => "🌐",
            CategoryId::PackageCache => "📦",
            CategoryId::Logs => "📝",
            CategoryId::RecycleBin => "♻️",
            CategoryId::Thumbnails => "🖼",
        }
    }

    pub fn desc(&self) -> &'static str {
        match self {
            CategoryId::SystemTemp => "系统临时文件与系统更新残留",
            CategoryId::UserTemp => "用户主目录下的应用临时文件",
            CategoryId::BrowserCache => "Chrome / Edge / Safari 等浏览器的缓存数据",
            CategoryId::PackageCache => "npm / pnpm / cargo / go 等包管理器缓存",
            CategoryId::Logs => "系统与应用日志、崩溃转储",
            CategoryId::RecycleBin => "回收站/废纸篓中已删除的文件",
            CategoryId::Thumbnails => "系统缩略图缓存，可安全重建",
        }
    }

    pub fn safety(&self) -> Safety {
        match self {
            CategoryId::SystemTemp => Safety::Safe,
            CategoryId::UserTemp => Safety::Safe,
            CategoryId::BrowserCache => Safety::Caution,
            CategoryId::PackageCache => Safety::Caution,
            CategoryId::Logs => Safety::Safe,
            CategoryId::RecycleBin => Safety::Caution,
            CategoryId::Thumbnails => Safety::Safe,
        }
    }
}

/// 一个清理目标：一个具体目录路径 + 描述
#[derive(Clone, Debug)]
pub struct ScanTarget {
    pub path: PathBuf,
    pub label: String,
    pub category: CategoryId,
}

/// 返回所有类别对应的扫描目标（支持跨平台）。
pub fn all_targets() -> Vec<ScanTarget> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let mut t: Vec<ScanTarget> = Vec::new();

    #[cfg(windows)]
    {
        let local = dirs::cache_dir().unwrap_or_else(|| home.join("AppData\\Local"));
        let windows = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()));

        // 系统临时
        t.push(target(windows.join("Temp"), "Windows\\Temp", CategoryId::SystemTemp));
        t.push(target(windows.join("SoftwareDistribution\\Download"), "Windows 更新缓存", CategoryId::SystemTemp));
        t.push(target(windows.join("SystemTemp"), "SystemTemp", CategoryId::SystemTemp));
        t.push(target(PathBuf::from("C:\\tmp"), "C:\\tmp", CategoryId::SystemTemp));

        // 用户临时
        t.push(target(std::env::temp_dir(), "%TEMP%", CategoryId::UserTemp));
        t.push(target(local.join("CrashDumps"), "CrashDumps 崩溃转储", CategoryId::UserTemp));

        // 浏览器缓存
        t.push(target(local.join("Google\\Chrome\\User Data\\Default\\Cache"), "Chrome 缓存", CategoryId::BrowserCache));
        t.push(target(local.join("Google\\Chrome\\User Data\\Default\\Code Cache"), "Chrome Code Cache", CategoryId::BrowserCache));
        t.push(target(local.join("Microsoft\\Edge\\User Data\\Default\\Cache"), "Edge 缓存", CategoryId::BrowserCache));
        t.push(target(local.join("Microsoft\\Edge\\User Data\\Default\\Code Cache"), "Edge Code Cache", CategoryId::BrowserCache));

        // 包管理缓存
        t.push(target(local.join("npm-cache"), "npm 缓存", CategoryId::PackageCache));
        t.push(target(home.join("npm-cache"), "npm 缓存(home)", CategoryId::PackageCache));
        t.push(target(home.join(".pnpm-store"), "pnpm store", CategoryId::PackageCache));
        t.push(target(home.join(".pnpm-cache"), "pnpm cache", CategoryId::PackageCache));
        t.push(target(home.join(".cargo\\registry"), "cargo registry 缓存", CategoryId::PackageCache));
        t.push(target(home.join(".rustup\\downloads"), "rustup 下载缓存", CategoryId::PackageCache));
        t.push(target(home.join("go\\pkg\\mod"), "go module 缓存", CategoryId::PackageCache));
        t.push(target(local.join("go-build"), "go build 缓存", CategoryId::PackageCache));
        t.push(target(home.join(".bun"), "bun 缓存", CategoryId::PackageCache));
        t.push(target(home.join(".gradle\\caches"), "gradle 缓存", CategoryId::PackageCache));
        t.push(target(home.join(".nuget\\packages"), "nuget 包缓存", CategoryId::PackageCache));
        t.push(target(home.join(".m2\\repository"), "maven 本地仓库", CategoryId::PackageCache));
        t.push(target(home.join(".cache"), "~/.cache", CategoryId::PackageCache));
        t.push(target(local.join("uv\\cache"), "uv 缓存", CategoryId::PackageCache));
        t.push(target(local.join("pip\\cache"), "pip 缓存", CategoryId::PackageCache));

        // 日志
        t.push(target(windows.join("Logs"), "Windows\\Logs", CategoryId::Logs));
        t.push(target(windows.join("System32\\LogFiles"), "LogFiles", CategoryId::Logs));
        t.push(target(windows.join("System32\\winevt\\Logs"), "事件日志", CategoryId::Logs));
        t.push(target(local.join("D3DSCache"), "D3D 着色器缓存", CategoryId::Logs));

        // 回收站（只统计当前用户自己的 SID 子目录）
        if let Some(sid) = crate::platform::windows::security::current_user_sid() {
            for letter in 'A'..='Z' {
                let rb = PathBuf::from(format!("{letter}:\\$Recycle.Bin")).join(&sid);
                if rb.exists() {
                    t.push(target(rb, &format!("{letter}: 回收站"), CategoryId::RecycleBin));
                }
            }
        }

        // 缩略图
        t.push(target(local.join("Microsoft\\Windows\\Explorer"), "缩略图/图标缓存", CategoryId::Thumbnails));
    }

    #[cfg(target_os = "macos")]
    {
        let cache = home.join("Library/Caches");
        let logs = home.join("Library/Logs");

        // 系统与用户临时/缓存
        t.push(target(PathBuf::from("/private/tmp"), "/private/tmp", CategoryId::SystemTemp));
        t.push(target(PathBuf::from("/private/var/tmp"), "/private/var/tmp", CategoryId::SystemTemp));
        t.push(target(cache.clone(), "~/Library/Caches", CategoryId::UserTemp));

        // 浏览器缓存
        t.push(target(cache.join("Google/Chrome/Default"), "Chrome 缓存", CategoryId::BrowserCache));
        t.push(target(cache.join("com.apple.Safari"), "Safari 缓存", CategoryId::BrowserCache));
        t.push(target(cache.join("com.microsoft.edgemac"), "Edge 缓存", CategoryId::BrowserCache));

        // 包管理缓存
        t.push(target(home.join(".npm/_cacache"), "npm 缓存", CategoryId::PackageCache));
        t.push(target(home.join(".cargo/registry"), "cargo 缓存", CategoryId::PackageCache));
        t.push(target(home.join(".rustup/downloads"), "rustup 缓存", CategoryId::PackageCache));
        t.push(target(home.join("go/pkg/mod"), "go 缓存", CategoryId::PackageCache));
        t.push(target(home.join(".cache"), "~/.cache", CategoryId::PackageCache));
        t.push(target(home.join("Library/Caches/Homebrew"), "Homebrew 缓存", CategoryId::PackageCache));

        // 日志
        t.push(target(logs, "~/Library/Logs", CategoryId::Logs));
        t.push(target(PathBuf::from("/Library/Logs"), "/Library/Logs", CategoryId::Logs));

        // 废纸篓
        t.push(target(home.join(".Trash"), "废纸篓", CategoryId::RecycleBin));
    }

    t
}

fn target(path: PathBuf, label: &str, category: CategoryId) -> ScanTarget {
    ScanTarget {
        path,
        label: label.to_string(),
        category,
    }
}
