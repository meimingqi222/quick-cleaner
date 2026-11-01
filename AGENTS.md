# AGENTS.md

QuickCleaner 是 Windows / macOS 磁盘清理工具，使用 Go + MyGo 编写。产品入口为 `cmd/quick-cleaner`，业务代码在 `internal/`；分层是 `ui → core → platform`，不要倒过来依赖。

## 开发方向

- 只维护 Go 版本。原 Rust + GPUI 参照树（`src/`、`examples/`、Cargo 与 Rust CI）已于 2026-11-01 删除；需要查旧行为时从该日期之前的 git 历史里看，不要恢复或重新引入 Rust 代码。
- 保留既有界面和交互，不做无关的重新设计。原生平台验收仍在进行中，现状见 [`GO_MIGRATION.md`](GO_MIGRATION.md)。

更完整的现状和模块图见 [`docs/HANDOFF.md`](docs/HANDOFF.md)，需求见 [`docs/REQUIREMENTS.md`](docs/REQUIREMENTS.md)。

`docs/` 默认不进库，需要长期维护的文档要在 `.gitignore` 里逐个放行。临时调查、审查和 spike 结论放进 `docs/temp/`，只保留在本地。

## 命令

日常验证（按改动范围选择，提交前核对相关 CI 要求）：

```bash
gofmt -l cmd internal assets rules tools
go vet ./...
go test -race ./...
go build ./...
go run ./cmd/quick-cleaner --no-elevate
```

`.github/workflows/go.yml` 在 Windows、macOS 和 Linux 检查格式、vet、race 测试和构建，并检查 Windows 资源。运行产品程序不是 dry run，实际操作仍会修改文件。

扫描性能可直接用 `go run ./tools/indexbench -mode refresh -product-discovery -rounds 2` 测量（macOS），无需启动 GUI；默认使用临时缓存副本，覆盖完整发现规划，并分开报告前台返回与后台保存等待时间。缓存解析/单树发现微基准用 `go run ./tools/indexbench`。参数及 CPU profile 用法见 [`tools/indexbench/README.md`](tools/indexbench/README.md)。

仓库带 `.githooks/pre-commit`（暂存 Go 文件的 gofmt 检查、agent-notes 反查提醒、PITFALLS 指针校验），clone 后可执行 `git config core.hooksPath .githooks` 启用；提交时以实际 hook 输出为准。

## Commit 前必做：核对立项 pitfalls

准备 commit **之前**，必须打开 [`docs/PITFALLS.md`](docs/PITFALLS.md)，逐条核对本轮改动有没有重新踩中。

1. 读完整份列表，不要只看标题。
2. 本轮动到某条列出的文件或相关逻辑时，那一条视为 **必须过**：不能拆掉防护、不能把测试改成永远绿、不能用「看起来更合理」的回退（例如把「是文件」检查改回「存在」检查）。
3. 没直接改那些文件，也要扫一眼是否间接触及（解析命令行、拉起卸载器、残留名字匹配、路径保护）。
4. 核对结果写进自己的检查，不要默认「这次无关」。
5. 新发现的、用户已经踩过、根因不直观、以后很容易改回去的问题，**补进 PITFALLS**，不要只写在 commit message 里。

现在已有清单见 [`docs/PITFALLS.md`](docs/PITFALLS.md)——它是唯一的 pitfalls 台账，编号以它为准（P10–P13 已废弃空缺）。较早的条目以 Rust 实现写成，其中的 `.rs` 路径和测试名是历史记录，约束本身同样适用于对应的 Go 代码。有 `Note:` 指针的条目，决策理由和测试绑定在 `docs/agent-notes/` 对应的 note 里单点维护；`Note:` 指针由 `scripts/check-pitfall-notes.py` 校验（CI 已接线），note 归档或改名时必须同批更新指针。

## 回归记录（agent-notes）

非平凡 bug 修复必须「同一次提交里附带一份 note + 一个回归测试」：note 记决策与备选，测试锁行为，`## Verification` 的 `Proved:` 行记红跑证据。写 note 前先 `python tools/regression-notes/verify-notes.py --notes-dir docs/agent-notes --find "<关键词>"` 查重。

**改文件之前**，对要动的路径跑反查，被引用的 note 先读再改：

```bash
python tools/regression-notes/verify-notes.py --notes-dir docs/agent-notes --for-path <staged 或待改文件>
```

pre-commit 会对暂存文件自动打印命中提醒（不阻断）；CI 的 `agent-notes.yml` 跑格式校验 + PITFALLS 指针校验。本地全量校验与 CI 一致：

```bash
RETIRED="--retired-prefix src/ --retired-prefix examples/ --retired-prefix scripts/test_ui_"
python -X utf8 tools/regression-notes/verify-notes.py --notes-dir docs/agent-notes --strict-anchors $RETIRED
python -X utf8 tools/regression-notes/verify-notes.py --notes-dir docs/agent-notes --dump-anchors > anchors.txt
python -X utf8 scripts/check-anchors-vs-tests.py anchors.txt $RETIRED
```

`--retired-prefix` 只用于已删除的 Rust 参照树及其 UI 检查脚本：旧 note 引用的这些测试保留为历史，不要求存在。新 note 只能绑定现存的 Go 测试，锚点必须是引用文件里真实的 `func` 声明。

红跑两种都算数，但都要留证据：修复前真实失败（TDD 先红）直接算数，`Proved:` 写明断言和证据位置；修复后才写的测试走制造型——只撤一处防护、只跑 focused、存 log、恢复后立即同 focused 确认绿。证据 log 提交到 `docs/agent-notes-evidence/`（`.gitignore` 已放行），不要 cite 临时目录里会被清理的路径。

## 改代码时

- 路径能不能删，只问中央安全模块 `internal/core/safety.go`。不要在 cleaner / MFT / residuals 里再抄一份黑名单。
- Windows 卸载走官方 `UninstallString`，成功判据是登记项或安装目录没了，不是进程退出码 0。
- 界面文案走 `internal/ui/` 下的 i18n 代码；不要在视图里写死中英串。
- 注释只写非显然的约束，不要叙述实现步骤。

## 规则（`rules/*.toml`）

- 规则是声明，不是回调；单个应用专用的逻辑留在 provider 里，不为它新增通用选择种类。
- 把 provider 逻辑迁成规则时，先用**未改代码**生成迁移基线（`rules/fixtures/*-baseline.json`），迁移后逐行一致；基线不许为让测试通过而改，测试里不留写基线的开关。
- 类别的安全等级与分组在 `engine.toml` 的 `[categories.*]`，文案在 `internal/ui/junk_i18n.go`。
