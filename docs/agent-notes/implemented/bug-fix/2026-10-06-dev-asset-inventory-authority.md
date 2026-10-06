# Agent Note: Dev-asset inventory comes from the ecosystem, never from listing the directory

Status: implemented
Partly-superseded-by: 2026-10-07-pip-package-and-venv-removal.md

## Problem

开发环境页最初这样列全局包：读 `node_modules` 的直接子目录，每个目录算一个包。
在 bun 上这是一场灾难——本机实测 `~/.bun/install/global/package.json` 声明了 **3**
个全局包，而同目录的 `node_modules` 里有 **132** 个目录：另外 129 个是那 3 个的
**传递依赖**。

后果有两层，第二层更严重：

1. 界面被噪声淹没，用户要找自己装的那个包得在 129 行别人的依赖里翻。
2. 更糟的是这 129 行**看起来都一样可删**。删掉 `@babel/parser` 会打断依赖它的
   那个工具，而用户根本不知道它是谁装的。

同一个错误还有几个更安静的版本：`~/.conda` 被当成 conda 安装根，于是凭空多出
一个名叫 `base` 的假环境（体积是配置目录的体积）；Windows 上取
`user_cache_dir().parent()` 得到 `%USERPROFILE%\AppData`，于是探的是
`AppData\miniconda3` 而不是真实的 `%LOCALAPPDATA%\miniconda3`；环境去重按原始
`PathBuf` 比较，`Miniconda3` 与 `miniconda3` 各留一条。

## Decision

清单分两级，条目自己带着这个事实（`AssetSource`）：

- `AssetSource::Tool`：生态自己声明的。全局包的顶层成员取
  `node_modules` 旁边的 `package.json` 的 `dependencies`（bun 与 pnpm 都写在那里），
  或命令输出（`npm ls --global --depth=0 --json`、`conda info --json`、
  `pipx list --json`、`uv tool dir`）。
- `AssetSource::Layout`：命令与声明都拿不到时，按已知安装布局扫出来的降级结果。

只有 `Tool` 来源的条目进入移除通道；`Layout` 条目照常展示与称重，但只读。
这条判断在 `remove::channel_for` 一处定义，界面用的 `remove::can_remove` 与执行
用的 `PreflightRemoval` 都从它取——分两处写迟早漂移成「界面有按钮、执行说没有
通道」。

`core::dev_env::inventory` 是「这个生态现在装了什么」的唯一解析入口：发现与移除
问的是同一个函数。两边各写一份解析的话，就会出现「界面上有这个包、点删除却说不
存在」。

去重、体积、来源三件事一起修：跨通道按 `safety::norm` 去重，体积改用
`measure_batch_storage` 一次批量测定（逐项相加会把 conda 的 `pkgs` 缓存与环境之间
的硬链接算两遍），时间点标到来源（`TimestampSource`），base 环境由 conda 自己报的
`root_prefix` 认定而不是猜安装根。

## Alternatives considered

按目录列、但把「是别人的依赖」的条目灰掉——拒绝：传递依赖与用户装的包在目录层面
没有任何可靠区别，灰掉需要我们自己读全量 `package.json` 反推依赖图，而生态已经
把答案写在清单文件里了。

自己解析 lockfile 求依赖图——拒绝：lockfile 格式与版本强相关，且生态的命令/清单
已经把顶层答案给出来了，再实现一遍依赖解析就是第二套真相来源。

保留 `~/.conda` 作为安装根但标成非 base——拒绝：它不是环境，多一条假条目就是在
教用户忽略这个列表。

`dependencies` 键不存在时按「一个都没装」处理——拒绝：清单文件在、但没有该键，
不等于什么都没装，拿它去隐藏真实存在的目录是拿未知当结论；返回 `None` 退回按
目录列，并把来源降为 `Layout`。

## Consequences

本机条目从 139 条降到 14 条：npm 7 个（真实全局包）、bun 3 个（与它的清单一致）、
pnpm 1 个、uv 工具 3 个。降级路径仍然存在（命令不可用、清单读不到），此时条目
只展示——宁可少一个删除入口，也不用未经生态确认的路径去删。

`can_remove` 只保证「有通道」，不保证「命令当下可用」：pnpm 未装或不在 PATH 上时
界面仍会给按钮，点下去得到的是明确的拒绝原因而不是静默失败。这是有意的——按钮
的存在与否不该依赖一次 PATH 探测，而拒绝原因本身对用户是有用信息。

`docs/PITFALLS.md` 的 P38 与 P39 记录了这两条：清单权威性，以及「目录 mtime 不是
使用时间」。

## Verification

- `src/core/dev_env/discovery.rs::a_transitive_dependency_is_not_mistaken_for_an_installed_package`
- `src/core/dev_env/discovery.rs::bun_items_are_never_removable_even_when_its_manifest_declares_them`
- `src/core/dev_env/discovery.rs::without_a_manifest_the_listing_falls_back_to_the_directory`
- `src/core/dev_env/discovery.rs::dot_conda_directory_is_never_listed_as_an_environment`
- `src/core/dev_env/discovery.rs::inspect_conda_meta_reads_version_count_and_transaction_time`
- `src/core/dev_env/discovery.rs::timestamps_only_come_from_tool_authored_sources`
- `src/core/dev_env/remove.rs::a_layout_sourced_item_has_no_removal_channel`
- `src/core/dev_env/discovery.rs::live_probe_lists_the_assets_on_this_machine`

Proved: 撤掉「按生态清单取顶层」这一步、退回按目录列之后，
`a_transitive_dependency_is_not_mistaken_for_an_installed_package` 失败，
`left: ["its-dep", "tool"]` / `right: ["tool"]`，本机实探从 11 条 node 条目跳到
139 条；证据 `docs/agent-notes-evidence/2026-10-06-dev-asset-inventory-authority-red.log`。
恢复后同一测试通过、实探回到 11 条，证据 `...-green.log`。Pitfalls checked: P31
（只认清单一来源的身份，不放松「测不出」）、P32（靠内容/自证而不是名字）。
macOS 路径未在本机执行，由 `resolve_tool_program` 的契约与同一套解析逻辑覆盖。

## Superseded

仍然成立：本条的全部结论——清单权威性、`AssetSource` 两级来源、
`channel_for` 单一名单、`inventory` 单一解析入口。

需要补充的边界（2026-10-07）：生态写在自己安装目录里的**登记文件**也算
「生态自己声明」，枚举它们不是「按目录列」——pip 的 `*.dist-info`（PEP 376）
与 venv 的 `pyvenv.cfg`（PEP 405）是这两个生态自己写的账本，读它与跑命令
同一权威。venv 的删除凭据因此是 cfg 本身而不看扫描来源。见
`2026-10-07-pip-package-and-venv-removal.md` 与 P38 的条目修订。
