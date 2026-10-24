# Agent Note: 系统托盘宿主与内容边界

Status: implemented

## Problem

系统托盘的原生事件、窗口重入和平台驻留约束容易在 UI 改版时被误改。
业务采样、布局和窗口句柄混在一起，让只调整内容的开发者也必须理解窗口生命周期。

## Decision

`../gpui-app-kit/crates/app-host/src/tray.rs` 提供两个产品共用的 TrayHost；
`src/ui/tray_host.rs` 只适配图标和本地化菜单。宿主独占图标注册接线、
窗口定位、切换、焦点、Esc、失活、延迟销毁以及每次打开的会话。
平台层只实现原生状态项；业务模型和宿主均不放进通用视觉 components。
它与 `src/ui/components/popover.rs` 的窗口内浮层不是同一类组件。

`src/ui/tray_overview.rs` 持有采样任务和动作路由；控制器只弱引用当前模型。
`src/ui/views/tray_popover.rs` 只读模型、观察通知、布局并发送枚举事件。
关闭按钮用 session.dismiss(cx)，业务按钮用 session.emit(action, cx)。
关闭策略在 TrayAction 实现中声明，宿主在事件分发结束后先关窗，再调用业务处理器。
旧 session 的延迟关闭和动作不得操作新窗口或新业务模型。

### 复用与修改

仅改内容时编辑视图及本地化文案，保留构造函数传入的模型和 session；
不要接触宿主、原生托盘或主窗口生命周期。新增维护操作时在业务模块添加动作和处理，
内容仅发该动作。不要在 render 中读磁盘、发命令或启动定时器。

另一种托盘内容只需：定义实现 TrayAction 的动作枚举，创建任意 GPUI Render 视图，
在应用启动时构造 TrayHost::new(width, height)，调用 install 传入应用 TrayConfig、
ActionHandler 和内容工厂。工厂接收独立 TraySession、Window 和 App，返回 Entity<V>。
后台业务需要停止采样时使用 session.lifetime() 的只读存活检查。
每个宿主同时只允许一个窗口；mount 是公开的宿主集成/测试接缝，不是内容 API。

`scripts/check-ui-components.py` 同时检查主内容文件与拆分子目录，禁止窗口句柄、
原生托盘、任务和命令启动、直接修改模型等常见越界路径，测试 fixture 除外。
守卫是防止误改的静态检查，不是 Rust 权限沙箱；代码评审仍需确认依赖方向。

## Alternatives considered

只拆文件但继续向内容暴露关闭回调和窗口句柄，仍然允许事件重入。
让 UI 自行 defer 把正确性责任留给每个按钮，无法保证下一次改版。
仅在本仓库保留宿主无法保护 Usage Metrics 的另一份实现。当前已有两个真实消费者，
因此提取独立 crate；不引入插件注册框架。

## Consequences

现有产品是实际迁移调用方；测试还用不依赖空间透镜的简单内容验证同一宿主。
原有硬件可见性、列表滚动、实时刷新、主界面与显式扫描行为保持。
后续改变交互生命周期需修改宿主并跑宿主测试；内容改版只需内容与布局回归。
Windows 代码保留原有驻留与菜单处理，本次 macOS 开发不代表已做 Windows 实机验收。

## Verification

- `src/ui/tray_overview.rs::tray_refresh_updates_repeatedly_and_stops_on_close`
- `scripts/test_ui_components.py::test_tray_content_rejects_window_and_business_control`
- `scripts/test_ui_components.py::test_tray_content_scan_covers_split_content_modules`

宿主侧的 open/close/重开与旧会话隔离原本绑定兄弟仓库 gpui-app-kit 的测试；Go 版本改用
MyGo，不再消费该库，跨仓绑定不再是本仓库的可校验目标。

Proved: 添加边界守卫前，内容越界测试失败于 cx.open_window 未被拒绝，见
`docs/agent-notes-evidence/2026-10-15-tray-boundary-red.log`；实现后同 focused 及完整脚本测试
通过，见 `docs/agent-notes-evidence/2026-10-15-tray-boundary-green.log`。

Validation: 最终 cargo fmt --check、cargo build、cargo clippy --all-targets -- -D warnings
通过；宿主环境完整 cargo test --lib 为 840 passed、12 ignored、0 failed。
13 项 UI Python 检查测试、组件边界、本地化、strict-anchors 与 diff whitespace 检查通过；
PITFALLS 指针校验无错误，保留八项既有缺少 Note 指针警告。
macOS 用本轮二进制独立启动验证，主窗口正常显示。原生自动化工具未能直接操作菜单栏
状态项，因此真实菜单栏点击尚待人工验收；宿主 toggle、重开、旧会话隔离、Esc、失焦、
硬件可见性和后台刷新通过 GPUI 测试。Windows 未做实机验证。
