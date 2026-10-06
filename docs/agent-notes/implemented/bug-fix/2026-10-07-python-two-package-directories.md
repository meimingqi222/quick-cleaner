# Agent Note: A Python interpreter has two package directories, and they are two rows

Status: implemented
Partly-superseded-by: 2026-10-07-pip-package-and-venv-removal.md

## Problem

开发环境页把 Python 这一组显示成「1 个包」，而同一台机器上 `pip list` 报 145 个。
用户看到的是「`pip list` 明明有东西，界面说没有」——同一类表象这次已经是第二次
出现（第一次是 npm 通道整体查不到）。

根因是模型错了，不是解析错了。一个 Python 解释器有**两个**包目录：

- 解释器自带的 `Lib/site-packages`（`sysconfig` 的 `purelib`）；
- 用户级 site-packages（`site.getusersitepackages()`）。

本机（微软商店版 CPython 3.13）实测：安装目录里 1 个包（pip），用户级目录
`…\AppData\Local\Packages\PythonSoftwareFoundation.Python.3.13_…\LocalCache\local-packages\Python313\site-packages`
里 144 个，合计正好是 `pip list` 的 145。商店版尤其明显——**安装目录只读**，
用户 `pip install` 的东西只可能落在用户级目录；系统 Python 上也是同一个道理，
只是份额没这么极端。

模型只认一个「site-packages」，于是量到了那个几乎空的安装目录（19 MB），
144 个包（669 MB）完全不出现。

同一轮还踩到一个更隐蔽的坑：`site.getsitepackages()` 返回的是**列表**，而商店版
上它的第 0 个元素是 `prefix`（整个安装目录），不是包目录。照 `[0]` 取会把几百 MB
的安装目录算成包的体积。现在走 `sysconfig.get_paths()["purelib"]`，并且列表形态
一律不认（`an_array_shaped_package_dir_is_rejected_rather_than_indexed`）——宁可
留空，也不猜一个可能指错地方的路径。

## Decision

`InterpreterFacts` hold both directories；`interpreter_rows` 对**每一个存在的包
目录出一行**，行上标出 `SiteScope::{Install, User}`，所以：

- 两行加起来才是 `pip list` 报的包数量；
- 界面上写明是哪一级（「解释器自带包目录」/「用户包目录」），否则用户会以为
  重复了，或者以为其中一行算错了；
- 两行本身都不是删除单位（行级 `channel_for` 返回 `None`）：`site-packages`
  是解释器的一部分，不是自包含资产，见 `core/dev_env/python` 模块头。行里
  的 pip 包另有条目，见 `## Superseded` 一节。

包数量改用本地数 `*.dist-info`（`count_distributions`），不再为它跑一次子进程枚举
发行版：那是每次扫描都要付的固定成本，而答案就在目录里。探测脚本因此只回答
「是什么、装在哪」，不回答「装了多少」。

行构造抽成纯函数 `interpreter_rows(name, program, facts, seen_sites)`：`interpreters`
还要先解析 PATH 上的解释器，那一步依赖本机装没装 Python，纯函数才测得动。

## Alternatives considered

只报用户级目录（把安装目录藏起来）——拒绝：安装目录里的包（pip、setuptools）
真实存在且会占空间，藏起来就变成另一种「界面说没有」。

两行合并成一行、数量相加、体积相加——拒绝：合并后无法说明「669 MB 在哪」，
而用户正是为了看这个；而且两个目录的可迁移性完全不同（用户级可以整个重建，
安装级由商店/安装器管）。

把 `site.getsitepackages()[0]` 当包目录——拒绝，这正是修掉的坑（列表第 0 个是
prefix）。同理，`sys.prefix + "/Lib/site-packages"` 这种拼法在允许用户级安装的
解释器上就是错的，所以宁可留空。

## Consequences

本机条目从 13 变成 14（多出用户级包目录一行），逻辑总规模从 2.21 GB 变成 2.86 GB
——之前那 669 MB 根本没被统计到。Python 侧的扫描也变慢了一点（多走 1.7 万个
文件），但体积测算是并行的，总代价从 3.6 s 变到 4.9 s。

「可移除项可释放」不含这两行：它们只读，这个页面不会去删，算进去就是报一个做
不到的数。

## Verification

- `src/core/dev_env/python.rs::one_row_per_package_directory_so_the_count_matches_pip_list`
- `src/core/dev_env/python.rs::probe_reads_version_prefix_and_both_package_dirs`
- `src/core/dev_env/python.rs::an_array_shaped_package_dir_is_rejected_rather_than_indexed`
- `src/core/dev_env/python.rs::a_virtualenv_reports_its_base_interpreter_and_package_count`

Proved: 撤掉「用户级包目录单独成行」之后，
`one_row_per_package_directory_so_the_count_matches_pip_list` 失败，
`left: 1, right: 2`，且剩下那一行退化成 `package_count: None` / `source: Layout`
的兜底行——正是用户看到的「1 个包」；证据
`docs/agent-notes-evidence/2026-10-07-python-two-site-rows-red.log`。恢复后同一
测试通过，本机实探从 1 行变 2 行（`0 conda, 2 python, 10 node, 2 tools`）。
Pitfalls checked: P38（清单只能来自生态自己）。Windows 上实际执行；macOS 侧
未在本机跑，但那两级的划分与平台无关（`site.getusersitepackages()` 两个平台都有）。

## Superseded

仍然成立：一个解释器两个包目录、各成一行、数量加起来对上 `pip list`、
`getsitepackages()[0]` 不认——本条的核心修复全部有效。

不再成立：当初「两行都是只读、永不给移除入口」的结论只对了一半。安装级行
整行只读维持；用户级行与 venv 行本身仍不可删，但可以展开，行内的 pip 包
逐个走 `python -m pip uninstall` 卸载，venv 整体以 `pyvenv.cfg` 为凭删除——
划分与机制见 `2026-10-07-pip-package-and-venv-removal.md`。上面的断言
`one_row_per_package_directory_so_the_count_matches_pip_list` 中「两行
`!can_remove`」仍然通过，因为行不是删除单位。
