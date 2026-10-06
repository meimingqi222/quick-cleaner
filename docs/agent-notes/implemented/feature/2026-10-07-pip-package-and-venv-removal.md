# Agent Note: pip packages in user sites and venvs are removable, and venvs can be deleted whole

Status: implemented

## Problem

用户级 site-packages 里的 144 个包（669 MB）与各虚拟环境里的包在界面上全部
「只展示」：没有任何清理入口。用户的论点成立——pip 装的包 `pip install` 就能
原样装回来，「重新安装即可恢复」的边界在这里**是**成立的；venv 整体也是一个
自包含、可重建的单元。当初把「解释器自带包目录」「用户包目录」「venv」捆在
一起判成只读，把两层本来不同的东西连坐了。

真正站得住的只读理由只剩一条：解释器**自带**的 `Lib/site-packages`（安装级）
是解释器的一部分，商店版在文件系统层就不可写（TrustedInstaller），且
`core/safety.rs` 不保护 `C:\Program Files` 之下的子目录，删那里没有兜底。

## Decision

按「恢复边界在哪里、删除后有没有第二处状态要同步」重新划线：

- **安装级行整行只读**，不可展开，维持 P41 的禁止。
- **用户级目录与 venv 的 site-packages 里的包逐个可卸载**。每个包枚举自
  `*.dist-info` 目录（PEP 376）——那是 pip 自己写的登记，读它与跑
  `pip list` 同一权威，不是 P38 禁止的「按目录猜安装」。卸载走
  `python -m pip uninstall -y <name>`（`remove::Channel::Pip`）：program 是
  条目自带的解释器绝对路径（venv 的包用 venv 自己的 python），预检先探活
  `python -m pip --version`，再重验 dist-info 目录还在、从 `METADATA` 重读
  `Name:` 并校验 PEP 503 字符集，argv 在预检定死；事后核验 dist-info 目录
  NotFound。名字读不出/不合规的包**不产条目**——argv 要用它，宁缺不猜。
- **venv 整体可删除**（`remove::Channel::VenvDirectory`）。这是移除通道里
  唯一的非命令路径：venv 不在任何包管理器的登记里，`pyvenv.cfg`（PEP 405）
  是它全部的「已安装」状态，删目录没有第二处状态要同步。预检重验
  `pyvenv.cfg` 仍在并匹配扫描期目录稳定身份（否则 `NotRegistered`）；
  根与所有祖先有 symlink/reparse 时拒绝，身份未知不放行。整树删除复用
  `cleaner::delete_tree`，逐节点查询 safety，受保护子项保留并报整体失败；来源闸门对它不生效（按目录扫出来的与 poetry 报上来
  的可删性没有区别，凭据是 cfg 本身）。P20 不适用——那条禁的是「跑过生态
  命令后回退裸删」，这里从头就没有命令可跑。「项目还在不在用」依然无证据
  （P39 的处境），由确认弹窗交给用户判断，正文措辞照实说「整目录删除」。
- **界面**：用户级行与 venv 行加展开箭头，包列表展开时从磁盘枚举（无子进程），
  每个包子行带卸载按钮；包卸载成功就地摘掉子行并减父行计数（体积待下次
  扫描刷新，确认弹窗如实给版本号不拿 0 冒充）；venv 删除沿用 `drop_asset`。
  包列表是展开间隙枚举的，重扫后整份清空。

包名一律取 `METADATA` 的 `Name:`，不从目录名猜：`python-dateutil-2.9.0.dist-info`
里哪个连字符是分隔符没法从目录名确定。

## Alternatives considered

安装级目录也开放按包卸载（探测可写性）——拒绝：商店版在文件系统层就写不进
去，可写探测又添一层边界情况；卸载 pip/setuptools 会弄坏解释器工具链。用户
选择保持只读。

「一键清空用户包目录」——拒绝（用户选择）：逐包卸载粒度最细、风险最小；批量
argv 一条命令删 144 个包，错一步的代价不成比例。

包删除走「按 dist-info 的 RECORD 删文件」而不是 `pip uninstall`——拒绝：生态
命令是唯一执行方式（P20 的前提），pip 自己处理 egg-info、脚本、`__pycache__`
这些 RECORD 之外的边角。

venv 删除前跑 `poetry env remove` 之类管理器命令——拒绝：virtualenvwrapper
没有跨平台删除命令，poetry 不一定在场；venv 没有外部登记，目录删除本来就是
完整的生态操作。

