# Agent Note: GPUI 基础交互的跨应用所有权

Status: implemented

## Problem

QuickCleaner 与 Usage Metrics 各自维护托盘、定位和关窗状态；内容改版可能重新引入窗口重入和旧事件误操作。
单个应用内抽文件不能保护另一个消费者。文本和滚动机制也容易形成多份实现。

## Decision

独立兄弟仓库 gpui-app-kit 的 gpui-app-host 拥有托盘、桌面生命周期与会话；gpui-app-widgets 拥有文本编辑/宿主绑定、滚动与泛型浮层。
两个应用通过固定 Git 提交实际调用；根 manifest 的 GPUI patch 同时指向该库的相同提交，保留 macOS 激活死锁补丁。
不依赖消费者中的 patch 自动向下传播；切 Git 时两个库与 root GPUI patch 必须固定同一 revision。

应用保留业务模型、采样/请求、刷新频率、产品图标、翻译、主题、内容布局以及浮层业务 ID/关闭回调。
QuickCleaner 的宿主、输入、滚动与浮层模块变为适配或重导出；平台托盘和旧 vendor 已迁走。
Usage Metrics 的 tray.rs 只渲染和发事件，tray_model.rs 管业务，tray_controller.rs 管动作路由。
公共 tools/check_content.py 与应用静态守卫阻止内容重新持有窗口、开任务或直接修改模型。

## Alternatives considered

只复制工具代码会让修复继续分叉。让内容拿关闭回调仍把 defer 的正确性留给每个按钮。
统一产品页面和品牌样式会把业务带进库。未经两平台原生 IME 验收就替换 QuickCleaner 的输入架构不符合现有约束。

## Consequences

现有宿主、输入、滚动测试迁到其新所有者；应用布局、硬件可见性、采样和关闭行为测试保留。
应用 notes 的迁移测试引用指向兄弟库；CI 检出库并合并两侧测试列表。
公共库已发布到 meimingqi222/gpui-app-kit 私有仓库；两个 crate 与根 GPUI patch 固定 3cf5e6cb3c171769e91674d3d5b0b3a7115b9c56。CI 从 manifest 读取提交；跨仓库读取需要 GPUI_APP_KIT_TOKEN 专用只读凭据。
完整原生点击、Windows DPI/多屏、Explorer 重启和原生 IME 不由模拟测试证明。

## Verification

- `../gpui-app-kit/crates/app-host/src/tray.rs::tray_host_old_session_cannot_touch_reopened_window`
- `../gpui-app-kit/crates/app-host/src/tray.rs::tray_host_programmatic_open_needs_no_window_event`
- `../gpui-app-kit/crates/ui-widgets/src/input.rs::bindings_isolate_fields_and_commit_composition_once`
- `../gpui-app-kit/crates/ui-widgets/src/scroll_area.rs::virtualized_lists_drag_outside_and_keep_independent_offsets`
- `src/ui/shell/overlay_stack.rs::nested_close_is_children_first_and_render_order_matches_escape`
- `src/ui/views/tray_popover.rs::tray_hardware_stays_visible_with_many_volumes`

Validation: 公共库 workspace 测试、严格 clippy、格式检查与内容守卫通过；Usage Metrics 完整 cargo test 为 201 passed，原生窗口 smoke 通过 keep-open、close-before-action、dismiss 与 Escape。
Usage Metrics 原有 local_sources.rs 有四条 clippy 告警；沿用其原 CI 策略。其旧 note 2026-10-03-antigravity-windows-login.md 引用的 antigravity_windows_client_pairs_config_references 在本轮前的源码中已缺失，本轮未扩展到该业务模块。
QuickCleaner 首轮全库 813 passed / 1 failed / 12 ignored；失败的 clean_path_allows_stale_live_database_shapes 在同一测试二进制及 cargo focused 下均复跑通过。清理源码与重构前备份逐字节相同；进一步串行完整复核单独记录。
原生 smoke 不代表菜单栏真实点击、所有多屏/DPI和Windows原生验收。
公共库已提交并推送；消费者提交状态由各自 Git 历史记录。原始重构备份保留在本地临时目录。

Final validation: QuickCleaner 同一完整库测试二进制串行复核 814 passed / 0 failed / 12 ignored。两产品 cargo build 均通过；QC 与公共库严格 clippy 通过；三个仓库格式检查通过。公共库最终 43 项测试和内容守卫 4 项测试通过。

迁移到公共库的 anchor 已与实际测试列表匹配。全仓库双平台 anchor 的最终并集检查仍需 Windows CI；macOS 单平台列表无法覆盖原有 Windows 专用用例。

Publication verification: 两个应用从 GitHub 实际拉取同一固定提交，Cargo.lock 来源与解析结果一致；全目标 cargo check 与 cargo build 均通过。待提交测试证据中的本机 Home 路径已替换为 <HOME>，保留原始断言和测试结果。

Pre-commit pitfalls review: 完整核对 P1–P64（P10–P13 空缺）。直接涉及 P2/P3 盒度量、P30 GPUI 激活补丁与 runtime_shaders、P34 首帧后台采样、P51–P54 浮层策略、P58–P61 输入与本地化，以及 P63/P64 托盘与轮询；相关防护和测试保留或迁入公共库。P55 工具链差异仍由远端 CI 复核。卸载命令、残留匹配、路径保护、删除、索引批处理及其余平台安全约束未改动。
