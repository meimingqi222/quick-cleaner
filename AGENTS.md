# AGENTS.md

QuickCleaner：Rust + GPUI 的 Windows / macOS 磁盘清理工具。分层是 `ui → core → platform`，不要倒过来依赖。

更完整的现状和模块图见 [`docs/HANDOFF.md`](docs/HANDOFF.md)，需求见 [`docs/REQUIREMENTS.md`](docs/REQUIREMENTS.md)。

`docs/` 默认不进库，需要长期维护的文档要在 `.gitignore` 里逐个放行。临时调查、审查和 spike 结论放进 `docs/temp/`，只保留在本地。

## 命令

```bash
cargo fmt --check
cargo build
cargo test --lib
cargo clippy --all-targets -- -D warnings
cargo run -- --no-elevate
```

CI 卡 `cargo fmt --check` 和 clippy `-D warnings`。提交前这几项都要能过。

仓库带 `.githooks/pre-commit`（未格式化时自动 `cargo fmt` 并拦下提交），clone 后执行一次 `git config core.hooksPath .githooks` 启用。

## Commit 前必做：核对立项 pitfalls

准备 commit **之前**，必须打开 [`docs/PITFALLS.md`](docs/PITFALLS.md)，逐条核对本轮改动有没有重新踩中。

1. 读完整份列表，不要只看标题。
2. 本轮动到某条列出的文件或相关逻辑时，那一条视为 **必须过**：不能拆掉防护、不能把测试改成永远绿、不能用「看起来更合理」的回退（例如把 `is_file()` 改回 `exists()`）。
3. 没直接改那些文件，也要扫一眼是否间接触及（解析命令行、拉起卸载器、残留名字匹配、路径保护）。
4. 核对结果写进自己的检查，不要默认「这次无关」。
5. 新发现的、用户已经踩过、根因不直观、以后很容易改回去的问题，**补进 PITFALLS**，不要只写在 commit message 里。

现在已有清单见 [`docs/PITFALLS.md`](docs/PITFALLS.md)——它是唯一的 pitfalls 台账，编号以它为准（P10–P13 已废弃空缺）。有 `Note:` 指针的条目，决策理由和测试绑定在 `docs/agent-notes/` 对应的 note 里单点维护；`Note:` 指针由 `scripts/check-pitfall-notes.py` 校验（CI 已接线），note 归档或改名时必须同批更新指针。

## 回归记录（agent-notes）

非平凡 bug 修复必须「同一次提交里附带一份 note + 一个回归测试」：note 记决策与备选，测试锁行为，`## Verification` 的 `Proved:` 行记红跑证据。写 note 前先 `python tools/regression-notes/verify-notes.py --notes-dir docs/agent-notes --find "<关键词>"` 查重。

**改文件之前**，对要动的路径跑反查，被引用的 note 先读再改：

```bash
python tools/regression-notes/verify-notes.py --notes-dir docs/agent-notes --for-path <staged 或待改文件>
```

pre-commit 会对暂存文件自动打印命中提醒（不阻断）；CI 的 `agent-notes.yml` 跑格式校验 + PITFALLS 指针校验。

红跑两种都算数，但都要留证据：修复前真实失败（TDD 先红）直接算数，`Proved:` 写明断言和证据位置；修复后才写的测试走制造型——只撤一处防护、只跑 focused、存 log、恢复后立即同 focused 确认绿。证据 log 提交到 `docs/agent-notes-evidence/`（`.gitignore` 已放行），不要 cite `target/` 里会被清理的路径。

## 改代码时

- 路径能不能删，只问 `core/safety.rs`。不要在 cleaner / MFT / residuals 里再抄一份黑名单。
- Windows 卸载走官方 `UninstallString`，成功判据是登记项或安装目录没了，不是进程退出码 0。
- 界面文案走 `ui/i18n.rs` 的 `tr_*`，不要在视图里写死中英串。
- 注释只写非显然的约束，不要叙述实现步骤。

## 规则 DSL 准入

- 新增 `Selection` 变体，引入时至少要有 2 条 bundled 声明使用；只服务一个应用的逻辑留在 provider 里。确有理由单用的，写进 `SINGLE_USE_SELECTIONS` 并给出理由，由 `selection_kinds_meet_the_admission_bar` 检查。新增 leaf signature 或 `Operation` 变体按同一标准判断（测试只检查 signature 至少被用到一次，不数次数）。
- 新 selection 必须在 `rules::claims::Claims::collect` 里显式决定认不认领（那里是穷尽 match）；新 `Operation` 变体必须补 `plan.rs` 上的属性方法（`removes_directory`/`covers_subtree`/`route` 等，穷尽 match）。
- 把 provider 逻辑迁成规则时，先用**未改代码**生成迁移基线（`rules/fixtures/*-baseline.json`），迁移后逐行一致；基线不许为让测试通过而改，测试里不留写基线的开关。
- 类别的安全等级与分组在 `engine.toml` 的 `[categories.*]`，文案在 `ui/i18n` 的 `tr_category_*`。

## 系统托盘内容开发边界

- 托盘窗口统一使用 `gpui-app-host`（兄弟仓库 `../gpui-app-kit`）；本地 `src/ui/tray_host.rs` 只适配图标和本地化菜单。公共宿主负责开关、定位、焦点、Esc、失焦与销毁。
- `src/ui/views/tray_popover.rs` 及其同名子目录只负责布局、读取模型与发送 `TraySession` 事件。仅调整 UI 时不要改公共宿主或桌面生命周期适配。
- 采样、命令与动作路由放 `src/ui/tray_overview.rs`；新增动作在控制层声明是否收起窗口，内容不要自行关窗、defer、spawn、quit 或直接 update 模型。
- 复用方式与验证见 [托盘宿主架构说明](docs/agent-notes/implemented/architecture/2026-10-15-tray-host-content-boundary.md)。现有 `python3 scripts/check-ui-components.py` 会检查内容边界；不要加 baseline 例外规避检查。

共享输入编辑、滚动和浮层核心在 `gpui-app-widgets`；产品配色、OverlayId 和业务关闭回调留在应用。
构建通过 Cargo.toml 中固定提交的 Git 依赖获取公共库；`../gpui-app-kit` 只用于公共库联合开发、内容守卫和回归测试 anchor 校验。接入与私库认证见公共库 README.md。
