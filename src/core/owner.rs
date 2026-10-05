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
//! 大小），由统一能力执行器分开执行作用域预检、固定操作与完成核验。
//! 未尝试命令且预检不可用时，按原授权范围清空内容并保留根目录；
//! 步骤报告记录实际路线，尝试命令后失败禁止回退。

use std::path::Path;
use std::time::Duration;

/// owner command 的超时。大缓存的清理可以到几十秒；超时按失败处理，
/// 由 cleaner 报 Failed（不回退裸删——命令跑到一半被杀已经动过 store，
/// 再裸删等于在未知状态上继续动刀）。
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Debug)]
pub(crate) struct PreparedOwner {
    operation: crate::core::rules::Operation,
    target: std::path::PathBuf,
}
impl PreparedOwner {
    pub(crate) fn prepare(
        target: &Path,
        operation: &crate::core::rules::Operation,
    ) -> Option<Self> {
        Self::prepare_with(target, operation, |tool, args, timeout| {
            crate::core::proc::run_with_timeout(tool, args, timeout)
        })
    }
    fn prepare_with(
        target: &Path,
        operation: &crate::core::rules::Operation,
        mut run: impl FnMut(&str, &[&str], Duration) -> Option<crate::core::proc::ProcRun>,
    ) -> Option<Self> {
        use crate::core::rules::Operation;
        let (tool, args): (&str, &[&str]) = match operation {
            Operation::Go => ("go", &["env", "GOMODCACHE"]),
            Operation::Pnpm => ("pnpm", &["store", "path"]),
            _ => return None,
        };
        let result = run(tool, args, Duration::from_secs(5))?;
        if !result.ok {
            return None;
        }
        let path = std::str::from_utf8(&result.stdout).ok()?.trim();
        let owner = crate::core::safety::norm(Path::new(path));
        let selected = crate::core::safety::norm(target);
        if path.is_empty()
            || !(owner == selected
                || (*operation == Operation::Pnpm && owner.starts_with(&format!("{selected}\\"))))
        {
            return None;
        }
        Some(Self {
            operation: operation.clone(),
            target: target.into(),
        })
    }
    pub(crate) fn apply(&self) -> Result<(), String> {
        self.apply_with(|tool, args, timeout| {
            crate::core::proc::run_with_timeout(tool, args, timeout)
        })
    }
    fn apply_with(
        &self,
        mut run: impl FnMut(&str, &[&str], Duration) -> Option<crate::core::proc::ProcRun>,
    ) -> Result<(), String> {
        let (tool, args): (&str, &[&str]) = match self.operation {
            crate::core::rules::Operation::Go => ("go", &["clean", "-modcache"]),
            crate::core::rules::Operation::Pnpm => ("pnpm", &["store", "prune"]),
            _ => return Err("Unsupported owner capability".into()),
        };
        let result = run(tool, args, COMMAND_TIMEOUT)
            .ok_or("Owner cleanup unavailable or timed out after preparation")?;
        if result.ok {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&result.stderr).trim().into())
        }
    }
    pub(crate) fn completion(&self) -> crate::core::rules::facts::Evidence {
        self.completion_with(|tool, args, timeout| {
            crate::core::proc::run_with_timeout(tool, args, timeout)
        })
    }
    fn completion_with(
        &self,
        mut run: impl FnMut(&str, &[&str], Duration) -> Option<crate::core::proc::ProcRun>,
    ) -> crate::core::rules::facts::Evidence {
        use crate::core::rules::facts::Evidence;
        match self.operation {
            crate::core::rules::Operation::Go => crate::core::rules::flow::filesystem_completion(
                &crate::core::rules::CompletionCondition::PathAbsent {
                    path: self.target.clone(),
                },
            ),
            crate::core::rules::Operation::Pnpm => {
                match run("pnpm", &["store", "status"], COMMAND_TIMEOUT) {
                    Some(result) if result.ok => Evidence::Confirmed,
                    Some(_) => Evidence::Absent,
                    None => Evidence::Unknown,
                }
            }
            _ => Evidence::Unknown,
        }
    }
}

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
    go_clean_modcache_with(target, |args, timeout| {
        crate::core::proc::run_with_timeout("go", args, timeout)
    })
}
fn go_clean_modcache_with(
    target: &Path,
    mut run: impl FnMut(&[&str], Duration) -> Option<crate::core::proc::ProcRun>,
) -> Option<bool> {
    let owner = PreparedOwner::prepare_with(
        target,
        &crate::core::rules::Operation::Go,
        |_, args, timeout| run(args, timeout),
    )?;
    Some(
        owner
            .apply_with(|_, args, timeout| run(args, timeout))
            .is_ok()
            && owner.completion_with(|_, args, timeout| run(args, timeout))
                == crate::core::rules::facts::Evidence::Confirmed,
    )
}

