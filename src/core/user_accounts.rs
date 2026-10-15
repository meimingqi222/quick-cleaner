//! 本地用户账户空间用量探测
//!
//! 枚举当前系统上的本地用户账户目录（macOS 的 `/Users/*` 或 Windows 的 `%SystemDrive%\Users\*`），
//! 标识当前登录用户，只读取账户目录，不在托盘入口递归统计占用。

use std::path::PathBuf;

/// 本地用户账户信息与磁盘占用
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalAccount {
    /// 账户名称（即用户目录文件夹名）
    pub name: String,
    /// 用户主目录完整路径
    pub path: PathBuf,
    /// 是否为当前操作用户
    pub is_current: bool,
    /// 完整统计的目录占用；未统计时为 None。
    pub size_bytes: Option<u64>,
}

/// 枚举系统上的本地用户账户列表
pub fn list_local_accounts() -> Vec<LocalAccount> {
    let current_home = crate::platform::user_home();
    let users_dir = get_system_users_dir();

    let mut accounts = Vec::new();
    let entries = match std::fs::read_dir(&users_dir) {
        Ok(e) => e,
        Err(_) => return accounts,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // 过滤系统模板与占位目录
        if should_skip_user_dir(&name) {
            continue;
        }

        let is_current = current_home.as_ref().map(|h| h == &path).unwrap_or(false);

        accounts.push(LocalAccount {
            name,
            path,
            is_current,
            size_bytes: None,
        });
    }

    // 排序：当前用户排在第一位，其余按字母排序
    accounts.sort_by(|a, b| {
        b.is_current
            .cmp(&a.is_current)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    accounts
}

/// 系统用户目录根路径
fn get_system_users_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/Users")
    }
    #[cfg(windows)]
    {
        if let Some(home) = crate::platform::user_home() {
            if let Some(parent) = home.parent() {
                return parent.to_path_buf();
            }
        }
        if let Ok(drive) = std::env::var("SystemDrive") {
            return PathBuf::from(format!(r"{}\Users", drive));
        }
        PathBuf::from(r"C:\Users")
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        PathBuf::from("/home")
    }
}

/// 跳过非真实账户目录
fn should_skip_user_dir(name: &str) -> bool {
    let lower = name.to_lowercase();
    // 隐藏目录（如 .localized）
    if name.starts_with('.') {
        return true;
    }

    #[cfg(target_os = "macos")]
    {
        // Shared 目录保留作为公共共享目录，但不算作个人登录账户
        if lower == "shared" {
            return false;
        }
    }

    #[cfg(windows)]
    {
        if matches!(
            lower.as_str(),
            "default" | "default user" | "all users" | "public"
        ) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_local_accounts_finds_current() {
        let accounts = list_local_accounts();
        // 如果当前环境能拿到 user_home，则列表中应当包含当前用户且排在第一位
        if crate::platform::user_home().is_some() {
            assert!(!accounts.is_empty(), "本地用户列表不应为空");
            assert!(accounts[0].is_current, "当前用户应当排在首位");
        }
    }

    #[test]
    fn test_should_skip_user_dir() {
        assert!(should_skip_user_dir(".hidden"));
        assert!(should_skip_user_dir(".localized"));
        #[cfg(windows)]
        {
            assert!(should_skip_user_dir("Default"));
            assert!(should_skip_user_dir("Public"));
        }
        assert!(!should_skip_user_dir("myuser"));
    }
}
