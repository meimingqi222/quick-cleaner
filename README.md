# QuickCleaner

受 Clean My Mac 启发的 Windows 系统清理工具,使用 Rust + GPUI 原生 UI 构建。

## 功能

- 扫描常见安全目录并统计可清理空间
- 清理类别:系统临时文件 / 用户临时文件 / 浏览器缓存 / 包管理缓存 / 日志与崩溃转储 / 回收站 / 缩略图缓存
- 清理模式:默认移入回收站(可恢复),可选永久删除
- Material 3 浅色主题界面(类似 Clean My Mac 的信息密度与卡片布局)

## 技术栈

- [GPUI](https://github.com/zed-industries/zed) 0.2 — Zed 的高性能 Rust 原生 UI 框架
- walkdir / rayon — 目录并行扫描
- winapi SHFileOperation — 移入回收站

## 构建与运行

```bash
cargo run     # 调试
cargo build --release   # 发布
```

## 项目结构

```
src/
├── main.rs        # GPUI 应用入口
├── core/          # 领域层：扫描 / 清理 / 安全规则 / 软件与磁盘模型
├── platform/      # 适配层：Windows(NTFS·注册表·UAC) 与 macOS
└── ui/            # GPUI 视图层：概览 / 智能清理 / 软件管理 / 磁盘透镜
```

依赖方向严格自上而下 `ui → core → platform`。详细分层说明与
「改动前必读」的渲染缓存约定见 [`docs/HANDOFF.md`](docs/HANDOFF.md)。
