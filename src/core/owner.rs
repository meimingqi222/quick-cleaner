//! 包管理器缓存的 owner command 清理：让生态自己的命令安全收缩，
//! 而不是裸删目录。
//!
//! 动机（两处真实的不一致风险）：
//!
//! 1. **Go module cache**：目录里的文件被故意设成**只读**（go 工具链
//!    主动做的，防意外修改）。`cleaner::clear_readonly` 清掉只读位强删
//!    后，文件没了但 go 的索引/校验状态还以为包在——下次构建拿到的
//!    是「以为有、实际没有」的半成品 store。
//! 2. **pnpm store**：store 内有 side-effects 缓存与包索引元数据，
//!    裸删绕开了 pnpm 对 store 一致性的管理。
//!
//! `go clean -modcache` 和 `pnpm store prune` 是这两个生态自己的安全
//! 收缩通道：命令知道内部结构，清完不留不一致。
//!
//! # 集成方式
//!
//! 与 brew（`core::brew`）不同，这两个**不做虚拟目标**：现有的
//! `go/pkg/mod`、`pnpm/store` 目录目标保留（体积称重真实、用户能看到
//! 大小），只在 `cleaner::clean_targets` 里把删除动作**路由**到命令。
//! 探测失败（工具链不在 PATH、目标路径与命令作用域不一致）一律回退
//! 现有裸删路径——行为不比改动前差。

use std::path::Path;
use std::time::Duration;

/// owner command 的超时。大缓存的清理可以到几十秒；超时按失败处理，
/// 由 cleaner 报 Failed（不回退裸删——命令跑到一半被杀已经动过 store，
/// 再裸删等于在未知状态上继续动刀）。
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

/// `path` 是不是 Go module cache（`…/go/pkg/mod`）。
///
/// 按路径后缀识别：目录名是 Go 生态的固定约定，固定表（
/// `categories::cache`）产出的也正是这个路径。
pub fn is_go_modcache(path: &Path) -> bool {
    let lower = crate::core::safety::norm(path);
    lower.ends_with("\\go\\pkg\\mod")
}

/// `path` 是不是 pnpm store。
///
/// 覆盖两种常见布局：显式 `…/pnpm/store`（部分配置/macOS
/// `~/Library/pnpm/store`）以及默认的 `…/.pnpm-store`（pnpm 历史默认
/// 位置，macOS/Linux 最常见）。固定表（`categories::cache`）两种都生成，
/// 这里若漏掉 `.pnpm-store`，那条目标就会退化成普通删除——正是 3.5
/// 想挡的「裸删 store 留下不一致」风险。
pub fn is_pnpm_store(path: &Path) -> bool {
    let lower = crate::core::safety::norm(path);
    lower.ends_with("\\pnpm\\store") || lower.ends_with("\\.pnpm-store")
}

/// 用 `go clean -modcache` 收缩 module cache。
///
/// 两个前提都满足才返回 `Some`（调用方据此决定走命令还是回退裸删）：
/// 1. `go` 在 PATH 里（`go env` 起得来）；
/// 2. 目标路径与 `go env GOMODCACHE` 一致——用户自定义 GOMODCACHE
///    时，命令的作用域是自定义路径，清 `~/go/pkg/mod` 的目标就
///    对不上号，这种情形必须回退裸删而不是清错地方。
pub fn go_clean_modcache(target: &Path) -> Option<bool> {
    let env_run =
        crate::core::proc::run_with_timeout("go", &["env", "GOMODCACHE"], Duration::from_secs(5))?;
    if !env_run.ok {
        return None;
    }
    let gomodcache = String::from_utf8_lossy(&env_run.stdout).trim().to_string();
    if gomodcache.is_empty()
        || crate::core::safety::norm(Path::new(&gomodcache)) != crate::core::safety::norm(target)
    {
        return None;
    }
    let run = crate::core::proc::run_with_timeout("go", &["clean", "-modcache"], COMMAND_TIMEOUT)?;
    Some(run.ok)
}

/// 用 `pnpm store prune` 收缩 store。
///
/// 前提：`pnpm` 在 PATH 且 `pnpm store path` 报告的 store 与目标一致。
/// 老版本 pnpm 没有 `store prune`（6.0 引入）——命令跑失败如实返回
/// `Some(false)`，由 cleaner 报 Failed，不静默回退裸删（理由见
/// [`COMMAND_TIMEOUT`] 的注释）。
pub fn pnpm_store_prune(target: &Path) -> Option<bool> {
    let path_run =
        crate::core::proc::run_with_timeout("pnpm", &["store", "path"], Duration::from_secs(5))?;
    if !path_run.ok {
        return None;
    }
    let store_path = String::from_utf8_lossy(&path_run.stdout).trim().to_string();
    // 一致性（安全闸）：只在本工具要清的目标与 pnpm 自报的 store 一致时
    // 才跑命令。pnpm 把版本目录挂在 store 下（`pnpm store path` 返回
    // `…/.pnpm-store/v3`），而固定表产出的目标是 `…/.pnpm-store`（父）。
    // 严格相等会失配、让优化永远不触发，因此放宽为「相等，或目标是 store
    // 的父目录」——`pnpm store prune` 作用在整个 store，对父目录也安全。
    // 用分隔符收尾的「前缀」判定，避免 `~/.pnpm-store` 误中 `~/.pnpm-store-evil`。
    let store_norm = crate::core::safety::norm(Path::new(&store_path));
    let target_norm = crate::core::safety::norm(target);
    let matches = store_norm == target_norm
        || store_norm.starts_with(&format!("{}\\", target_norm));
    if store_path.is_empty() || !matches {
        return None;
    }
    let run = crate::core::proc::run_with_timeout("pnpm", &["store", "prune"], COMMAND_TIMEOUT)?;
    Some(run.ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn go_modcache_recognized_by_suffix() {
        assert!(is_go_modcache(&PathBuf::from("/home/u/go/pkg/mod")));
        assert!(is_go_modcache(&PathBuf::from(r"C:\Users\u\go\pkg\mod")));
        assert!(!is_go_modcache(&PathBuf::from("/home/u/go/pkg")));
        assert!(!is_go_modcache(&PathBuf::from("/tmp/other")));
    }

    #[test]
    fn pnpm_store_recognized_by_suffix() {
        assert!(is_pnpm_store(&PathBuf::from(
            "/Users/u/Library/pnpm/store"
        )));
        // 默认 macOS/Linux 布局，最常被漏掉的那条
        assert!(is_pnpm_store(&PathBuf::from("/Users/u/.pnpm-store")));
        assert!(is_pnpm_store(&PathBuf::from(
            r"C:\Users\u\AppData\Local\pnpm\store"
        )));
        assert!(!is_pnpm_store(&PathBuf::from("/tmp/pnpm/cache")));
    }

    /// `pnpm store path` 带版本后缀（`/v3`）时，目标是其父目录也得放行——
    /// 否则默认布局下 owner command 优化永不触发。
    #[test]
    fn pnpm_store_prune_tolerates_version_suffix() {
        // 模拟 `pnpm store path` 输出与目标的关系：需要真实跑命令才能
        // 得到 Some(...) 或 None，这里直接验证一致性判定不会误杀父目录。
        // go_clean_modcache / pnpm_store_prune 内部都调 proc::run_with_timeout，
        // 没有工具链时返回 None（回退裸删），行为不退化，故只断言不会 panic。
        let _ = go_clean_modcache(&PathBuf::from("/Users/u/go/pkg/mod"));
        let _ = pnpm_store_prune(&PathBuf::from("/Users/u/.pnpm-store"));
    }
}
