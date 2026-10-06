//! 开发环境与全局包的移除：只走生态自己的命令，或删除生态自证的目录。
//!
//! # 为什么不是删目录
//!
//! conda 环境、pipx / uv 的工具 venv、npm 全局包，都是**工具自己拥有内部
//! 登记**的目录——conda 的 `conda-meta/`、pipx 的 `pipx_metadata.json`、
//! 包管理器自己的安装记录。裸删目录会留下「登记说装了、实际没了」的半成品；
//! 同一类不一致在 `core::owner`（go module cache、pnpm store）上已经付过一次
//! 代价，那里定下的做法是让生态自己的命令去收缩它。
//!
//! # 授权从哪来
//!
//! 删掉一个包或环境不是「释放可再生的缓存」，是「让它从此消失」，所以这里
//! 不认扫描期的结果，只认执行前当场重新验证的三件事：
//!
//! 1. **目标还在生态自己的清单里**：conda 重跑 `conda info --json` 看 `envs`，
//!    npm 重跑 `npm ls -g --depth=0 --json` 看顶级依赖，pipx 重跑
//!    `pipx list --json`，pnpm / uv 则重跑 `pnpm root -g` / `uv tool dir` 拿根；
//!    pip 包重验 `*.dist-info` 目录（PEP 376）并从 `METADATA` 重读名字，
//!    venv 重验 `pyvenv.cfg`（PEP 405）——后两者是生态自己写的登记文件，
//!    重验它们与重跑命令是同一件事。
//! 2. **解析出的路径与扫描期一致**：不一致说明中途换过（node 版本管理器切了
//!    全局前缀、环境被删掉重建），此时拒绝而不是照旧执行。
//! 3. **卸载的完整 argv 在预检时就定死**，执行期不再从任何可变状态推导——
//!    与安装产物「授权不在执行期扩张」同一条纪律。
//!
//! 命令不可用、输出解析不了、目标不在清单里，三种都**拒绝**，不降级成裸删。
//!
//! # venv 的整目录删除是唯一例外
//!
//! venv 不在任何包管理器的登记里：`pyvenv.cfg` 是它全部的「已安装」状态，
//! 删掉目录没有第二处状态要同步。所以 `Channel::VenvDirectory` 不跑命令，
//! 直接整树删除，但删除前重验 cfg 与扫描期稳定身份，拒绝根和祖先重定向。
//! 所有节点复用 `cleaner::delete_tree` 的安全保护与删除重试。这与 P20 不冲突——那条禁的是
//! 「跑过生态命令后回退裸删」，这里从头就没有命令可跑，删除本身就是操作。
//!
//! # 失败语义
//!
//! 与 P20 一致：**已尝试生态命令后一律 `Failed`，禁止回退裸删**——命令可能
//! 已经动了一半，在未知状态上继续动刀比停下更糟。「命令根本没跑成」（拒绝，
//! 目标未被触碰）与「跑了但没通过核验」（失败，状态未知）是两件事，分开报。

use super::inventory::{self, InventoryError};
use super::{AssetSource, DevAssetItem, DevAssetKind, PythonToolKind};
use crate::core::proc::ProcRun;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 卸载命令的超时。conda 删一个几 GB 的环境可以到分钟级。
const REMOVE_TIMEOUT: Duration = Duration::from_secs(300);

/// pip 探活的超时（`python -m pip --version`）。解释器坏了时它可能挂着，
/// 给它一个比扫描探测略宽、比卸载本身短得多的上限。
const PIP_PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// 有受支持卸载通道的生态。
///
/// bun **故意不在其中**：它的全局清单输出格式没有在本机核实过，而
/// `bun remove -g` 一旦跑错就是删错包。在核实之前 bun 只展示、不提供移除。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Conda,
    Npm,
    Pnpm,
    Pipx,
    Uv,
}

impl Tool {
    fn executable(self) -> &'static str {
        match self {
            Self::Conda => "conda",
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Pipx => "pipx",
            Self::Uv => "uv",
        }
    }
}

/// 一条经过核实的移除通道。
///
/// [`Tool`] 通道跑生态命令；[`Channel::Pip`] 也是命令，只是 program 是
/// 包条目自带的那个解释器（`python -m pip uninstall`），不是 PATH 上的
/// 工具名；[`Channel::VenvDirectory`] 是唯一的非命令通道，见模块头。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Channel {
    Tool(Tool),
    Pip,
    VenvDirectory,
}

/// 拒绝原因：命令没有跑成，目标未被触碰。每一条都要能对用户讲清楚。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemovalRefusal {
    /// 路径落在 `core::safety` 的保护范围内。
    Protected,
    /// conda 的 base 环境，或此刻正处于激活状态的环境。
    ProtectedEnvironment,
    /// 此刻被占用，或者占用状态测不出（后者同样拒绝，见 `core::inuse`）。
    Busy,
    /// 该生态没有受支持的卸载通道（目前只有 bun）。
    UnsupportedTool,
    /// 条目来自按布局的降级扫描，没有被生态确认过。
    UnverifiedSource,
    /// 生态命令跑不起来或超时——「测不出」，不是「没有」。
    ToolUnavailable,
    /// 命令有输出但解析不了。同样不拿它当「清单里没有」。
    ToolUnreadable,
    /// 目标已不在生态的清单里，或解析出的路径与扫描期不一致。
    NotRegistered,
}

/// 已经跑过生态命令但没通过核验。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovalFailure {
    /// 稳定失败码，进审计记录、不随文案漂移。
    pub code: &'static str,
    /// 给人看的细节：命令的 stderr 摘要，或核验结论。
    pub detail: String,
}

/// 单个资产的移除结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemovalOutcome {
    /// 生态命令已执行，且目标路径确认消失。
    Removed,
    /// 未执行任何命令：闸门拒绝或预检不通过。目标未被触碰。
    Refused(RemovalRefusal),
    /// 已尝试生态命令但未通过核验。**不再回退裸删**（P20）。
    Failed(RemovalFailure),
}

/// 条目对应的卸载通道；`None` = 这个生态没有经过核实的卸载通道。
///
/// 「哪个生态能删」只有这一处名单：界面用的 [`can_remove`] 与执行用的
/// `PreparedRemoval::prepare_with` 都从这里取。分两处写迟早会漂移成
/// 「界面有按钮、执行说没有通道」。
fn channel_for(item: &DevAssetItem) -> Option<Channel> {
    match &item.kind {
        DevAssetKind::CondaEnv { .. } => Some(Channel::Tool(Tool::Conda)),
        DevAssetKind::NodeGlobalPackage { manager, .. } => match manager.as_str() {
            "npm" => Some(Channel::Tool(Tool::Npm)),
            "pnpm" => Some(Channel::Tool(Tool::Pnpm)),
            // bun 只到 Layout 或「有清单、没有核实过的卸载语义」，见 `Tool`。
            _ => None,
        },
        DevAssetKind::PythonTool { tool_kind, .. } => match tool_kind {
            PythonToolKind::Pipx => Some(Channel::Tool(Tool::Pipx)),
            PythonToolKind::Uv => Some(Channel::Tool(Tool::Uv)),
        },
        // pip 包：dist-info 是 pip 自己写的登记（PEP 376），卸载命令是拥有
        // 该 site 的解释器自己的 `python -m pip uninstall`。安装级包目录里的
        // 条目在枚举阶段就不产出（P41：那是解释器的一部分，且商店版在文件
        // 系统层就不可写），所以这里不需要再按 scope 分叉。
        DevAssetKind::PipPackage { .. } => Some(Channel::Pip),
        // venv 整体：pyvenv.cfg（PEP 405）自证，目录即全部状态，见模块头。
        DevAssetKind::VirtualEnv { .. } => Some(Channel::VenvDirectory),
        // 解释器的包目录行本身没有移除入口：可移除的是它里面的 pip 包条目。
        DevAssetKind::PythonInterpreter { .. } => None,
    }
}

