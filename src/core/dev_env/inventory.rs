//! 「这个生态现在装了什么」的唯一入口。
//!
//! 发现（列出资产）与移除（授权删除）问的是同一个问题，所以只有这一份
//! 实现：同一个工具、同一条命令、同一套解析。两边各写一份解析的话，用户
//! 看到的清单与预检认定的清单就可能不是同一份——那种不一致恰好会表现成
//! 「界面上有这个包、点删除却说不存在」，或者更糟的反面。
//!
//! 每个函数只做一件事：跑工具自己的清单/路径命令，把输出解析成类型化结果。
//! 失败分两种，调用方必须区分（`core::proc` 的 `None` 语义）：
//!
//! - [`InventoryError::Unavailable`]：命令跑不起来、超时，或输出为空。
//! - [`InventoryError::Unreadable`]：有输出但解析不了。
//!
//! 两种都**不是**「清单是空的」。发现层据此降级为「只展示」，移除层据此拒绝。

use crate::core::proc::ProcRun;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 清单命令的超时。`conda info` 在索引大或首次运行时可能偏慢。
pub const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InventoryError {
    /// 命令跑不起来、超时，或没有任何输出。
    Unavailable,
    /// 命令有输出，但解析不了。
    Unreadable,
}

/// conda 报告的环境清单。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CondaInventory {
    /// 所有环境前缀（含 base）。`.condarc` 的 `envs_dirs` 会被 conda 自己
    /// 合并进来，所以装在别的盘上的环境也在其中——这正是按安装根猜测
    /// 永远补不齐的那部分。
    pub envs: Vec<PathBuf>,
    /// base 环境的前缀。由 conda 自己指名，不是靠猜安装根。
    pub root_prefix: Option<PathBuf>,
}

/// npm 全局前缀下报告的一个包。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NpmPackage {
    pub name: String,
    pub version: Option<String>,
    /// `<prefix>/node_modules/<name>`。
    pub path: PathBuf,
}

/// pipx 报告的一个工具。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipxTool {
    pub name: String,
    /// pipx 自己报告的 venv 路径；版本较老没有这个字段时为 `None`。
    pub venv: Option<PathBuf>,
}

/// `conda info --json`：环境清单 + base 前缀。
pub fn conda(
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
) -> Result<CondaInventory, InventoryError> {
    let json = json_of(run, "conda", &["info", "--json"])?;
    let envs = json
        .get("envs")
        .and_then(|value| value.as_array())
        .ok_or(InventoryError::Unreadable)?
        .iter()
        .filter_map(|value| value.as_str())
        .map(PathBuf::from)
        .collect();
    Ok(CondaInventory {
        envs,
        root_prefix: json
            .get("root_prefix")
            .and_then(|value| value.as_str())
            .map(PathBuf::from),
    })
}

/// `mamba info --json`：与 conda 同一套输出，给只装了 mamba 的机器兜底。
pub fn mamba(
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
) -> Result<CondaInventory, InventoryError> {
    let json = json_of(run, "mamba", &["info", "--json"])?;
    let envs = json
        .get("envs")
        .and_then(|value| value.as_array())
        .ok_or(InventoryError::Unreadable)?
        .iter()
        .filter_map(|value| value.as_str())
        .map(PathBuf::from)
        .collect();
    Ok(CondaInventory {
        envs,
        root_prefix: json
            .get("root_prefix")
            .and_then(|value| value.as_str())
            .map(PathBuf::from),
    })
}

/// `npm prefix --global`：全局安装前缀。
pub fn npm_global_prefix(
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
) -> Result<PathBuf, InventoryError> {
    let text = text_of(run, "npm", &["prefix", "--global"])?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(InventoryError::Unavailable);
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(InventoryError::Unreadable);
    }
    Ok(path)
}

/// `npm ls --global --depth=0 --json`：全局**顶级**依赖。
///
/// 退出码有意不看：`npm ls` 在存在 extraneous / missing 包时返回非 0，
/// 但 stdout 仍是完整可用的 JSON。反过来，`ProcRun::ok` 也只表示退出码为 0，
/// 不代表结果可信——所以这里以「能否解析出 dependencies 对象」为准。
///
/// 顶级这一层是安全属性而不只是显示选择：是别的全局包依赖的包不在这个
/// 结果里，因而永远拿不到删除授权。
pub fn npm_top_level(
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
) -> Result<Vec<NpmPackage>, InventoryError> {
    let prefix = npm_global_prefix(run)?;
    let result = run("npm", &["ls", "--global", "--depth=0", "--json"], TIMEOUT)
        .ok_or(InventoryError::Unavailable)?;
    let text = String::from_utf8_lossy(&result.stdout).to_string();
    if text.trim().is_empty() {
        return Err(InventoryError::Unavailable);
    }
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| InventoryError::Unreadable)?;
    let dependencies = json
        .get("dependencies")
        .and_then(|value| value.as_object())
        .ok_or(InventoryError::Unreadable)?;
    Ok(dependencies
        .iter()
        .map(|(name, value)| NpmPackage {
            name: name.clone(),
            version: value
                .get("version")
                .and_then(|version| version.as_str())
                .map(str::to_string),
            path: prefix.join("node_modules").join(name),
        })
        .collect())
}

