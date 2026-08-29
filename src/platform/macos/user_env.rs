//! macOS 用户目录语义。

use std::path::PathBuf;

pub fn user_home() -> Option<PathBuf> {
    dirs::home_dir()
}

pub fn user_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir()
}

pub fn user_data_dir() -> Option<PathBuf> {
    dirs::data_dir()
}

pub fn user_temp_dir() -> Option<PathBuf> {
    Some(std::env::temp_dir())
}
