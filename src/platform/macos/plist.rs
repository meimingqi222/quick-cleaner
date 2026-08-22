//! plist 文件读取（macOS）
//!
//! 读的是 `plutil(1)`，不是自己解析二进制/XML plist：LaunchAgent 的 plist
//! 两种格式都有，自己实现要拉一个解析库并承担格式兼容风险，而这里的调用
//! 频次很低（只在扫描损坏登录项时对少量文件调用）。
//!
//! 这两个函数只提供**机制**（能不能读、读到什么），不含「什么算损坏」的
//! 判断——那是领域策略，留在 `core::categories`。以前整段逻辑连同两次
//! `Command::new("plutil")` 一起写在 core 里，属于领域层直接调外部进程。

use std::path::Path;

/// 读 plist 里某个 key 的标量值。
///
/// `key` 支持 `plutil -extract` 的路径语法（如 `ProgramArguments.0`）。
/// 文件不存在、语法非法、key 不存在、值为空，一律返回 `None`——调用方
/// 只关心「读没读到」，不需要区分失败原因。
pub fn read_scalar(plist: &Path, key: &str) -> Option<String> {
    std::process::Command::new("plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(plist)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