/// `pnpm root --global`：pnpm 全局包的根目录。
///
/// 不硬编码 `pnpm/global/<n>/node_modules`：中间那一段是 store 版本号，
/// pnpm 主版本一变就失效，而 `pnpm root` 是它自己报的。
pub fn pnpm_global_root(
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
) -> Result<PathBuf, InventoryError> {
    let text = text_of(run, "pnpm", &["root", "--global"])?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(InventoryError::Unavailable);
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(InventoryError::Unreadable);
    }
    Ok(path)
}

/// `uv tool dir`：uv 工具的根目录，其直接子目录即各个工具（uv 的既定布局）。
pub fn uv_tool_root(
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
) -> Result<PathBuf, InventoryError> {
    let text = text_of(run, "uv", &["tool", "dir"])?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(InventoryError::Unavailable);
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(InventoryError::Unreadable);
    }
    Ok(path)
}

/// `pipx list --json`：pipx 安装的各个工具。
pub fn pipx_venvs(
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
) -> Result<Vec<PipxTool>, InventoryError> {
    let json = json_of(run, "pipx", &["list", "--json"])?;
    let venvs = json
        .get("venvs")
        .and_then(|value| value.as_object())
        .ok_or(InventoryError::Unreadable)?;
    Ok(venvs
        .iter()
        .map(|(name, value)| PipxTool {
            name: name.clone(),
            venv: value
                .get("metadata")
                .and_then(|metadata| metadata.get("environment"))
                .and_then(|value| value.as_str())
                .map(PathBuf::from),
        })
        .collect())
}

/// 跑一条命令并取出 stdout 文本。
fn text_of(
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
    program: &str,
    args: &[&str],
) -> Result<String, InventoryError> {
    let result = run(program, args, TIMEOUT).ok_or(InventoryError::Unavailable)?;
    if !result.ok {
        return Err(InventoryError::Unavailable);
    }
    let text = String::from_utf8_lossy(&result.stdout).to_string();
    if text.trim().is_empty() {
        return Err(InventoryError::Unavailable);
    }
    Ok(text)
}

/// 跑一条命令并解析成 JSON。
fn json_of(
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
    program: &str,
    args: &[&str],
) -> Result<serde_json::Value, InventoryError> {
    let text = text_of(run, program, args)?;
    serde_json::from_str(&text).map_err(|_| InventoryError::Unreadable)
}

/// 一个全局根自己写的 `package.json` 里的顶层依赖名。
///
/// 这是「用户到底装了什么」的权威答案，和 `npm ls --global --depth=0` 取的是
/// 同一层信息：bun 写 `<global>/package.json`、pnpm 写 `<root 的父级>/package.json`，
/// 都在 `node_modules` 旁边。
///
/// 没有它就只能列 `node_modules` 的直接子目录，而那里绝大多数是别人的**传递
/// 依赖**——本机实测：bun 声明 3 个全局包，目录里有 132 个。把 129 个传递依赖
/// 当作用户装的包展示，既没法看也没法用。
///
/// 只在 `dependencies` 确实是对象时返回 `Some`；文件缺失、解析不了、没有这个
/// 键都返回 `None`（= 问不出，退回按目录列），因为「清单文件在但没有
/// dependencies 键」并不等于「一个都没装」，不该拿它去隐藏真实存在的目录。
pub fn declared_top_level(node_modules: &Path) -> Option<Vec<String>> {
    let manifest = node_modules.parent()?.join("package.json");
    let content = std::fs::read_to_string(manifest).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    let dependencies = json.get("dependencies")?.as_object()?;
    Some(dependencies.keys().cloned().collect())
}

