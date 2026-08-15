# QuickCleaner

[English](README.md) | [中文说明](README_zh.md)

> **极速、原生、安全的 Windows 磁盘空间分析与系统深度清理工具**  
> 基于 **Rust + GPUI** 构建，无 WebView / Electron 开销，毫秒级响应与 GPU 加速渲染。

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
- **系统核心保护**：路径规则只有一份事实来源（`core/safety.rs`），守住盘符根、`%SystemRoot%`、`System32`、`WinSxS`、`Program Files`、`ProgramData`、用户主目录与 NTFS 元数据文件——它们可以浏览、可以参与占用分析，但不可删除；删除过程不跟随符号链接与 junction。
- **智能 UAC 提权**：Windows 下启动时自动请求管理员权限，解锁直接读取 MFT 原始卷扇区与深度清理的能力；支持 `--no-elevate` 参数以普通权限启动。

### 5. 现代化原生桌面体验与多语言支持
- **GPUI 高性能原生 UI**：基于 Zed 的 GPUI 渲染引擎，纯 Rust 编写，毫秒级冷启动与 60/120fps 流畅动效。
- **两阶段扫描**：约 1 秒就把系统垃圾类目的结果显示出来，界面立刻可用；最贵的那步（全盘检索项目构建产物）放到后台继续跑，跑完自动并进列表。开发机实测：改前整轮 33 秒，现在 3 秒内出结果。
- **记住你的选择**：首次启动按 Windows 显示语言决定界面语言（中文系统用中文，其余英文），你切过之后写进 `%APPDATA%\QuickCleaner\settings.json`，下次启动照旧。
- **中英文无缝切换**：所有文案——包括状态栏、扫描结果标签、残留来源徽章——都是双语的（`中文 / English`），点侧边栏底部即时切换。扫描结果里的标签自带两种语言，切语言**不会触发重扫**。
- **高质感浅色/深色排版**：无过度装饰与 AI 模板化视觉，规范的主题色彩系统（`Material 3 / Fluent` 融合风格），支持 PerMonitorV2 高 DPI 屏幕自适应。

---

## 架构设计

项目采用严格的自上而下单向分层架构：

```
quick-cleaner/
├── src/
│   ├── main.rs                 # 应用程序主入口与 UAC 提权自重启
│   ├── lib.rs                  # 模块定义
│   ├── bin/
│   │   └── mftscan.rs          # 独立命令行 MFT 验证工具
│   │
│   ├── core/                   # 领域逻辑层（不依赖 GPUI，无系统强耦合）
│   │   ├── i18n.rs             # 核心多语言定义（Language 枚举）
│   │   ├── categories.rs       # 10 大清理类别与扫描规则
│   │   ├── devscan.rs          # 构建产物的发现式扫描（MFT / 遍历双通道）
│   │   ├── scanner.rs          # walkdir + rayon 并行目录扫描
│   │   ├── cleaner.rs          # 清理执行器与原子进度计数
│   │   ├── safety.rs           # 路径安全防护规则（唯一事实来源）
│   │   ├── apps.rs             # 软件模型与残留分析器
│   │   ├── disk.rs             # 磁盘树与选择状态模型
│   │   └── model.rs            # 数据格式化与通用三态模型
│   │
│   ├── platform/               # 操作系统适配层（由 platform_contract! 编译期约束）
│   │   ├── mod.rs              # 统一平台门面契约
│   │   ├── windows/            # Windows 原生实现（$MFT、注册表、进程、UAC 等）
│   │   └── macos/              # macOS 基础跨平台适配
│   │
│   └── ui/                     # GPUI 视图与交互层
│       ├── mod.rs              # Root 状态机与派生缓存管理
│       ├── i18n.rs             # UI 视图层多语言映射字典
│       ├── theme.rs            # 设计系统色彩与尺寸 Token
│       ├── components/         # 通用组件（按钮、卡片、滚动条、弹窗等）
│       └── views/              # 页面视图（dashboard / junk / apps / disk）
```

---

## 🚀 构建与运行

### 前置要求
- **Rust** 1.75 或更高版本（推荐使用 `stable-x86_64-pc-windows-msvc` 工具链）
- **Windows 10 / 11**（推荐以管理员权限运行以获得完整的 `$MFT` 读取支持）

### 本地编译与调试

```bash
# 1. 克隆仓库
git clone https://github.com/meimingqi222/quick-cleaner.git
cd quick-cleaner

# 2. 运行单元测试（覆盖算法、MFT 解析、安全防护等）
cargo test

# 3. 启动开发版
cargo run

# 4. 构建发布优化版
cargo build --release
```

编译生成的可执行文件位于 `target/release/quick-cleaner.exe`。

### 独立命令行工具

项目附带了一个独立的命令行 $MFT 扫描验证工具，用于快速排查磁盘解析性能与准确性：

```bash
# 扫描 C 盘并输出前 20 个最大文件
cargo run --bin mftscan -- C 20
```

---

## 质量与测试

项目拥有完善的自动化测试套件（102 项单元测试，CI 中与 `cargo clippy --all-targets -- -D warnings` 一起卡关），覆盖：
- NTFS `$MFT` 记录解析、fixup 扇区尾校验、数据片段重组与树构建；
- 路径与注册表安全规则、系统保护边界校验；
- 卸载残留模糊匹配与置信度打分算法；
- 目录多线程并行扫描与清理进度原子计算。

运行全量测试：
```bash
cargo test
```

---

## 开源协议

本项目采用 [MIT License](LICENSE) 开源。
