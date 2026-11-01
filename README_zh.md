# QuickCleaner

[English](README.md) | [中文说明](README_zh.md)

QuickCleaner 使用 **Go + MyGo** 编写。原 Rust + GPUI 实现已于 2026-11-01 退役并删除，
Go 版保留了原有界面与交互。主程序入口为 `go run ./cmd/quick-cleaner --no-elevate`，
无破坏性操作的界面预览入口为 `go run ./cmd/ui-gallery`。原生平台验收仍在进行，
详见[迁移状态](GO_MIGRATION.md)。

> **原生、安全的 Windows / macOS 磁盘空间分析与系统清理工具**  
> 基于 **Go + MyGo** 构建，无 WebView / Electron 开销。

---

## 核心特性

### 1. NTFS $MFT 极速磁盘分析 (Disk Lens)
- **毫秒级全盘索引**：直接解析 NTFS 文件系统主文件表（`$MFT`），无需耗时的传统递归 I/O 遍历，3~5 秒内完成全盘数百万文件索引（速度媲美 WizTree / Everything）。
- **层级空间占比透镜**：直观的环形图与目录树下钻，迅速定位占用磁盘的大文件与冗余深层目录。
- **多卷/多分区支持**：支持快速切换分析 C 盘、D 盘、E 盘等多个 NTFS 固定分区，自动识别可用容量与占用分布。

### 2. CleanFlow 智能垃圾清理
- **全面覆盖十大类目**：
  - 系统临时文件 / 用户临时文件 / 缩略图缓存
  - 主流浏览器缓存（Chrome、Edge、Firefox 等）
  - 开发者包管理缓存（Cargo、npm、pnpm、Yarn、pip、Gradle、Go build 等）
  - 日志与系统崩溃转储（Minidump、Windows Error Reporting）
  - 回收站安全清空（调用系统 `SHEmptyRecycleBinW`，保留各卷元数据结构）
  - **AI 助手与 Agent 缓存**（Claude、Cursor、Antigravity 等会话与模型缓存）
  - **项目构建产物与依赖**（代码目录下的 `node_modules`、`target`、`.venv`、`bin/obj`，默认安全不预选）
- **清空内容且保留目录**：清理时清除目录内冗余内容，保留关键系统目录节点，确保系统与应用平稳运行。

### 3. 软件管理与卸载残留深度清理
- **已安装软件全览**：从 64 位/32 位注册表多源枚举，快速查看软件大小、安装日期与使用频次。
- **全生命周期残留追踪**：
  - **卸载前采集**：捕获应用关联的 ProgramData、AppData、注册表项、服务与启动项痕迹；
  - **执行官方卸载**：平滑调用官方卸载程序并监控其退出；
  - **卸载后复核**：比对残留项，仅将官方卸载程序遗留的孤儿配置与缓存列出供用户一键彻底清除。

### 4. 严格安全防护体系
- **删除一律是永久删除，不进回收站。** 这是有意的产品决定，不是遗漏：本工具会通过 UAC 自提权，而当管理员账户与当前登录用户不是同一个时，`FOF_ALLOWUNDO` 会把文件丢进**管理员自己的**回收站，用户在自己那儿根本找不到；何况把几十 GB 缓存搬进回收站，占用一个字节都没少。删了就是删了，所以每个确认框都明写这一点。
- **动手前有四道关口**：扫描 → 预览具体路径 → 逐项勾选 → 确认。开发相关类目（AI 助手缓存、构建产物、agent worktree）一律不默认勾选。
- **系统核心保护**：路径规则只有一份事实来源（`internal/core/safety.go`），守住盘符根、`%SystemRoot%`、`System32`、`WinSxS`、`Program Files`、`ProgramData`、用户主目录与 NTFS 元数据文件——它们可以浏览、可以参与占用分析，但不可删除；删除过程不跟随符号链接与 junction。
- **智能 UAC 提权**：Windows 下启动时自动请求管理员权限，解锁直接读取 MFT 原始卷扇区与深度清理的能力；支持 `--no-elevate` 参数以普通权限启动。

### 5. 现代化原生桌面体验与多语言支持
- **原生 UI**：基于 MyGo 即时模式 UI 工具包，不依赖 WebView 或 Electron 运行时。
- **两阶段扫描**：约 1 秒就把系统垃圾类目的结果显示出来，界面立刻可用；最贵的那步（全盘检索项目构建产物）放到后台继续跑，跑完自动并进列表。开发机实测：改前整轮 33 秒，现在 3 秒内出结果。
- **记住你的选择**：首次启动按 Windows 显示语言决定界面语言（中文系统用中文，其余英文），你切过之后写进 `%APPDATA%\QuickCleaner\settings.json`，下次启动照旧。
- **中英文无缝切换**：所有文案——包括状态栏、扫描结果标签、残留来源徽章——都是双语的（`中文 / English`），点侧边栏底部即时切换。扫描结果里的标签自带两种语言，切语言**不会触发重扫**。
- **高质感浅色/深色排版**：无过度装饰与 AI 模板化视觉，规范的主题色彩系统（`Material 3 / Fluent` 融合风格），支持 PerMonitorV2 高 DPI 屏幕自适应。

---

## 架构设计

项目采用严格的单向分层架构 `ui → core → platform`：

```
quick-cleaner/
├── cmd/
│   ├── quick-cleaner/          # 产品入口与 UAC 提权自重启
│   └── ui-gallery/             # 无破坏性操作的界面预览
├── internal/
│   ├── app/                    # 控制层：状态、后台任务与操作
│   ├── core/                   # 领域逻辑：扫描、清理、safety.go、规则
│   ├── platform/               # 系统适配层（Windows $MFT、注册表、进程；macOS）
│   └── ui/                     # MyGo 视图、主题与多语言
├── rules/                      # 声明式清理规则（TOML）
└── tools/                      # indexbench、winres、regression-notes
```

---

## 🚀 构建与运行

### 前置要求
- **Go** 1.27.1 或更高版本（不需要 cgo）
- **Windows 10 / 11**（以管理员权限运行可直接读取 `$MFT`）或 **macOS**

### 构建、测试与打包

```bash
git clone https://github.com/meimingqi222/quick-cleaner.git
cd quick-cleaner

go test ./...
go run ./cmd/quick-cleaner --no-elevate      # 不是演练模式：确认后的操作照样会改动文件
go run ./tools/winres                        # 修改 internal/app 的 Version 后重新生成 Windows 资源
pwsh ./scripts/package-windows.ps1           # 生成 dist/quick-cleaner-windows-x86_64.zip
./scripts/package-macos.sh dist universal    # 生成 dist/QuickCleaner.app（需在 macOS 上）
```

---

## 质量与测试

`.github/workflows/go.yml` 在 Windows、macOS 和 Linux 上运行 `gofmt`、`go vet`、
`go test -race` 和 `go build`，并检查 Windows 资源。测试覆盖 NTFS `$MFT` 解析、
路径与注册表安全规则、残留匹配、规则校验和清理进度统计。

```bash
go test -race ./...
```

---

## 开源协议

本项目采用 [MIT License](LICENSE) 开源。
