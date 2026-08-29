use crate::core::inuse::{Busy, SpotCheck};
use std::collections::HashMap;
use std::path::PathBuf;

/// Windows 暂不实现句柄检测；不能把“未实现”伪装成检测失败。
pub fn detect_inuse(_paths: &[PathBuf]) -> HashMap<PathBuf, Busy> {
    HashMap::new()
}

/// 删除前只保留跨平台活 SQLite 文件闸门，不假装实现 Windows 句柄检测。
pub fn spot_check_inuse(paths: &[PathBuf]) -> HashMap<PathBuf, SpotCheck> {
    crate::platform::spot_check_without_handle_probe(paths)
}
