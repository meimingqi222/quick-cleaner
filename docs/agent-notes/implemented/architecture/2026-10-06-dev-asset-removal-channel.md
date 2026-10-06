# Agent Note: Dev assets are removed by the ecosystem's own command, and only after a fresh preflight

Status: implemented
Partly-superseded-by: 2026-10-07-pip-package-and-venv-removal.md

## Problem

开发环境页要能移除 conda 环境、全局包与 CLI 工具。第一版只做了探测与展示；补上
移除时有两条老路可走，两条都不该走：

- **删目录**：conda 环境、pipx / uv 的工具 venv、npm 全局包都是「工具自己拥有内部
  登记」的目录（conda 的 `conda-meta/`、pipx 的 `pipx_metadata.json`、包管理器的
  安装记录）。裸删会留下「登记说装了、实际没了」的半成品——同一类不一致在
  `core::owner`（go module cache、pnpm store）上已经付过一次代价。
- **塞进 `cleaner::clean_targets`**：这条通道要求每个目标都带着规则快照
  （`CleanupPlan` / `RuleRef`），而开发资产不是规则推出来的。为了复用执行器去
  伪造一份计划，等于给「没有规则授权的东西」发一张规则授权，还会连带拖一次规则
  schema 升版——`docs/GOAL.md` 与
  `docs/agent-notes/implemented/architecture/2026-10-05-runtime-provider-policies.md`
  都明确不允许「用展示层/兼容构造器反推授权」。

## Decision

新增 `core::dev_env::remove`，形态照 `core::owner`：独立的生态命令模块，自己带
预检、执行与完成核验，不从规则引擎借授权。

授权来自执行前当场重新验证的三件事，扫描结果一律不算：

1. **目标还在生态自己的清单里**（清单重跑）：conda 靠 `conda info --json` 的
   `envs`，npm 靠 `npm ls --global --depth=0 --json` 的顶级依赖，pipx 靠
   `pipx list --json`，pnpm / uv 靠 `pnpm root --global` / `uv tool dir` 报告的根
   加各自既定布局。
2. **解析出的路径与扫描期一致**：不一致说明中途换过（nvm 切了全局前缀、环境被删掉
   重建），此时拒绝。npm 这一条尤其重要：`npm uninstall --global` 删的是**当前**
   前缀下那个同名的包，不是我们列出来的那一个。
3. **卸载的完整 argv 在预检时就定死**，执行期不再从任何可变状态推导——与安装产物
   「授权不在执行期扩张」同一条纪律。

闸门顺序是「先便宜的本地检查、再花时间跑生态命令」：`safety::is_protected`（路径
能不能删只问它）→ `is_protected()`（base 与激活环境，生态语义而非路径黑名单）→
来源必须是 `AssetSource::Tool` → 占用复检 → 预检 → 执行 → 核验。

完成判据是**目标路径确定消失**（`symlink_metadata` 只认 `NotFound`），不是退出码 0
（AGENTS.md 的既定原则）。失败语义对齐 P20：命令跑不起来、超时、退出码非 0、核验
不通过，四种都是 `Failed`，**禁止回退裸删**——命令可能已经动了一半，在未知状态上
继续动刀比停下更糟。「未执行」（拒绝，目标未被触碰）与「执行了但状态未知」（失败）
分成两个变体报给用户，混为一谈会让用户失去判断依据。

bun 不进这个通道：它的清单输出格式与卸载语义都没有在本机核实过。在核实之前 bun
的条目只展示——「清单可信」与「能删」是两件独立的事，所以来源用 `AssetSource`
表达、可否移除用 `channel_for` 表达，不用同一个字段硬凑。

conda 清单回退到 mamba 时，预检同时冻结实际成功提供清单的前端名称与 argv，
执行调用同一个前端；不能在预检成功后又从 Tool::Conda 固定还原成 conda。
base/root_prefix 防护对两个前端都适用。

## Alternatives considered

