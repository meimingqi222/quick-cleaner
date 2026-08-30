# AGENTS.md

QuickCleaner：Rust + GPUI 的 Windows / macOS 磁盘清理工具。分层是 `ui → core → platform`，不要倒过来依赖。

更完整的现状和模块图见 [`docs/HANDOFF.md`](docs/HANDOFF.md)，需求见 [`docs/REQUIREMENTS.md`](docs/REQUIREMENTS.md)。

## 命令

```bash
cargo build
cargo test --lib
cargo clippy --all-targets -- -D warnings
cargo run -- --no-elevate
```

CI 以 clippy `-D warnings` 卡关。提交前这三项都要能过。

## Commit 前必做：核对立项 pitfalls

准备 commit **之前**，必须打开 [`docs/PITFALLS.md`](docs/PITFALLS.md)，逐条核对本轮改动有没有重新踩中。

1. 读完整份列表，不要只看标题。
2. 本轮动到某条列出的文件或相关逻辑时，那一条视为 **必须过**：不能拆掉防护、不能把测试改成永远绿、不能用「看起来更合理」的回退（例如把 `is_file()` 改回 `exists()`）。
3. 没直接改那些文件，也要扫一眼是否间接触及（解析命令行、拉起卸载器、残留名字匹配、路径保护）。
4. 核对结果写进自己的检查，不要默认「这次无关」。
5. 新发现的、用户已经踩过、根因不直观、以后很容易改回去的问题，**补进 PITFALLS**，不要只写在 commit message 里。

现在已有：

| ID | 一句话 |
| --- | --- |
| P1 | 未加引号的 `UninstallString` 不能把 `C:\Program Files` 这种目录当成 exe |
| P2 | 补位用的隐形占位块要和真卡片一样有 `p_5` + `border_1`，否则同排卡片不等宽 |
| P3 | 卡片徽章要在源头缩短并由卡片裁切，别指望 gpui 的文字省略号 |
| P4 | WMI 方法入参的 `uint32` 要按 `VT_I4` 填，`VT_UI4` 一律 TYPE_MISMATCH |

## 改代码时

- 路径能不能删，只问 `core/safety.rs`。不要在 cleaner / MFT / residuals 里再抄一份黑名单。
- Windows 卸载走官方 `UninstallString`，成功判据是登记项或安装目录没了，不是进程退出码 0。
- 界面文案走 `ui/i18n.rs` 的 `tr_*`，不要在视图里写死中英串。
- 注释只写非显然的约束，不要叙述实现步骤。