/// 用 `pnpm store prune` 收缩 store。
///
/// 前提：`pnpm` 在 PATH 且 `pnpm store path` 报告的 store 与目标一致。
/// 老版本 pnpm 没有 `store prune`（6.0 引入）——命令跑失败如实返回
/// `Some(false)`，由 cleaner 报 Failed，不静默回退裸删（理由见
/// [`COMMAND_TIMEOUT`] 的注释）。
pub fn pnpm_store_prune(target: &Path) -> Option<bool> {
    pnpm_store_prune_with(target, |args, timeout| {
        crate::core::proc::run_with_timeout("pnpm", args, timeout)
    })
}
fn pnpm_store_prune_with(
    target: &Path,
    mut run: impl FnMut(&[&str], Duration) -> Option<crate::core::proc::ProcRun>,
) -> Option<bool> {
    let owner = PreparedOwner::prepare_with(
        target,
        &crate::core::rules::Operation::Pnpm,
        |_, args, timeout| run(args, timeout),
    )?;
    Some(
        owner
            .apply_with(|_, args, timeout| run(args, timeout))
            .is_ok()
            && owner.completion_with(|_, args, timeout| run(args, timeout))
                == crate::core::rules::facts::Evidence::Confirmed,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    fn response(path: &Path) -> crate::core::proc::ProcRun {
        crate::core::proc::ProcRun {
            stdout: path.to_string_lossy().as_bytes().to_vec(),
            stderr: vec![],
            exit_code: Some(0),
            ok: true,
        }
    }
    #[test]
    fn attempted_owner_timeout_never_grants_filesystem_fallback() {
        let root = crate::core::testing::fixture("owner_timeout");
        std::fs::write(root.join("keep"), b"unknown store state").unwrap();
        assert_eq!(
            go_clean_modcache_with(&root, |args, _| if args == ["env", "GOMODCACHE"] {
                Some(response(&root))
            } else {
                None
            }),
            Some(false)
        );
        assert_eq!(
            pnpm_store_prune_with(&root, |args, _| if args == ["store", "path"] {
                Some(response(&root))
            } else {
                None
            }),
            Some(false)
        );
        assert!(root.join("keep").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn owner_completion_checks_resource_state_after_zero_exit() {
        let root = crate::core::testing::fixture("owner_completion");
        std::fs::write(root.join("keep"), b"not removed").unwrap();
        assert_eq!(
            go_clean_modcache_with(&root, |_, _| Some(response(&root))),
            Some(false)
        );
        assert_eq!(
            pnpm_store_prune_with(&root, |args, _| if args == ["store", "status"] {
                None
            } else {
                Some(response(&root))
            }),
            Some(false)
        );
        assert_eq!(
            pnpm_store_prune_with(&root, |_, _| Some(response(&root))),
            Some(true)
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn prepared_owner_rejects_sibling_scope_and_invalid_utf8() {
        let target = PathBuf::from("/Users/u/.pnpm-store");
        let sibling = PathBuf::from("/Users/u/.pnpm-store-evil/v3");
        assert!(PreparedOwner::prepare_with(
            &target,
            &crate::core::rules::Operation::Pnpm,
            |tool, args, timeout| {
                assert_eq!(tool, "pnpm");
                assert_eq!(args, ["store", "path"]);
                assert_eq!(timeout, Duration::from_secs(5));
                Some(response(&sibling))
            }
        )
        .is_none());
        assert!(PreparedOwner::prepare_with(
            &target,
            &crate::core::rules::Operation::Pnpm,
            |_, _, _| {
                let mut output = response(&target);
                output.stdout = vec![0xff];
                Some(output)
            }
        )
        .is_none());
    }

    #[test]
    fn prepared_owner_uses_fixed_commands_and_retains_mutation_errors() {
        for (operation, tool, prepare, apply) in [
            (
                crate::core::rules::Operation::Go,
                "go",
                vec!["env", "GOMODCACHE"],
                vec!["clean", "-modcache"],
            ),
            (
                crate::core::rules::Operation::Pnpm,
                "pnpm",
                vec!["store", "path"],
                vec!["store", "prune"],
            ),
        ] {
            let target = PathBuf::from("/fixture/cache");
            let owner = PreparedOwner::prepare_with(&target, &operation, |actual_tool, args, _| {
                assert_eq!(actual_tool, tool);
                assert_eq!(args, prepare);
                Some(response(&target))
            })
            .unwrap();
            let error = owner
                .apply_with(|actual_tool, args, timeout| {
                    assert_eq!(actual_tool, tool);
                    assert_eq!(args, apply);
                    assert_eq!(timeout, COMMAND_TIMEOUT);
                    let mut result = response(&target);
                    result.ok = false;
                    result.stderr = b"permission denied".to_vec();
                    Some(result)
                })
                .unwrap_err();
            assert_eq!(error, "permission denied");
            assert!(owner
                .apply_with(|_, _, _| None)
                .unwrap_err()
                .contains("timed out"));
        }
    }

    #[test]
    fn go_modcache_recognized_by_suffix() {
        assert!(is_go_modcache(&PathBuf::from("/home/u/go/pkg/mod")));
        assert!(is_go_modcache(&PathBuf::from(r"C:\Users\u\go\pkg\mod")));
        assert!(!is_go_modcache(&PathBuf::from("/home/u/go/pkg")));
        assert!(!is_go_modcache(&PathBuf::from("/tmp/other")));
    }

    #[test]
    fn pnpm_store_recognized_by_suffix() {
        assert!(is_pnpm_store(&PathBuf::from("/Users/u/Library/pnpm/store")));
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
        let target = PathBuf::from("/Users/u/.pnpm-store");
        let reported = target.join("v3");
        assert_eq!(
            pnpm_store_prune_with(&target, |args, _| {
                Some(response(if args == ["store", "path"] {
                    &reported
                } else {
                    &target
                }))
            }),
            Some(true)
        );
    }
}
