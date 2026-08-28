//! plist 文件读取（macOS）
//!
//! 读的是 `plutil(1)`，不是自己解析二进制/XML plist：LaunchAgent 的 plist
//! 两种格式都有，自己实现要拉一个解析库并承担格式兼容风险，而这里的调用
//! 频次很低（只在扫描损坏登录项时对少量文件调用）。
//!
//! 这里的函数只提供**机制**（能不能读、读到什么），不含「什么算损坏」的
//! 判断——那是领域策略，留在 `core::categories`。以前整段逻辑连同两次
//! `Command::new("plutil")` 一起写在 core 里，属于领域层直接调外部进程。

use std::path::Path;

/// 把 plist 转成 JSON 值读取。
///
/// 文件不存在、无权读取、语法非法、`plutil` 失败或输出不是合法 JSON 时
/// 返回 `None`。成功解析但某个业务字段不存在，则由调用方在返回的 Value
/// 上得到明确的“缺键”；不能再把这两种情况折叠成同一个 `None`，否则权限
/// 失败会被错误地当成配置损坏并授权删除。
pub fn read_value(plist: &Path) -> Option<serde_json::Value> {
    std::process::Command::new("plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(plist)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| serde_json::from_slice(&output.stdout).ok())
}