/// `<root>/<name>` 的规范化拼接，用于「工具报告的位置」与「扫描期记录的位置」
/// 是否指向同一个东西。
pub fn join_norm(root: &Path, name: &str) -> String {
    crate::core::safety::norm(&root.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn proc_run(stdout: &str, ok: bool) -> ProcRun {
        ProcRun {
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
            exit_code: Some(if ok { 0 } else { 1 }),
            ok,
        }
    }

    fn fake(
        answers: &[(&str, Option<ProcRun>)],
    ) -> impl FnMut(&str, &[&str], Duration) -> Option<ProcRun> {
        let mut table: HashMap<String, Option<ProcRun>> = answers
            .iter()
            .map(|(key, value)| {
                (
                    key.to_string(),
                    value.as_ref().map(|result| ProcRun {
                        stdout: result.stdout.clone(),
                        stderr: result.stderr.clone(),
                        exit_code: result.exit_code,
                        ok: result.ok,
                    }),
                )
            })
            .collect();
        move |program, args, _timeout| {
            table
                .remove(&format!("{program} {}", args.join(" ")))
                .unwrap_or(None)
        }
    }

    #[test]
    fn conda_inventory_reads_envs_and_base() {
        let mut runner = fake(&[(
            "conda info --json",
            Some(proc_run(
                r#"{"envs":["/h/miniconda3","/d/other/envs/x"],"root_prefix":"/h/miniconda3"}"#,
                true,
            )),
        )]);
        let inventory = conda(&mut runner).unwrap();
        assert_eq!(
            inventory.envs,
            vec![
                PathBuf::from("/h/miniconda3"),
                PathBuf::from("/d/other/envs/x")
            ],
            "envs 必须原样来自 conda，包含 .condarc 指到别的盘的那些"
        );
        assert_eq!(inventory.root_prefix, Some(PathBuf::from("/h/miniconda3")));
    }

    #[test]
    fn a_missing_tool_is_unavailable_not_an_empty_inventory() {
        let mut runner = fake(&[]);
        assert_eq!(conda(&mut runner).unwrap_err(), InventoryError::Unavailable);
    }

    #[test]
    fn blank_output_is_unavailable_not_unreadable() {
        let mut runner = fake(&[("conda info --json", Some(proc_run("   \n", true)))]);
        assert_eq!(conda(&mut runner).unwrap_err(), InventoryError::Unavailable);
    }

    #[test]
    fn garbage_output_is_unreadable() {
        let mut runner = fake(&[("conda info --json", Some(proc_run("not json", true)))]);
        assert_eq!(conda(&mut runner).unwrap_err(), InventoryError::Unreadable);
    }

    #[test]
    fn non_zero_exit_is_unavailable_for_path_commands() {
        let mut runner = fake(&[(
            "npm prefix --global",
            Some(proc_run("/h/.npm-global", false)),
        )]);
        assert_eq!(
            npm_global_prefix(&mut runner).unwrap_err(),
            InventoryError::Unavailable
        );
    }

    #[test]
    fn a_relative_path_from_a_tool_is_not_accepted() {
        let mut runner = fake(&[("uv tool dir", Some(proc_run("tools\n", true)))]);
        assert_eq!(
            uv_tool_root(&mut runner).unwrap_err(),
            InventoryError::Unreadable,
            "相对路径无法与扫描期记录的绝对路径比对，不能当有效答案"
        );
    }

    /// 一个在两个平台上都成立的绝对根。
    ///
    /// CI 同时跑 Windows 与 macOS，硬编码 `/h/...` 在 Windows 上不是绝对路径，
    /// 会让「工具报的路径必须是绝对的」这道校验把测试自己挡在门外。
    fn abs_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    #[test]
    fn npm_top_level_parses_packages_under_the_reported_prefix() {
        let prefix = abs_root("qc-npm-global");
        let mut runner = fake(&[
            (
                "npm prefix --global",
                Some(proc_run(&format!("{}\n", prefix.display()), true)),
            ),
            (
                "npm ls --global --depth=0 --json",
                Some(proc_run(
                    r#"{"dependencies":{"typescript":{"version":"5.4.2"}}}"#,
                    true,
                )),
            ),
        ]);
        let packages = npm_top_level(&mut runner).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].name, "typescript");
        assert_eq!(packages[0].version.as_deref(), Some("5.4.2"));
        assert_eq!(
            packages[0].path,
            prefix.join("node_modules").join("typescript")
        );
    }

    #[test]
    fn npm_top_level_still_parses_when_npm_exits_non_zero() {
        // `npm ls` 在存在 extraneous / missing 包时返回 1，stdout 依然可用。
        let prefix = abs_root("qc-npm-global");
        let mut runner = fake(&[
            (
                "npm prefix --global",
                Some(proc_run(&format!("{}\n", prefix.display()), true)),
            ),
            (
                "npm ls --global --depth=0 --json",
                Some(proc_run(
                    r#"{"dependencies":{"typescript":{"version":"5.4.2"}}}"#,
                    false,
                )),
            ),
        ]);
        let packages = npm_top_level(&mut runner).expect("非 0 退出码不等于结果不可用");
        assert_eq!(packages[0].name, "typescript");
    }

    #[test]
    fn pipx_venv_path_is_optional() {
        let mut runner = fake(&[(
            "pipx list --json",
            Some(proc_run(
                r#"{"venvs":{"black":{},"ruff":{"metadata":{"environment":"/h/.local/pipx/venvs/ruff"}}}}"#,
                true,
            )),
        )]);
        let tools = pipx_venvs(&mut runner).unwrap();
        assert_eq!(tools.len(), 2);
        let black = tools.iter().find(|tool| tool.name == "black").unwrap();
        assert_eq!(black.venv, None, "老版本 pipx 没有 environment 字段");
        let ruff = tools.iter().find(|tool| tool.name == "ruff").unwrap();
        assert_eq!(ruff.venv, Some(PathBuf::from("/h/.local/pipx/venvs/ruff")));
    }

    #[test]
    fn join_norm_matches_the_scanned_path_normalization() {
        // 工具报的位置（根 + 名字）与扫描期记录的完整路径必须能用同一套
        // 规范化比对，否则「路径一致」这条预检会永远不成立（或永远成立）。
        assert_eq!(
            join_norm(Path::new("C:/npm-global"), "typescript"),
            crate::core::safety::norm(Path::new("c:\\npm-global\\typescript"))
        );
    }
}
