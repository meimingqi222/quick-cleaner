# Agent Note: WebKit residuals and exact Homebrew cask uninstall

Status: implemented

## Problem

按应用卸载前扫描使用 satellite_dirs，独立的孤儿扫描使用 orphan_roots。
只有后者列了 WebKit，导致 Ghostty 的 Library/WebKit/com.mitchellh.ghostty
在正常卸载流程漏扫。应用直接进入废纸篓也不会注销 Homebrew cask；
brew cleanup 只清理缓存/旧版本，不能代替卸载安装记录。

## Decision

WebKit 加入按应用的卫星目录表，以完整 Bundle ID 和已有封闭 helper 后缀匹配，
不把同厂商或以主 ID 开头的另一产品默认当成该应用的数据。使用 Other 来源，
与 HTTPStorages 一样走既有残留审核、身份复核、占用探测和废纸篓处置。
孤儿扫描的保守判据及默认不勾选行为不变。

macOS 的已审查脚本与内部 GUI 手动策略保持最高优先级。其余卸载先查询标准
Homebrew 可执行文件的 info --json=v2 --installed --cask，精确比较声明的 app target
与所选应用的规范路径；不从显示名或 Bundle ID 猜 token。只处理拥有一个 app target
且归属唯一的 cask；同名的另一安装副本不匹配。清单或 target 不可读/缺失时明确失败，
不能当作非 Homebrew 安装继续裸删。查询前捕获目录身份，执行前再次复核身份与 plist ID，
拒绝链接和 core safety 保护的路径。

只执行固定参数 uninstall --cask <full_token>，不加 zap、force 或依赖清理。
命令复用进程树超时约束。成功必须同时确认应用路径 NotFound 和 cask 登记消失；
超时、非零退出、未知清单或残留记录均返回错误，阻止自带卸载器/废纸篓回退。

**合入状态**：Homebrew cask 卸载最终采用远程的 receipt 绑定实现
（`src/platform/macos/cask_uninstall.rs`，见
`2026-10-16-tested-app-uninstall-coverage.md`），本 note 的
`brew info --json=v2 --installed --cask` 方案未合入，原实现保留在
`docs/temp/superseded/brew_uninstall.rs` 仅作存档。WebKit 卫星目录部分
已合入，但远程同时把 WebKit 声明在 `review_id_dirs`：同时出现在两张表的
子目录按「可能」发射、不预选（审核表更保守），helper 后缀仍靠卫星匹配入表。

## Alternatives considered

通过小写应用名猜 cask 会错误匹配同名程序与自定义安装路径。
直接删除 Caskroom、调用全局 brew cleanup 或加 --zap 无法保证只注销所选应用，
还会绕过 Homebrew 的关联产物管理或提前清除用户数据。
遇到命令失败回退删本体会留下安装记录，并隐藏真正失败原因。

## Consequences

Ghostty 这类应用的 WebKit 数据进入现有卸载后残留审核。Homebrew 管理的单应用
cask 通过生态自己的卸载命令处理其关联产物和记录。多应用 cask、旧版 Homebrew
未提供绝对 app target、清单不可用时要求用户通过 Homebrew 处理，宁可拒绝，不能猜。
本体已经被其他工具删掉的历史 cask 记录没有基于 Bundle ID 的删除授权，仍不自动注销。
审查过的保留数据脚本维持原路线；此适配器不取代脚本策略。

## Verification

- `src/platform/macos/residuals.rs::webkit_residual_scan_matches_ghostty_without_sibling_products`
- `src/platform/macos/apps.rs::homebrew_error_never_reaches_trash_fallback`（防
  止归属检查错误落到废纸篓回退的约束不变；其余 brew 专属测试随未合入实现
  一并移出生产树）

Proved: WebKit 修复前自然红跑，断言 missing Ghostty WebKit: []，加入规则后同 focused 转绿，
见 `docs/agent-notes-evidence/2026-10-16-webkit-red.log` 和
`docs/agent-notes-evidence/2026-10-16-webkit-green.log`。
以下 brew 相关证据记录的是**未合入**的 `brew info` 方案（见上文「合入状态」）：
制造型分别只撤一处防护：把 brew 错误吞成 false 后，卸载入口测试实际返回 Ok(())；
只撤 cask 登记残留判据后，本体消失但登记仍在的测试错误返回 Ok(true)。每次均立即
恢复同一防护并运行相同 focused 转绿。证据分别为
`docs/agent-notes-evidence/2026-10-16-brew-fallback-red.log`、
`docs/agent-notes-evidence/2026-10-16-brew-fallback-green.log`、
`docs/agent-notes-evidence/2026-10-16-brew-completion-red.log` 和
`docs/agent-notes-evidence/2026-10-16-brew-completion-green.log`。

未合入方案的 8 个夹具测试还覆盖路径归属、同名副本、多应用/多主人歧义、无效清单、
身份被替换、链接、固定无 zap 参数、超时、命令错误与未知完成状态；随实现一并移至
`docs/temp/superseded/brew_uninstall.rs` 存档。真实 Homebrew 只读查询核对过
JSON app target 形态，未卸载真实应用。Windows 执行未验证。

Pitfalls checked: 完整核对 P1–P64。P1/Windows 命令解析不变；P19/P28 的去重与
Unix dev/ino 不变；P20 的已尝试生态命令禁止裸删回退由错误传播覆盖；P31/P60 的
孤儿所有者未知拒绝不变；P36 的规则完整性随构建/规则检查验证；P37 的逐项身份
与原有残留处置不变；P46 复用有界进程树执行；P49/P50 的输入法脚本与手动策略
保持优先。新增 P65。其余 UI、更新、Windows 权限与开发环境防护未触及。

Final validation (macOS, rustc 1.95.0): cargo fmt --check、cargo build、
cargo clippy --all-targets -- -D warnings 均通过；cargo test --lib 为 811 passed、
0 failed、12 ignored。首次全量测试暴露卫星目录固定数量仍为 7；同步为 8，
并增加 WebKit/Other 的显式配置断言后，完整套件转绿。cargo run --example rules -- check
通过（schema=11、13 rules）。严格 notes 校验、PITFALLS 指针、UI 组件边界和本地化
守卫通过；PITFALLS 的 7 个无 Note 指针警告为已有项。