单独递归删除 venv、只检查根路径保护——拒绝：白名单子目录仍可能被删。
复用 cleaner 的逐节点保护、链接处理与只读/ACL 重试，避免维护第二条删除机制。

只重验同名 cfg 而接受环境替换——拒绝：新环境不属于扫描期授权。Windows
必须拿到卷序列号和文件 ID，Unix 只比 dev+ino；不比较 mtime，原地装包不算替换。

## Consequences

venv 发现阶段冻结稳定目录身份，发现和执行阶段均拒绝根或祖先上的重定向。
未知身份无移除入口，执行前再次复核，替换后需要重新扫描确认。受保护子项
会保留，父目录无法删空时如实报失败。测试夹具先规范化可信临时根，避免
macOS 的系统临时目录别名被误当成待删环境的链接。

包卸载仍由对应解释器执行，安装级包目录仍只读，pip 失败仍不回退裸删。

## Verification

- `src/core/dev_env/python.rs::pip_packages_are_named_by_the_dist_info_metadata`
- `src/core/dev_env/python.rs::a_dist_info_without_a_trustworthy_name_is_not_listed`
- `src/core/dev_env/python.rs::metadata_field_reads_only_the_header_and_ignores_case`
- `src/core/dev_env/python.rs::a_virtualenv_exposes_its_own_python`
- `src/core/dev_env/python.rs::one_row_per_package_directory_so_the_count_matches_pip_list`
- `src/core/dev_env/remove.rs::pip_uninstall_freezes_the_argv_from_the_dist_info_metadata`
- `src/core/dev_env/remove.rs::a_pip_package_whose_dist_info_vanished_is_refused`
- `src/core/dev_env/remove.rs::pip_missing_from_the_interpreter_refuses_before_touching_the_target`
- `src/core/dev_env/remove.rs::a_failed_pip_uninstall_never_falls_back_to_deleting_the_site`
- `src/core/dev_env/remove.rs::pip_exit_zero_without_the_dist_info_gone_is_a_failure`
- `src/core/dev_env/remove.rs::a_verified_pip_uninstall_reports_removed`
- `src/core/dev_env/remove.rs::a_site_row_is_never_the_deletion_unit_but_its_packages_are`
- `src/core/dev_env/remove.rs::a_venv_without_pyvenv_cfg_is_refused`
- `src/core/dev_env/remove.rs::active_venv_is_refused`
- `src/core/dev_env/remove.rs::a_venv_on_a_protected_path_is_refused`
- `src/core/dev_env/remove.rs::a_layout_sourced_venv_is_not_blocked_by_the_source_gate`
- `src/core/dev_env/remove.rs::a_verified_venv_deletion_reports_removed`
- `src/core/dev_env/remove.rs::a_venv_deletion_that_leaves_the_directory_is_a_failure`

Proved: 把 `PreparedKind::DirectoryTree` 的结果映射临时改回「删除失败」与
「报成功但目录还在」混成一个码的写法，
`a_venv_deletion_that_leaves_the_directory_is_a_failure` 失败，
`实际 Failed(directory-removal-failed)`（期望 `reported-success-but-remains`）；
证据 `docs/agent-notes-evidence/2026-10-07-pip-package-and-venv-removal-red.log`，
同一 log 末尾是恢复后同 focused 测试的绿跑。Pitfalls checked: P38（dist-info /
pyvenv.cfg 是生态自己的登记，豁免已写进条目）、P41（安装级维持只读）、P20
（pip 通道失败不回退，venv 无命令可跑）、P40（`python -m pip` 经
`run_tool_with_timeout`）、P5（树删除出口走 `remove_dir_forcing`）、P39（时间
来源如实标注，不参与删除判定）。

- `src/core/dev_env/remove.rs::review_venv_preserves_protected_children`
- `src/core/dev_env/remove.rs::review_venv_replaced_after_scan_is_refused`
- `src/core/dev_env/remove.rs::review_venv_junction_root_and_ancestor_are_refused`
- `src/core/dev_env/remove.rs::review_venv_unknown_identity_is_refused_but_in_place_changes_are_allowed`

Proved: 修复前前三个安全回归分别断言失败：白名单 sentinel 被删、替换后的
环境返回 Removed、Junction 返回 Removed；原始红跑在
`docs/agent-notes-evidence/2026-10-07-review-removal-red.log`，恢复后 focused
绿跑在 `docs/agent-notes-evidence/2026-10-07-review-removal-green.log`。
Pitfalls checked: P5/P6/P15/P24/P25/P28/P37/P38/P41/P43/P45，保留共享安全
机制，不扩张删除授权。Windows 实跑，macOS 未实跑。