/// 界面据此决定要不要给这一条显示移除入口。
///
/// 这只是提前告知；[`remove_asset`] 里的每一道闸门都会独立再验一遍，
/// 不依赖任何界面先做过检查。
pub fn can_remove(item: &DevAssetItem) -> bool {
    match channel_for(item) {
        None => false,
        // venv 的凭据是 pyvenv.cfg 本身（预检会重验），不看扫描来源——
        // 按目录扫出来的与 poetry 报上来的可删性没有区别。
        Some(Channel::VenvDirectory) => {
            matches!(
                &item.kind,
                DevAssetKind::VirtualEnv {
                    identity: Some(_),
                    ..
                }
            ) && !item.is_protected()
        }
        Some(_) => item.source == AssetSource::Tool && !item.is_protected(),
    }
}

/// 预检通过后冻结下来的卸载方案。
#[derive(Clone, Debug)]
pub(crate) struct PreparedRemoval {
    /// 扫描期解析出的目标路径，核验时要求它消失。
    target: PathBuf,
    /// 在预检时定死的卸载程序。生态命令是 PATH 上的工具名；pip 通道是
    /// 预检时核验过的那个解释器绝对路径。
    program: String,
    args: Vec<String>,
    kind: PreparedKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreparedKind {
    Command,
    /// 整树删除（venv）。没有 argv 可冻结——目录就是全部状态。
    DirectoryTree,
}

impl PreparedRemoval {
    fn prepare_with(
        item: &DevAssetItem,
        channel: Channel,
        mut run: impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
    ) -> Result<Self, RemovalRefusal> {
        match channel {
            Channel::Tool(tool) => {
                let id = match &item.kind {
                    // conda 的生态标识就是环境路径（`remove --prefix <path>`）。
                    DevAssetKind::CondaEnv { .. } => norm(&item.path),
                    DevAssetKind::NodeGlobalPackage { name, .. } => name.clone(),
                    DevAssetKind::PythonTool { name, .. } => name.clone(),
                    // 上面 `channel_for` 已经把只读来源挡掉了。
                    DevAssetKind::PythonInterpreter { .. }
                    | DevAssetKind::VirtualEnv { .. }
                    | DevAssetKind::PipPackage { .. } => {
                        return Err(RemovalRefusal::UnsupportedTool)
                    }
                };

                let (program, args) = match tool {
                    Tool::Conda => preflight_conda(&id, &item.path, &mut run)?,
                    _ => {
                        let args = match tool {
                            Tool::Npm => preflight_npm(&id, &item.path, &mut run)?,
                            Tool::Pnpm => preflight_pnpm(&id, &item.path, &mut run)?,
                            Tool::Pipx => preflight_pipx(&id, &item.path, &mut run)?,
                            Tool::Uv => preflight_uv(&id, &item.path, &mut run)?,
                            Tool::Conda => unreachable!(),
                        };
                        (tool.executable().to_string(), args)
                    }
                };

                Ok(Self {
                    target: item.path.clone(),
                    program,
                    args,
                    kind: PreparedKind::Command,
                })
            }
            Channel::Pip => {
                let (program, args) = preflight_pip(item, &mut run)?;
                Ok(Self {
                    target: item.path.clone(),
                    program,
                    args,
                    kind: PreparedKind::Command,
                })
            }
            Channel::VenvDirectory => {
                preflight_venv(item)?;
                Ok(Self {
                    target: item.path.clone(),
                    program: String::new(),
                    args: Vec::new(),
                    kind: PreparedKind::DirectoryTree,
                })
            }
        }
    }

