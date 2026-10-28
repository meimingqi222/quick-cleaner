# Agent Note: 本地格式和 Clippy 应与 CI 工具链对齐

Status: implemented

## Problem

CI 跟随 Rust stable，旧版本的 rustfmt 或 Clippy 本地通过不保证 CI 通过；新版可能调整格式或增加 lint。

## Decision

`.github/workflows/ci.yml` 使用 `dtolnay/rust-toolchain@stable`。提交前应确认实际 CI stable 版本，并用相同版本的 rustfmt 和 Clippy 检查；根据具体 lint 修正代码，不以禁用 lint 代替修复。

## Alternatives considered

长期假设某个固定版本等于 stable，会在 stable 更新后重新出现本地和 CI 不一致。

## Consequences

工具链版本会变化，历史版本号只能作为过去的例子；本地通过不能替代实际 CI 结果。

## Verification

本次核对了 CI 配置。本机 `rustc --version` 为 1.95.0，环境没有 `rustup`；未查询或安装当前 CI 的 stable，不能声称二者一致。