把 `Operation::DevAsset` 加进 `core::rules::plan` 并复用 `clean_targets`——拒绝：
要伪造 `CleanupPlan` 与 `RuleRef` 才能进去，那是给没有规则授权的目标发授权，还要
连带升规则 schema；换来的只是复用 `spot_check` 与报告结构，而这两者本来就可以直接
调用（`core::inuse::spot_check`、`core::history::record`）。

删除前只依赖界面做的 `can_remove` 判定——拒绝：权限判断不能只存在于视图里。
`remove_asset` 里的每一道闸门都独立再验一遍，`can_remove` 只是提前告知。

`bun remove -g` 直接放开、出错再说——拒绝：bun 的清单输出格式未核实，一旦解析错
就是删错包。先只读，等真机核实。

把 `node_modules` 的目录 mtime 当「最后使用时间」显示——拒绝：见 P39。

## Consequences

开发环境页现在有两类行：可移除的（带按钮）与只展示的（带徽章）。移除走后台任务，
与扫描任务分开持有，重扫不会取消进行中的移除；移除期间整块禁用按钮，因为同一个
包管理器的两条命令并发会互相抢锁。

`core::history` 记 `dev_asset_remove` 审计；Windows 上占用探测目前只能给出
`Clear`（P15），所以它为拒绝提供不了什么保护，真正的保护是生态命令自己拒绝删活动
环境。

平台无关的代价：conda / npm / pnpm / pipx / uv 五条通道的预检解析都在
`core::dev_env::inventory` 里，与发现层共用；任一生态的清单格式变化会同时影响
展示与移除，这是有意的——两份解析迟早会给出不一致的答案。

## Verification

- `src/core/dev_env/remove.rs::base_environment_is_refused_even_though_nothing_lists_it_as_protected`
- `src/core/dev_env/remove.rs::a_layout_sourced_item_has_no_removal_channel`
- `src/core/dev_env/remove.rs::bun_has_no_removal_channel_until_its_inventory_is_verified`
- `src/core/dev_env/remove.rs::npm_prefix_mismatch_is_refused`
- `src/core/dev_env/remove.rs::a_global_package_that_is_only_a_dependency_is_never_authorized`
- `src/core/dev_env/remove.rs::failed_removal_never_falls_back_to_deleting_the_directory`
- `src/core/dev_env/remove.rs::a_success_exit_code_without_the_target_actually_going_away_is_a_failure`
- `src/core/dev_env/remove.rs::a_read_error_is_not_absence`
- `src/core/dev_env/remove.rs::pipx_removal_freezes_the_argv_at_preflight`

架构记录，不主张 bug-fix 红跑：上面的断言都靠注入式 runner（照 `core::owner` 的
`pnpm_store_prune_with` 的接缝写法）在任何主机上可执行，覆盖拒绝、失败不回退、
完成核验与「读失败不是缺席」四类语义。Windows 上实际执行；macOS 原生验收仍是
未满足的外部条件。真实生态资源未被破坏性验收——本条只做主机构建与单元测试。

## Superseded

仍然成立：命令通道的预检-冻结-核验纪律、失败不回退（P20）、`channel_for`
单一名单、bun 只展示。

不再成立：「移除一律不是删目录」的绝对表述。2026-10-07 起 venv 整体删除走
`Channel::VenvDirectory`——venv 不在任何外部登记里，`pyvenv.cfg` 是它全部的
「已安装」状态，整目录删除没有第二处状态要同步；凭据及扫描期稳定身份预检重验，所有节点复用
`cleaner::delete_tree` 的安全保护，与 P20 不冲突（从头没有命令可跑）。这是唯一
的非命令通道，机制与 pip 逐包卸载见 `2026-10-07-pip-package-and-venv-removal.md`。

## Additional verification

- `src/core/dev_env/remove.rs::review_mamba_inventory_freezes_mamba_executor`

Proved: 修复前 fake runner 只提供 mamba 清单，实际调用却是 conda remove，
断言失败记录在 `docs/agent-notes-evidence/2026-10-07-review-removal-red.log`；
修复后实际调用 mamba remove，focused 绿跑在
`docs/agent-notes-evidence/2026-10-07-review-removal-green.log`。P20/P38 已核对。