    /// 跑卸载命令。`None` 表示命令没跑起来或超时；退出码非 0 也算没成功，
    /// 两种都由调用方转成 `Failed` 而**不是**回退裸删。
    fn run(
        &self,
        run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
    ) -> Option<ProcRun> {
        let args: Vec<&str> = self.args.iter().map(String::as_str).collect();
        run(&self.program, &args, REMOVE_TIMEOUT)
    }
}

/// 移除一个开发资产：闸门 → 预检 → 执行 → 核验。
///
/// 顺序是有意的：先跑三个便宜的本地闸门，再花时间跑生态命令。
pub fn remove_asset(item: &DevAssetItem) -> RemovalOutcome {
    let mut run = crate::core::proc::run_tool_with_timeout;
    remove_asset_with(item, &mut run)
}

pub(crate) fn remove_asset_with(
    item: &DevAssetItem,
    mut run: impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
) -> RemovalOutcome {
    remove_asset_with_tree(item, &mut run, remove_tree)
}

/// 树删除也走注入，与命令执行器同一待遇：测试要能锁「删除报了成功但目录
/// 还在」与「删除本身失败」这两种失败，而让真实文件系统制造这两种状态
/// 既不可移植也不可靠。
pub(crate) fn remove_asset_with_tree(
    item: &DevAssetItem,
    mut run: impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
    mut remove_tree_fn: impl FnMut(&Path) -> bool,
) -> RemovalOutcome {
    // 1. 路径保护只问 core::safety（AGENTS.md：不在别处抄第二份黑名单）。
    if crate::core::safety::is_protected(&item.path) {
        return RemovalOutcome::Refused(RemovalRefusal::Protected);
    }
    // 2. base 环境与当前激活环境：生态语义上的不可删，不是路径黑名单。
    if item.is_protected() {
        return RemovalOutcome::Refused(RemovalRefusal::ProtectedEnvironment);
    }
    // 3. 清单来源：走生态命令的通道只认生态命令报出来的条目，按布局扫出来
    //    的没有删除资格——权限判断不能只存在于视图里。venv 例外：它的凭据
    //    是 pyvenv.cfg 本身（预检会重验），按目录扫出来的与 poetry 报上来
    //    的可删性没有区别。
    let Some(channel) = channel_for(item) else {
        return RemovalOutcome::Refused(RemovalRefusal::UnsupportedTool);
    };
    if !matches!(channel, Channel::VenvDirectory) && item.source != AssetSource::Tool {
        return RemovalOutcome::Refused(RemovalRefusal::UnverifiedSource);
    }
    // 4. 占用复检。Windows 上这一步目前只能给出 Clear（P15），所以它是
    //    补充而不是保证——真正的保护来自生态命令自己拒绝删活动环境。
    if !matches!(item.in_use, crate::core::inuse::SpotCheck::Clear) {
        return RemovalOutcome::Refused(RemovalRefusal::Busy);
    }

    let prepared = match PreparedRemoval::prepare_with(item, channel, &mut run) {
        Ok(prepared) => prepared,
        Err(refusal) => return RemovalOutcome::Refused(refusal),
    };

    match prepared.kind {
        PreparedKind::Command => match prepared.run(&mut run) {
            // 命令跑不起来或超时：已经尝试过了（`run` 返回 None 也可能是超时被杀，
            // 命令可能动过一半），按 P20 记失败，不回退裸删。
            None => RemovalOutcome::Failed(RemovalFailure {
                code: "command-unavailable-or-timeout",
                detail: format!(
                    "{} {} did not complete",
                    prepared.program,
                    prepared.args.join(" ")
                ),
            }),
            Some(result) if !result.ok => RemovalOutcome::Failed(RemovalFailure {
                code: "command-failed",
                detail: command_detail(&result),
            }),
            Some(_) => {
                // 退出码 0 不算数（AGENTS.md：成功判据是产物/登记没了）。
                if confirmed_absent(&prepared.target) {
                    RemovalOutcome::Removed
                } else {
                    RemovalOutcome::Failed(RemovalFailure {
                        code: "reported-success-but-remains",
                        detail: format!("{} still exists", prepared.target.display()),
                    })
                }
            }
        },
        PreparedKind::DirectoryTree => {
            if let Err(refusal) = preflight_venv(item) {
                return RemovalOutcome::Refused(refusal);
            }
            // venv 没有「跑了半截的命令」：删除要么完成要么留下残迹，与
            // 命令失败同语义，报 Failed，没有任何兜底动作可做。删除报告了
            // 成功但目录还在（比如删除中途某项失败却被吞掉）也必须单独报——
            // 退出码 0 不算数，venv 的整树删除没有退出码，纪律相同。
            if !remove_tree_fn(&prepared.target) {
                RemovalOutcome::Failed(RemovalFailure {
                    code: "directory-removal-failed",
                    detail: format!("{} could not be fully deleted", prepared.target.display()),
                })
            } else if confirmed_absent(&prepared.target) {
                RemovalOutcome::Removed
            } else {
                RemovalOutcome::Failed(RemovalFailure {
                    code: "reported-success-but-remains",
                    detail: format!("{} still exists", prepared.target.display()),
                })
            }
        }
    }
}

/// 目标路径是否**确定**已经不存在。
///
/// 只有 `NotFound` 才算消失。权限错误、路径过长、I/O 失败一律算「测不出」——
/// 把读失败当成缺席就是拿未知当证据（与 P20「读失败不是缺席」同一条纪律）。
fn confirmed_absent(path: &Path) -> bool {
    absence_from_stat(
        std::fs::symlink_metadata(path)
            .err()
            .map(|error| error.kind()),
    )
}

/// `stat` 的结论 → 是否算缺席。`None` 表示 stat 成功（路径还在）。
///
/// 单独抽出来是为了让「哪种失败算消失」这条判断本身可测：制造权限拒绝、
/// 路径过长这类失败需要改 ACL 或拼超长路径，在测试里既不可移植也不可靠，
/// 而这恰恰是最不能写错的一行。
fn absence_from_stat(kind: Option<std::io::ErrorKind>) -> bool {
    matches!(kind, Some(std::io::ErrorKind::NotFound))
}

fn command_detail(result: &ProcRun) -> String {
    let stderr = String::from_utf8_lossy(&result.stderr);
    let text = stderr.trim();
    if text.is_empty() {
        String::from_utf8_lossy(&result.stdout).trim().to_string()
    } else {
        text.chars().take(300).collect()
    }
}

// ---- 各生态的预检：命令跑不通、解析不了、目标不在清单里，一律拒绝 ----
//
// 这里不自己解析任何清单——解析只有 `inventory` 一份，发现层问的是同一个
// 函数。两边各写一份的话，「界面上有这个包、点删除却说不存在」就是必然。

/// 清单错误 → 拒绝原因。两种失败都**不是**「清单里没有」。
fn refusal_from(error: InventoryError) -> RemovalRefusal {
    match error {
        InventoryError::Unavailable => RemovalRefusal::ToolUnavailable,
        InventoryError::Unreadable => RemovalRefusal::ToolUnreadable,
    }
}

/// 两个失败里更值得报告的那个。
///
/// `Unreadable` 说明工具在、只是输出读不懂，比 `Unavailable`（命令根本
/// 没有）更具体，也更接近用户需要修的东西。
fn worse(first: InventoryError, second: InventoryError) -> InventoryError {
    if first == InventoryError::Unreadable || second == InventoryError::Unreadable {
        InventoryError::Unreadable
    } else {
        InventoryError::Unavailable
    }
}

fn preflight_conda(
    id: &str,
    target: &Path,
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
) -> Result<(String, Vec<String>), RemovalRefusal> {
    // conda 与 mamba 是同一套 CLI 的两个前端；只装了 mamba 的机器不该整块消失。
    let (program, inventory) = match inventory::conda(run) {
        Ok(inventory) => ("conda", inventory),
        Err(conda_error) => match inventory::mamba(run) {
            Ok(inventory) => ("mamba", inventory),
            Err(mamba_error) => return Err(refusal_from(worse(conda_error, mamba_error))),
        },
    };

    if !inventory.envs.iter().any(|path| norm(path) == id) {
        return Err(RemovalRefusal::NotRegistered);
    }

    // base 环境由 conda 自己指名，不是靠猜安装根：`~/.conda` 这类配置目录
    // 混进候选根时，猜出来的 base 名单会既漏又错。
    if inventory
        .root_prefix
        .as_ref()
        .is_some_and(|root| norm(root) == id)
    {
        return Err(RemovalRefusal::ProtectedEnvironment);
    }

    // 用 `--prefix` 而不是 `-n <name>`：名字在不同 envs_dirs 间可能重名，
    // 路径才是扫描期确认过的那一个。
    Ok((
        program.into(),
        vec![
            "remove".into(),
            "--prefix".into(),
            target.to_string_lossy().to_string(),
            "--all".into(),
            "--yes".into(),
        ],
    ))
}

fn preflight_npm(
    name: &str,
    target: &Path,
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
) -> Result<Vec<String>, RemovalRefusal> {
    let packages = inventory::npm_top_level(run).map_err(refusal_from)?;
    // 顶级依赖的名字登记就是授权本身：是别的全局包依赖的包不在其中
    // （`--depth=0`），因而永远拿不到删除授权。
    let package = packages
        .iter()
        .find(|package| package.name == name)
        .ok_or(RemovalRefusal::NotRegistered)?;
    // 前缀可能中途换过（nvm / fnm 切了 node）。路径与扫描期不一致就拒绝，
    // 否则 `npm uninstall --global` 删的是当前前缀下那个同名的包，
    // 而不是我们列出来的那一个。
    if norm(&package.path) != norm(target) {
        return Err(RemovalRefusal::NotRegistered);
    }
    Ok(vec!["uninstall".into(), "--global".into(), name.into()])
}

fn preflight_pnpm(
    name: &str,
    target: &Path,
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
) -> Result<Vec<String>, RemovalRefusal> {
    let root = inventory::pnpm_global_root(run).map_err(refusal_from)?;
    // 根由 pnpm 自己报告，包名到目录的映射是它的既定布局。不硬编码
    // `global/<n>/node_modules`：中间那段是 store 版本号，pnpm 一变就失效。
    if inventory::join_norm(&root, name) != norm(target) {
        return Err(RemovalRefusal::NotRegistered);
    }
    Ok(vec!["remove".into(), "--global".into(), name.into()])
}

fn preflight_uv(
    name: &str,
    target: &Path,
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
) -> Result<Vec<String>, RemovalRefusal> {
    let root = inventory::uv_tool_root(run).map_err(refusal_from)?;
    if inventory::join_norm(&root, name) != norm(target) {
        return Err(RemovalRefusal::NotRegistered);
    }
    Ok(vec!["tool".into(), "uninstall".into(), name.into()])
}

fn preflight_pipx(
    name: &str,
    target: &Path,
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
) -> Result<Vec<String>, RemovalRefusal> {
    let tools = inventory::pipx_venvs(run).map_err(refusal_from)?;
    let tool = tools
        .iter()
        .find(|tool| tool.name == name)
        .ok_or(RemovalRefusal::NotRegistered)?;
    // 老版本 pipx 不报 `environment`，那就只认名字登记（`venvs/<name>`
    // 是 pipx 自己的约定，不是我们猜的布局）。
    if let Some(venv) = &tool.venv {
        if norm(venv) != norm(target) {
            return Err(RemovalRefusal::NotRegistered);
        }
    }
    Ok(vec!["uninstall".into(), name.into()])
}

/// pip 包的预检：探活 pip、重验 dist-info 登记、从登记重读名字。
///
/// dist-info 目录（PEP 376）就是 pip 自己的登记：不在了 = 目标已经卸载，
/// `NotRegistered`。名字从登记重读而不是沿用扫描期缓存——卸载 argv 由它
/// 拼出，「执行什么」必须在预检这一刻定死。program 是包条目自带的解释器
/// 绝对路径（venv 的包就是 venv 自己的 python），不走 PATH。
fn preflight_pip(
    item: &DevAssetItem,
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
) -> Result<(String, Vec<String>), RemovalRefusal> {
    let DevAssetKind::PipPackage { python, .. } = &item.kind else {
        return Err(RemovalRefusal::UnsupportedTool);
    };
    let program = python.to_string_lossy().to_string();

    // pip 探活：解释器没了、pip 没装，都算命令不可用，在动目标之前拒绝，
    // 与「卸载命令跑到一半失败」分开报。
    match run(&program, &["-m", "pip", "--version"], PIP_PROBE_TIMEOUT) {
        None => return Err(RemovalRefusal::ToolUnavailable),
        Some(result) if !result.ok => return Err(RemovalRefusal::ToolUnavailable),
        Some(_) => {}
    }

    if !item.path.is_dir() {
        return Err(RemovalRefusal::NotRegistered);
    }
    let metadata = std::fs::read_to_string(item.path.join("METADATA"))
        .map_err(|_| RemovalRefusal::ToolUnreadable)?;
    let name = super::python::metadata_field(&metadata, "Name")
        .filter(|name| super::python::is_valid_distribution_name(name))
        .ok_or(RemovalRefusal::ToolUnreadable)?;

    Ok((
        program,
        vec![
            "-m".into(),
            "pip".into(),
            "uninstall".into(),
            "-y".into(),
            name,
        ],
    ))
}

/// 同时复验生态凭据与扫描期对象身份，拒绝根和祖先上的重定向。
fn preflight_venv(item: &DevAssetItem) -> Result<(), RemovalRefusal> {
    let DevAssetKind::VirtualEnv { identity, .. } = &item.kind else {
        return Err(RemovalRefusal::UnsupportedTool);
    };
    if !identity.is_some_and(|identity| identity.recheck(&item.path)) {
        return Err(RemovalRefusal::NotRegistered);
    }
    if !item.path.join("pyvenv.cfg").is_file() {
        return Err(RemovalRefusal::NotRegistered);
    }
    Ok(())
}

/// 所有节点的保护、链接和删除重试必须复用统一 cleaner。
fn remove_tree(root: &Path) -> bool {
    crate::core::cleaner::delete_tree(root, &Default::default())
        == crate::core::cleaner::CleanResult::Ok
}

fn norm(path: &Path) -> String {
    crate::core::safety::norm(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::dev_env::{DevAssetKind, PythonToolKind, SiteScope, VenvManager};
    use crate::core::inuse::SpotCheck;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn proc_run(stdout: &str, ok: bool) -> ProcRun {
        ProcRun {
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
            exit_code: Some(if ok { 0 } else { 1 }),
            ok,
        }
    }

    /// 一个假命令执行器：按 (程序, 参数) 查表回答，未登记的调用一律返回
    /// `None`（= 命令跑不起来），用来把「测不出」和「没有」分开测。
    struct FakeRunner {
        answers: HashMap<String, Option<ProcRun>>,
        calls: Vec<String>,
    }

    impl FakeRunner {
        fn new() -> Self {
            Self {
                answers: HashMap::new(),
                calls: Vec::new(),
            }
        }
        fn answers(mut self, key: &str, value: Option<ProcRun>) -> Self {
            self.answers.insert(key.to_string(), value);
            self
        }
        fn run(&mut self, program: &str, args: &[&str], _timeout: Duration) -> Option<ProcRun> {
            let key = format!("{program} {}", args.join(" "));
            self.calls.push(key.clone());
            self.answers.remove(&key).unwrap_or(None)
        }
    }

    fn conda_env(name: &str, path: &str) -> DevAssetItem {
        DevAssetItem {
            id: format!("conda:{path}"),
            kind: DevAssetKind::CondaEnv {
                name: name.into(),
                is_base: false,
                python_version: None,
                package_count: None,
            },
            path: PathBuf::from(path),
            size: Default::default(),
            last_change: None,
            in_use: SpotCheck::Clear,
            is_active_env: false,
            source: AssetSource::Tool,
        }
    }

    fn node_pkg(manager: &str, name: &str, path: &str) -> DevAssetItem {
        DevAssetItem {
            id: format!("{manager}:{name}"),
            kind: DevAssetKind::NodeGlobalPackage {
                manager: manager.into(),
                name: name.into(),
                version: "1.0.0".into(),
                bin_shims: Vec::new(),
            },
            path: PathBuf::from(path),
            size: Default::default(),
            last_change: None,
            in_use: SpotCheck::Clear,
            is_active_env: false,
            source: AssetSource::Tool,
        }
    }

    fn python_tool(kind: PythonToolKind, name: &str, path: &str) -> DevAssetItem {
        DevAssetItem {
            id: format!("{kind:?}:{name}"),
            kind: DevAssetKind::PythonTool {
                tool_kind: kind,
                name: name.into(),
                executables: vec![name.into()],
            },
            path: PathBuf::from(path),
            size: Default::default(),
            last_change: None,
            in_use: SpotCheck::Clear,
            is_active_env: false,
            source: AssetSource::Tool,
        }
    }

    const CONDA_INFO: &str = r#"{
        "envs": ["/home/u/miniconda3", "/home/u/miniconda3/envs/old"],
        "root_prefix": "/home/u/miniconda3"
    }"#;

    #[test]
    fn base_environment_is_refused_even_though_nothing_lists_it_as_protected() {
        // base 是 `~/miniconda3` 本身，core::safety 不会拦它——拦住它的是
        // conda 自己报的 root_prefix。这一条测的是 `is_base` 标记万一没打上，
        // root_prefix 仍然兜得住。
        let mut item = conda_env("base", "/home/u/miniconda3");
        item.kind = DevAssetKind::CondaEnv {
            name: "base".into(),
            is_base: false,
            python_version: None,
            package_count: None,
        };
        let mut runner =
            FakeRunner::new().answers("conda info --json", Some(proc_run(CONDA_INFO, true)));
        let outcome = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
        assert_eq!(
            outcome,
            RemovalOutcome::Refused(RemovalRefusal::ProtectedEnvironment)
        );
        assert!(
            !runner.calls.iter().any(|c| c.contains("remove")),
            "拒绝之后不得再跑卸载命令，实际调用 {:?}",
            runner.calls
        );
    }

    #[test]
    fn flagged_base_environment_is_refused_before_any_command_runs() {
        let mut item = conda_env("base", "/home/u/miniconda3");
        item.kind = DevAssetKind::CondaEnv {
            name: "base".into(),
            is_base: true,
            python_version: None,
            package_count: None,
        };
        let mut runner = FakeRunner::new();
        let outcome = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
        assert_eq!(
            outcome,
            RemovalOutcome::Refused(RemovalRefusal::ProtectedEnvironment)
        );
        assert!(runner.calls.is_empty(), "闸门应在跑任何命令之前拦下");
    }

    #[test]
    fn active_environment_is_refused() {
        let mut item = conda_env("work", "/home/u/miniconda3/envs/work");
        item.is_active_env = true;
        let mut runner = FakeRunner::new();
        let outcome = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
        assert_eq!(
            outcome,
            RemovalOutcome::Refused(RemovalRefusal::ProtectedEnvironment)
        );
        assert!(runner.calls.is_empty());
    }

    #[test]
    fn busy_or_unknown_occupancy_is_refused() {
        for status in [SpotCheck::Busy, SpotCheck::Unknown] {
            let mut item = conda_env("work", "/home/u/miniconda3/envs/work");
            item.in_use = status;
            let mut runner = FakeRunner::new();
            let outcome = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
            assert_eq!(outcome, RemovalOutcome::Refused(RemovalRefusal::Busy));
            assert!(runner.calls.is_empty());
        }
    }

    #[test]
    fn missing_tool_refuses_without_touching_the_target() {
        let item = conda_env("old", "/home/u/miniconda3/envs/old");
        // 执行器对任何命令都回答 None = 命令跑不起来。
        let outcome = remove_asset_with(&item, |_, _, _| None);
        assert_eq!(
            outcome,
            RemovalOutcome::Refused(RemovalRefusal::ToolUnavailable)
        );
    }

    #[test]
    fn unreadable_inventory_is_not_treated_as_an_empty_one() {
        let item = conda_env("old", "/home/u/miniconda3/envs/old");
        let mut runner =
            FakeRunner::new().answers("conda info --json", Some(proc_run("not json at all", true)));
        let outcome = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
        assert_eq!(
            outcome,
            RemovalOutcome::Refused(RemovalRefusal::ToolUnreadable)
        );
    }

    #[test]
    fn target_no_longer_in_the_inventory_is_refused() {
        let item = conda_env("gone", "/home/u/miniconda3/envs/gone");
        let mut runner =
            FakeRunner::new().answers("conda info --json", Some(proc_run(CONDA_INFO, true)));
        let outcome = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
        assert_eq!(
            outcome,
            RemovalOutcome::Refused(RemovalRefusal::NotRegistered)
        );
    }

    #[test]
    fn npm_prefix_mismatch_is_refused() {
        // 扫描期看到的是另一个前缀下的同名包（中途换了 node 版本管理器），
        // 此时 `npm uninstall --global` 删的是当前前缀下那个同名的包，
        // 而不是我们列出来的那一个。
        let scanned = std::env::temp_dir().join("qc-scanned-prefix");
        let live = std::env::temp_dir().join("qc-live-prefix");
        let item = node_pkg(
            "npm",
            "typescript",
            &scanned
                .join("node_modules")
                .join("typescript")
                .to_string_lossy(),
        );
        let mut runner = FakeRunner::new()
            .answers(
                "npm prefix --global",
                Some(proc_run(&format!("{}\n", live.display()), true)),
            )
            .answers(
                "npm ls --global --depth=0 --json",
                Some(proc_run(
                    r#"{"dependencies":{"typescript":{"version":"5.4.2"}}}"#,
                    true,
                )),
            );
        let outcome = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
        assert_eq!(
            outcome,
            RemovalOutcome::Refused(RemovalRefusal::NotRegistered)
        );
        assert!(
            !runner.calls.iter().any(|c| c.contains("uninstall")),
            "预检不通过不得跑卸载命令"
        );
    }

    #[test]
    fn a_global_package_that_is_only_a_dependency_is_never_authorized() {
        // `--depth=0` 的清单里没有它 = 它是别的全局包的依赖，不给删除授权。
        let prefix = std::env::temp_dir().join("qc-npm-global");
        let item = node_pkg(
            "npm",
            "some-transitive-dep",
            &prefix
                .join("node_modules")
                .join("some-transitive-dep")
                .to_string_lossy(),
        );
        let mut runner = FakeRunner::new()
            .answers(
                "npm prefix --global",
                Some(proc_run(&format!("{}\n", prefix.display()), true)),
            )
            .answers(
                "npm ls --global --depth=0 --json",
                Some(proc_run(
                    r#"{"dependencies":{"typescript":{"version":"5.4.2"}}}"#,
                    true,
                )),
            );
        let outcome = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
        assert_eq!(
            outcome,
            RemovalOutcome::Refused(RemovalRefusal::NotRegistered)
        );
    }

    #[test]
    fn failed_removal_never_falls_back_to_deleting_the_directory() {
        let dir = std::env::temp_dir().join("qc_dev_remove_failed");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let item = conda_env("old", &dir.to_string_lossy());
        let info = format!(
            r#"{{"envs": ["{}"], "root_prefix": "/home/u/miniconda3"}}"#,
            dir.to_string_lossy().replace('\\', "/")
        );
        let prefix = dir.to_string_lossy().to_string();
        let mut runner = FakeRunner::new()
            .answers("conda info --json", Some(proc_run(&info, true)))
            .answers(
                &format!("conda remove --prefix {prefix} --all --yes"),
                Some(proc_run("", false)),
            );

        let outcome = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
        assert!(
            matches!(outcome, RemovalOutcome::Failed(ref f) if f.code == "command-failed"),
            "命令失败必须是 Failed，实际 {outcome:?}"
        );
        assert!(dir.exists(), "命令失败后不得回退裸删——目录必须原样还在");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_success_exit_code_without_the_target_actually_going_away_is_a_failure() {
        let dir = std::env::temp_dir().join("qc_dev_remove_liar");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let item = conda_env("old", &dir.to_string_lossy());
        let info = format!(
            r#"{{"envs": ["{}"], "root_prefix": "/home/u/miniconda3"}}"#,
            dir.to_string_lossy().replace('\\', "/")
        );
        let prefix = dir.to_string_lossy().to_string();
        // 命令报成功，目录却还在。
        let mut runner = FakeRunner::new()
            .answers("conda info --json", Some(proc_run(&info, true)))
            .answers(
                &format!("conda remove --prefix {prefix} --all --yes"),
                Some(proc_run("", true)),
            );

        let outcome = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
        assert!(
            matches!(outcome, RemovalOutcome::Failed(ref f) if f.code == "reported-success-but-remains"),
            "退出码 0 不能当作完成，实际 {outcome:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_verified_removal_reports_removed() {
        let dir = std::env::temp_dir().join("qc_dev_remove_ok");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let item = conda_env("old", &dir.to_string_lossy());
        let info = format!(
            r#"{{"envs": ["{}"], "root_prefix": "/home/u/miniconda3"}}"#,
            dir.to_string_lossy().replace('\\', "/")
        );
        let prefix = dir.to_string_lossy().to_string();
        let removal_dir = dir.clone();
        let mut runner = FakeRunner::new()
            .answers("conda info --json", Some(proc_run(&info, true)))
            .answers(
                &format!("conda remove --prefix {prefix} --all --yes"),
                Some(proc_run("", true)),
            );

        let outcome = remove_asset_with(&item, |p, a, t| {
            let result = runner.run(p, a, t);
            // 模拟命令真的删掉了环境目录。
            if a.contains(&"remove") {
                let _ = std::fs::remove_dir_all(&removal_dir);
            }
            result
        });
        assert_eq!(outcome, RemovalOutcome::Removed);
        assert!(!dir.exists());
    }

    #[test]
    fn a_layout_sourced_item_has_no_removal_channel() {
        // 权限判断不能只存在于视图里：即使调用方没检查 `is_removable()`
        // 就把降级条目送进来，这里也必须拒绝，而不是拿猜出来的路径动刀。
        let mut item = conda_env("old", "/home/u/miniconda3/envs/old");
        item.source = AssetSource::Layout;
        let mut runner = FakeRunner::new();
        let outcome = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
        assert_eq!(
            outcome,
            RemovalOutcome::Refused(RemovalRefusal::UnverifiedSource)
        );
        assert!(runner.calls.is_empty(), "拒绝应在跑任何命令之前");
    }

    #[test]
    fn bun_has_no_removal_channel_until_its_inventory_is_verified() {
        let item = node_pkg(
            "bun",
            "typescript",
            "/home/u/.bun/install/global/node_modules/typescript",
        );
        let mut runner = FakeRunner::new();
        let outcome = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
        assert_eq!(
            outcome,
            RemovalOutcome::Refused(RemovalRefusal::UnsupportedTool)
        );
        assert!(runner.calls.is_empty());
    }

    #[test]
    fn pipx_removal_freezes_the_argv_at_preflight() {
        let item = python_tool(
            PythonToolKind::Pipx,
            "black",
            "/home/u/.local/pipx/venvs/black",
        );
        let listed = r#"{"venvs": {"black": {"metadata": {"environment": "/home/u/.local/pipx/venvs/black"}}}}"#;
        let mut runner = FakeRunner::new()
            .answers("pipx list --json", Some(proc_run(listed, true)))
            .answers("pipx uninstall black", Some(proc_run("", false)));

        let outcome = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
        assert!(matches!(outcome, RemovalOutcome::Failed(_)));
        assert!(
            runner.calls.iter().any(|c| c == "pipx uninstall black"),
            "预检应定死 `pipx uninstall <name>`，实际调用 {:?}",
            runner.calls
        );
    }

    #[test]
    fn a_read_error_is_not_absence() {
        // stat 失败里只有 NotFound 算消失。权限拒绝与任何其他 I/O 失败都是
        // 「测不出」，必须继续按「还在」处理——否则一个读不了的路径会被
        // 当成删除成功报给用户。
        use std::io::ErrorKind;
        assert!(absence_from_stat(Some(ErrorKind::NotFound)));
        assert!(!absence_from_stat(None), "stat 成功 = 路径还在");
        assert!(!absence_from_stat(Some(ErrorKind::PermissionDenied)));
        assert!(!absence_from_stat(Some(ErrorKind::NotADirectory)));
        assert!(!absence_from_stat(Some(ErrorKind::InvalidInput)));
    }

    #[test]
    fn absence_probe_agrees_with_the_filesystem() {
        let dir = std::env::temp_dir().join("qc_dev_absent_probe");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a-file");
        std::fs::write(&file, b"x").unwrap();

        assert!(!confirmed_absent(&file), "存在的文件不是缺席");
        assert!(confirmed_absent(&dir.join("never-existed")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- pip 包与 venv：新通道的闸门 ----

    /// 测试里统一用的解释器绝对路径。FakeRunner 直接拦下调用，路径不必真实存在。
    const PY: &str = "C:/Python313/python.exe";

    fn pip_package(name: &str, dist_info: &Path) -> DevAssetItem {
        DevAssetItem {
            id: format!("pip:{}", crate::core::safety::norm(dist_info)),
            kind: DevAssetKind::PipPackage {
                python: PathBuf::from(PY),
                name: name.into(),
                version: Some("1.0.0".into()),
            },
            path: dist_info.to_path_buf(),
            size: Default::default(),
            last_change: None,
            in_use: SpotCheck::Clear,
            is_active_env: false,
            source: AssetSource::Tool,
        }
    }

    fn interpreter_row(scope: SiteScope, path: &str) -> DevAssetItem {
        DevAssetItem {
            id: format!("python:{}", crate::core::safety::norm(Path::new(path))),
            kind: DevAssetKind::PythonInterpreter {
                name: "Python 3.13.14".into(),
                version: Some("3.13.14".into()),
                scope,
                package_count: Some(1),
                site_packages: Some(PathBuf::from(path)),
                python: PathBuf::from(PY),
            },
            path: PathBuf::from(path),
            size: Default::default(),
            last_change: None,
            in_use: SpotCheck::Clear,
            is_active_env: false,
            source: AssetSource::Tool,
        }
    }

    fn venv_item(path: &Path) -> DevAssetItem {
        DevAssetItem {
            id: format!("venv:{}", crate::core::safety::norm(path)),
            kind: DevAssetKind::VirtualEnv {
                name: "proj".into(),
                version: Some("3.13.14".into()),
                base_python: Some("C:/Python313".into()),
                package_count: Some(2),
                manager: VenvManager::Virtualenvwrapper,
                identity: super::super::VenvIdentity::capture(path),
            },
            path: path.to_path_buf(),
            size: Default::default(),
            last_change: None,
            in_use: SpotCheck::Clear,
            is_active_env: false,
            source: AssetSource::Tool,
        }
    }

    fn fresh_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn review_venv_preserves_protected_children() {
        let _guard = crate::core::whitelist::lock_for_test();
        let dir = crate::core::testing::fixture("qc_review_venv_keep")
            .canonicalize()
            .unwrap();
        std::fs::write(dir.join("pyvenv.cfg"), "home = Python\n").unwrap();
        let keep = dir.join("keep");
        std::fs::create_dir(&keep).unwrap();
        let sentinel = keep.join("important.txt");
        std::fs::write(&sentinel, b"must survive").unwrap();
        crate::core::whitelist::reload(&[keep.to_string_lossy().into_owned()]);
        let outcome = remove_asset_with(&venv_item(&dir), |_, _, _| None);
        let survived = std::fs::read(&sentinel).ok();
        crate::core::whitelist::clear();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(survived.as_deref(), Some(b"must survive".as_slice()));
        assert!(matches!(outcome, RemovalOutcome::Failed(_)));
    }

    #[test]
    fn review_venv_replaced_after_scan_is_refused() {
        let dir = crate::core::testing::fixture("qc_review_venv_replace")
            .canonicalize()
            .unwrap();
        let env = dir.join("env");
        std::fs::create_dir(&env).unwrap();
        std::fs::write(env.join("pyvenv.cfg"), "home = Python\n").unwrap();
        let item = venv_item(&env);
        std::fs::rename(&env, dir.join("original")).unwrap();
        std::fs::create_dir(&env).unwrap();
        std::fs::write(env.join("pyvenv.cfg"), "home = Python\n").unwrap();
        let sentinel = env.join("important.txt");
        std::fs::write(&sentinel, b"new environment").unwrap();
        let outcome = remove_asset_with(&item, |_, _, _| None);
        let survived = sentinel.exists();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            outcome,
            RemovalOutcome::Refused(RemovalRefusal::NotRegistered)
        );
        assert!(survived);
    }

    #[cfg(windows)]
    #[test]
    fn review_venv_junction_root_and_ancestor_are_refused() {
        let dir = crate::core::testing::fixture("qc_review_venv_junction");
        let outside = dir.join("outside");
        std::fs::create_dir(&outside).unwrap();
        for path in [&outside, &outside.join("env")] {
            std::fs::create_dir_all(path).unwrap();
            std::fs::write(path.join("pyvenv.cfg"), "home = Python\n").unwrap();
            std::fs::write(path.join("important.txt"), b"outside").unwrap();
        }
        let link = dir.join("link");
        let output = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output);
        let discovered = super::super::python::virtual_envs(
            &[
                (dir.clone(), VenvManager::Poetry),
                (link.clone(), VenvManager::Poetry),
            ],
            &[link.clone(), link.join("env")],
        );
        assert!(discovered.iter().all(|item| !item.path.starts_with(&link)));
        let root = remove_asset_with(&venv_item(&link), |_, _, _| None);
        let ancestor = remove_asset_with(&venv_item(&link.join("env")), |_, _, _| None);
        let survived =
            outside.join("important.txt").exists() && outside.join("env/important.txt").exists();
        let _ = std::fs::remove_dir(&link);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(root, RemovalOutcome::Refused(RemovalRefusal::NotRegistered));
        assert_eq!(
            ancestor,
            RemovalOutcome::Refused(RemovalRefusal::NotRegistered)
        );
        assert!(survived);
    }

    #[test]
    fn review_mamba_inventory_freezes_mamba_executor() {
        let item = conda_env("old", "C:/qc-review/envs/old");
        let info = r#"{"envs":["C:/qc-review/envs/old"],"root_prefix":"C:/qc-review"}"#;
        let mut runner = FakeRunner::new()
            .answers("mamba info --json", Some(proc_run(info, true)))
            .answers(
                "mamba remove --prefix C:/qc-review/envs/old --all --yes",
                Some(proc_run("", false)),
            );
        let result = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
        assert!(matches!(result, RemovalOutcome::Failed(_)));
        assert!(
            runner
                .calls
                .iter()
                .any(|call| call == "mamba remove --prefix C:/qc-review/envs/old --all --yes"),
            "{:?}",
            runner.calls
        );
    }

    #[test]
    fn review_venv_unknown_identity_is_refused_but_in_place_changes_are_allowed() {
        let dir = fresh_dir("qc_review_venv_identity");
        std::fs::write(dir.join("pyvenv.cfg"), "home = Python\n").unwrap();
        let mut unknown = venv_item(&dir);
        let DevAssetKind::VirtualEnv { identity, .. } = &mut unknown.kind else {
            unreachable!()
        };
        *identity = None;
        assert!(!can_remove(&unknown));
        assert_eq!(
            remove_asset_with(&unknown, |_, _, _| None),
            RemovalOutcome::Refused(RemovalRefusal::NotRegistered)
        );
        let item = venv_item(&dir);
        std::fs::write(dir.join("new-package.txt"), "changed in place").unwrap();
        assert_eq!(
            remove_asset_with(&item, |_, _, _| None),
            RemovalOutcome::Removed
        );
    }

    /// 造一个带 `METADATA` 的 dist-info，返回它的路径。
    fn write_dist_info(site: &Path, dir_name: &str, name: &str) -> PathBuf {
        let dir = site.join(dir_name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("METADATA"),
            format!("Metadata-Version: 2.1\nName: {name}\nVersion: 1.26.0\n"),
        )
        .unwrap();
        dir
    }

    /// 卸载 argv 必须以 `METADATA` 里的登记名为准，而不是扫描期缓存的字段：
    /// dist-info 是 pip 自己的登记，执行什么在预检这一刻定死。
    #[test]
    fn pip_uninstall_freezes_the_argv_from_the_dist_info_metadata() {
        let site = fresh_dir("qc_pip_argv_site");
        let dist_info = write_dist_info(&site, "numpy-1.26.0.dist-info", "numpy");
        // 扫描期缓存的名字故意与登记不一致，argv 只能来自登记。
        let item = pip_package("scan-time-name", &dist_info);
        let uninstall = format!("{PY} -m pip uninstall -y numpy");
        let mut runner = FakeRunner::new()
            .answers(
                &format!("{PY} -m pip --version"),
                Some(proc_run("pip 26.1.2", true)),
            )
            .answers(&uninstall, Some(proc_run("", false)));

        let outcome = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
        assert!(matches!(outcome, RemovalOutcome::Failed(_)));
        assert!(
            runner.calls.iter().any(|call| call == &uninstall),
            "argv 必须从登记重读定死，实际调用 {:?}",
            runner.calls
        );

        let _ = std::fs::remove_dir_all(&site);
    }

    #[test]
    fn a_pip_package_whose_dist_info_vanished_is_refused() {
        // dist-info 是 pip 的登记：登记没了 = 目标已经不在清单里。
        let site = fresh_dir("qc_pip_gone_site");
        let item = pip_package("numpy", &site.join("numpy-1.26.0.dist-info"));
        let mut runner = FakeRunner::new().answers(
            &format!("{PY} -m pip --version"),
            Some(proc_run("pip 26.1.2", true)),
        );

        let outcome = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
        assert_eq!(
            outcome,
            RemovalOutcome::Refused(RemovalRefusal::NotRegistered)
        );
        assert!(
            !runner.calls.iter().any(|call| call.contains("uninstall")),
            "预检不通过不得跑卸载命令，实际调用 {:?}",
            runner.calls
        );

        let _ = std::fs::remove_dir_all(&site);
    }

    #[test]
    fn pip_missing_from_the_interpreter_refuses_before_touching_the_target() {
        let site = fresh_dir("qc_pip_nopip_site");
        let dist_info = write_dist_info(&site, "numpy-1.26.0.dist-info", "numpy");
        let item = pip_package("numpy", &dist_info);
        // 执行器对任何命令都回答 None = pip 探活失败。
        let outcome = remove_asset_with(&item, |_, _, _| None);
        assert_eq!(
            outcome,
            RemovalOutcome::Refused(RemovalRefusal::ToolUnavailable)
        );
        assert!(dist_info.exists(), "拒绝之后目标必须原样还在");

        let _ = std::fs::remove_dir_all(&site);
    }

    #[test]
    fn a_failed_pip_uninstall_never_falls_back_to_deleting_the_site() {
        let site = fresh_dir("qc_pip_failed_site");
        let dist_info = write_dist_info(&site, "numpy-1.26.0.dist-info", "numpy");
        let item = pip_package("numpy", &dist_info);
        let mut runner = FakeRunner::new()
            .answers(
                &format!("{PY} -m pip --version"),
                Some(proc_run("pip 26.1.2", true)),
            )
            .answers(
                &format!("{PY} -m pip uninstall -y numpy"),
                Some(proc_run("", false)),
            );

        let outcome = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
        assert!(
            matches!(outcome, RemovalOutcome::Failed(ref f) if f.code == "command-failed"),
            "命令失败必须是 Failed，实际 {outcome:?}"
        );
        assert!(
            dist_info.exists(),
            "命令失败后不得回退裸删——登记必须原样还在"
        );

        let _ = std::fs::remove_dir_all(&site);
    }

    #[test]
    fn pip_exit_zero_without_the_dist_info_gone_is_a_failure() {
        let site = fresh_dir("qc_pip_liar_site");
        let dist_info = write_dist_info(&site, "numpy-1.26.0.dist-info", "numpy");
        let item = pip_package("numpy", &dist_info);
        // 命令报成功，dist-info 却还在。
        let mut runner = FakeRunner::new()
            .answers(
                &format!("{PY} -m pip --version"),
                Some(proc_run("pip 26.1.2", true)),
            )
            .answers(
                &format!("{PY} -m pip uninstall -y numpy"),
                Some(proc_run("", true)),
            );

        let outcome = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
        assert!(
            matches!(outcome, RemovalOutcome::Failed(ref f) if f.code == "reported-success-but-remains"),
            "退出码 0 不能当作完成，实际 {outcome:?}"
        );

        let _ = std::fs::remove_dir_all(&site);
    }

    #[test]
    fn a_verified_pip_uninstall_reports_removed() {
        let site = fresh_dir("qc_pip_ok_site");
        let dist_info = write_dist_info(&site, "numpy-1.26.0.dist-info", "numpy");
        let item = pip_package("numpy", &dist_info);
        let removal_dir = dist_info.clone();
        let mut runner = FakeRunner::new()
            .answers(
                &format!("{PY} -m pip --version"),
                Some(proc_run("pip 26.1.2", true)),
            )
            .answers(
                &format!("{PY} -m pip uninstall -y numpy"),
                Some(proc_run("", true)),
            );

        let outcome = remove_asset_with(&item, |p, a, t| {
            let result = runner.run(p, a, t);
            // 模拟 pip 真的卸掉了这个发行版。
            if a.contains(&"uninstall") {
                let _ = std::fs::remove_dir_all(&removal_dir);
            }
            result
        });
        assert_eq!(outcome, RemovalOutcome::Removed);
        assert!(!dist_info.exists());

        let _ = std::fs::remove_dir_all(&site);
    }

    /// 行不是删除单位：解释器的包目录行（两个 scope 都一样）永远没有入口，
    /// 可移除的是它里面的 pip 包条目（P41 的剩余一半）。
    #[test]
    fn a_site_row_is_never_the_deletion_unit_but_its_packages_are() {
        for scope in [SiteScope::Install, SiteScope::User] {
            let row = interpreter_row(
                scope,
                "C:/Users/u/AppData/Roaming/Python/Python313/site-packages",
            );
            assert!(!can_remove(&row), "{scope:?} 行本身不该有移除入口");
            let mut runner = FakeRunner::new();
            let outcome = remove_asset_with(&row, |p, a, t| runner.run(p, a, t));
            assert_eq!(
                outcome,
                RemovalOutcome::Refused(RemovalRefusal::UnsupportedTool)
            );
            assert!(runner.calls.is_empty(), "拒绝应在跑任何命令之前");
        }

        let site = fresh_dir("qc_pip_row_site");
        let dist_info = write_dist_info(&site, "numpy-1.26.0.dist-info", "numpy");
        let package = pip_package("numpy", &dist_info);
        assert!(can_remove(&package), "包条目才有移除入口");

        let _ = std::fs::remove_dir_all(&site);
    }

    #[test]
    fn a_venv_without_pyvenv_cfg_is_refused() {
        // 凭据重验：cfg 不在了 = 这条路径已经不是扫描期那个 venv。
        let dir = fresh_dir("qc_venv_noproof");
        let item = venv_item(&dir);
        let mut runner = FakeRunner::new();

        let outcome = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
        assert_eq!(
            outcome,
            RemovalOutcome::Refused(RemovalRefusal::NotRegistered)
        );
        assert!(runner.calls.is_empty());
        assert!(dir.exists(), "拒绝之后目标必须原样还在");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_venv_is_refused() {
        let dir = fresh_dir("qc_venv_active");
        let mut item = venv_item(&dir);
        item.is_active_env = true;
        let mut runner = FakeRunner::new();

        let outcome = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
        assert_eq!(
            outcome,
            RemovalOutcome::Refused(RemovalRefusal::ProtectedEnvironment)
        );
        assert!(runner.calls.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_venv_on_a_protected_path_is_refused() {
        // 用户主目录本身在 core::safety 的保护表里（HOME_EXACT）。
        let Some(home) = crate::platform::user_home() else {
            return;
        };
        let item = venv_item(&home);
        let mut runner = FakeRunner::new();

        let outcome = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
        assert_eq!(outcome, RemovalOutcome::Refused(RemovalRefusal::Protected));
        assert!(runner.calls.is_empty(), "路径保护闸门不得放行任何命令");
    }

    /// venv 的删除凭据是 pyvenv.cfg 本身，不看扫描来源：按目录扫出来的
    /// （Layout）与 poetry 报上来的可删性没有区别——预检放行了它，才会
    /// 走到 cfg 重验报 NotRegistered。
    #[test]
    fn a_layout_sourced_venv_is_not_blocked_by_the_source_gate() {
        let dir = fresh_dir("qc_venv_layout");
        let mut item = venv_item(&dir);
        item.source = AssetSource::Layout;
        let mut runner = FakeRunner::new();

        let outcome = remove_asset_with(&item, |p, a, t| runner.run(p, a, t));
        assert_eq!(
            outcome,
            RemovalOutcome::Refused(RemovalRefusal::NotRegistered),
            "来源闸门不该拦 venv；拦下它的应是 cfg 重验"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_verified_venv_deletion_reports_removed() {
        let dir = fresh_dir("qc_venv_delete_ok");
        std::fs::write(dir.join("pyvenv.cfg"), "home = C:/Python313\n").unwrap();
        let packages = dir.join("Lib").join("site-packages");
        std::fs::create_dir_all(packages.join("numpy-1.26.0.dist-info")).unwrap();
        std::fs::create_dir_all(dir.join("Scripts")).unwrap();
        std::fs::write(dir.join("Scripts").join("python.exe"), b"").unwrap();

        let item = venv_item(&dir);
        let outcome = remove_asset_with(&item, |_, _, _| None);
        assert_eq!(
            outcome,
            RemovalOutcome::Removed,
            "整树删除后目录必须确认消失"
        );
        assert!(!dir.exists());
    }

    #[test]
    fn a_venv_deletion_that_leaves_the_directory_is_a_failure() {
        let dir = fresh_dir("qc_venv_delete_liar");
        std::fs::write(dir.join("pyvenv.cfg"), "home = C:/Python313\n").unwrap();
        let item = venv_item(&dir);

        // 删除函数报告成功，目录却还在：退出码 0 不算数，venv 也一样。
        let outcome = remove_asset_with_tree(&item, |_, _, _| None, |_| true);
        assert!(
            matches!(outcome, RemovalOutcome::Failed(ref f) if f.code == "reported-success-but-remains"),
            "实际 {outcome:?}"
        );

        // 删除本身失败（文件删不掉）：报 Failed，没有兜底动作。
        let outcome = remove_asset_with_tree(&item, |_, _, _| None, |_| false);
        assert!(
            matches!(outcome, RemovalOutcome::Failed(ref f) if f.code == "directory-removal-failed"),
            "实际 {outcome:?}"
        );
        assert!(dir.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
