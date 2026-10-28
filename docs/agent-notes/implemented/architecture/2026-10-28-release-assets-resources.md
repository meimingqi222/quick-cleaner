# Agent Note: 更新资产和 Windows 资源是发布兼容契约

Status: implemented

## Problem

更新器依赖固定的资产名和归档内部载荷；Windows 可执行文件依赖生成的图标、版本和 manifest 资源。改变打包方式时忽略这些契约会破坏更新或资源显示。

## Decision

`scripts/package-windows.ps1` 和 `.github/workflows/release.yml` 保持 Windows 资产名，macOS 发布工作流生成 universal、aarch64 和 x86_64 资产；macOS 包装由 `scripts/package-macos.sh` 完成。`tools/winres` 从产品版本生成 Windows 资源，CI 和 release 检查资源是否同步。更新器选择名与发布名和包内载荷必须一致。

## Alternatives considered

只检查打包成功不能证明旧更新器能找到新资产；只看生成的 exe 文件也不能证明 manifest 和版本信息完整。

## Consequences

更改名称、归档结构、bundle id 或资源生成方式需要同时评估已安装更新器的兼容性。

## Verification

- `tools/winres/main_test.go::TestResourcesMatchRustBuild` 检验生成资源和 manifest 内容。
- `tools/winres/main_test.go::TestVersionComesFromTheProductConstant` 检验版本解析。
- `src/core/updater.rs::mac_fallback_never_crosses_architecture` 和 `src/core/updater.rs::intel_mac_does_not_pick_aarch64_when_universal_missing` 检验 Rust 更新器架构选择。

本次核对了工作流、打包脚本和 Rust 更新器名称；未实际构建发布包，也未运行 Rust 测试。上述资源测试不验证全部归档结构或线上发布资产。
