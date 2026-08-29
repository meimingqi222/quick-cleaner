//! 清理历史审计：每次清理完成后追加一行 JSON 到 `history.jsonl`。
//!
//! 存在的理由和 `cleaner::audit` 一致但互补：audit 是内存里的当批日志，
//! 进程退出就没了；分类清理又是**永久删除**（无回收站），用户一周后回来
//! 说「上周那次清理好像把什么删错了」的时候，没有持久记录就等于什么都
//! 没有。这个文件是用户唯一可以翻旧账的地方，和 `Settings.whitelist`
//! 共同构成「被误删一次之后的自保手段」的另一半。
//!
//! # 格式
//!
//! JSON Lines（每行一条完整 JSON，自描述，无需版本迁移）：
//!
//! ```json
//! {"ts":"2026-08-28T10:15:30+08:00","action":"category_clean","targets":["/path/a"],"ok":3,"skipped":1,"failed":0,"freed_bytes":12345}
//! ```
//!
//! # 存放与轮转
//!
//! 与 settings.json 同目录（`%APPDATA%\QuickCleaner\` /
//! `~/Library/Application Support/QuickCleaner/`）。封顶 [`MAX_BYTES`]，
//! 超限时截掉头部旧记录只留尾部——清理记录是「最近的事最有用」型数据，
//! 一个月前的记录既不参与排障也不值得占空间。

use std::path::PathBuf;

/// 单条历史记录。字段名即 JSON key，保持精简——文件是逐条追加的，
/// 每条记录越长文件轮转得越快。
#[derive(serde::Serialize)]
pub struct HistoryEntry {
    /// 本地时间（带时区），ISO 8601。用户排障时对照的是本地时钟。
    ts: String,
    /// 动作标识：`category_clean`（分类清理）/ `residual_clean`（残留）/
    /// `arbitrary_clean`（磁盘透镜手选路径）。
    action: &'static str,
    /// 用户勾选的那一层目标清单。不展开递归内容——见 `cleaner::audit`
    /// 的同样取舍：一次清理动辄几十万个文件，全记下来文件先被自己撑爆。
    targets: Vec<String>,
    ok: usize,
    skipped: usize,
    failed: usize,
    freed_bytes: u64,
}

/// 文件封顶字节数。超限时截掉头部，只保留尾部 [`KEEP_TAIL`]。
const MAX_BYTES: u64 = 1024 * 1024;
/// 截断后保留的尾部大小。从新行边界开始，不留下半截 JSON。
const KEEP_TAIL: u64 = 512 * 1024;

/// 追加一条清理历史。尽力而为：写不进（权限、磁盘满）只是丢一条记录，
/// 不值得打断用户刚完成的清理，因此不返回错误。
pub fn record(
    action: &'static str,
    targets: &[PathBuf],
    ok: usize,
    skipped: usize,
    failed: usize,
    freed_bytes: u64,
) {
    let Some(path) = crate::core::settings::config_dir().map(|d| d.join("history.jsonl")) else {
        return;
    };
    if let Some(dir) = path.parent() {
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    let entry = HistoryEntry {
        ts: chrono::Local::now()
            .format("%Y-%m-%dT%H:%M:%S%:z")
            .to_string(),
        action,
        targets: targets
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        ok,
        skipped,
        failed,
        freed_bytes,
    };
    let Ok(mut line) = serde_json::to_string(&entry) else {
        return;
    };
    line.push('\n');

    // 轮转：超限时截头部。先读再写回比 seek 截断简单可靠——文件只有
    // 1MB，读写一次的代价可以忽略；而且「先截断、后写入」在这里安全：
    // 失败了损失的只是历史记录本身，不是用户数据。
    let mut content = std::fs::read_to_string(&path).unwrap_or_default();
    if content.len() as u64 > MAX_BYTES {
        let tail_start = content.len() - KEEP_TAIL as usize;
        // 从第一个完整行的开头截，不留半截 JSON 行
        let start = content[tail_start..]
            .find('\n')
            .map(|i| tail_start + i + 1)
            .unwrap_or(tail_start);
        content = content[start..].to_string();
    }
    content.push_str(&line);
    let _ = std::fs::write(&path, content);
}

#[cfg(test)]
mod tests {
    /// 轮转逻辑（不含文件 IO）：超限截头、从新行边界开始。
    #[test]
    fn rotation_logic_keeps_complete_tail_lines() {
        // 抽出纯字符串处理来测：轮转触发时剩余内容的第一行必须是完整 JSON
        let make_line = |i: usize| format!("{{\"n\":{}}}\n", i);
        let mut content: String = (0..100).map(make_line).collect();
        let max_bytes = 500u64;
        let keep_tail = 250u64;
        if content.len() as u64 > max_bytes {
            let tail_start = content.len() - keep_tail as usize;
            let start = content[tail_start..]
                .find('\n')
                .map(|i| tail_start + i + 1)
                .unwrap_or(tail_start);
            content = content[start..].to_string();
        }
        assert!(
            content.starts_with('{'),
            "截断后必须从完整行开始，不得留半截"
        );
        assert!(content.len() as u64 <= keep_tail + 32);
    }
}
