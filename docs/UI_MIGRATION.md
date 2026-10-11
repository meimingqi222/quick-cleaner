# UI 组件重构：批 0 调查基线

批 1 边界调整已完成：`sidebar`/`topbar` 已搬到 `shell/`，`dialogs`/`updater_dialog`/`search_box`/`progress` 中依赖 Root 的部分已搬到 `adapters/`，`checkbox` 改收组件自有 `CheckState`；`components/` 不再引用 Root/core/platform（守卫 R1 为 0）。

本文只记录 `src/ui` 当前行为，不含任何改动。所有结论来自读代码。引用写成 `文件:行号`，路径相对 `src/ui/`。
没有运行应用确认的结论标注「代码推断」；没读到或不能确定的写「未确认」。

目标层的含义（本文的约定，仓库里还没有 `adapters/`、`shell/` 目录，目前只有 `components/`）：

- `components`：只依赖 `ui::theme`、`gpui`，不引用 `Root`、`core`、`platform`。
- `adapters`：把 `Root` 或 `core` 的数据和回调接到 components 上。
- `shell`：直接持有 `Root`，负责整窗口的组合与浮层挂载顺序。

## 1. 迁移清单

状态列全部为「未迁移」。

### 1.1 components/ 现有文件

| 现有实现 | 目标组件 | 目标层 | 依赖 Root/core? | 状态 |
| --- | --- | --- | --- | --- |
| `components/buttons.rs::primary_button/danger_button/ghost_button/small_button` (:7/:27/:47/:68) | Button | components | 否（只依赖 theme） | 未迁移 |
| 无独立 IconButton；候选是搜索框清除钮 `search_box.rs:140-155`、行尾「···」`views/apps_components.rs:245-252` | IconButton | components | 否 | 未迁移（其他候选未确认） |
| `components/controls.rs::badge` (:62) | Badge | components | 否 | 未迁移 |
| `components/controls.rs::checkbox` (:8) | 不在目标族，需单列（Checkbox） | components | 是：`controls.rs:3` 引用 `core::model::Check` | 已解耦：改收组件自有 `CheckState`，`From<Check>` 在 `adapters/mod.rs`；Button 之外其余未迁移 |
| `components/controls.rs::page_heading` (:41) | 不在目标族，需单列（PageHeading） | components | 否 | 未迁移 |
| `components/controls.rs::loading_spinner/loading_state_view` (:75/:138) | LoadingState | components | 否 | 未迁移 |
| `components/cards.rs::card` (:7) | Card | components | 否 | 未迁移 |
| `components/tooltip.rs::text_tooltip/path_tooltip` (:50/:56) | 不在目标族（Tooltip） | components | 否 | 未迁移 |
| `components/donut.rs::render_donut` (:12) | 不在目标族，归 Meter/Progress 近亲 | components | 否 | 未迁移 |
| `components/icons.rs`（`use crate::ui::theme::*` 是唯一内部依赖，:4） | 不在目标族（Icon 词汇表） | components | 否 | 未迁移 |
| `components/scroll.rs::scroll_metrics/scrollbar/drag_to_offset/drag_capture` (:40/:102/:81/:141) | ScrollArea / VirtualList / TableShell 的滚动条部分 | components | 否；拖拽状态在 `Root` 子状态里（见 4 节） | 未迁移 |
| `components/popover.rs::PopoverState/popover_surface/popover_list/popover_row/popover_overlay` (:14/:145/:183/:198/:225) | Overlay + SuggestionList | components | 否（只用 theme；`docs/UI_POPOVER.md:3-4` 也这样声明） | 未迁移 |
| `components/search_box.rs::search_box` (:51) | TextInput / SearchInput | adapters（批 3 再拆：绘制与命中留 components） | 是：回调签名写死 `crate::ui::Root` (:53-57)；内部调用 `Root` 上的 `text_input_*_target`、`poke_cursor_blink` (:188-250)；`target` 是 `text_input::InputTarget` | 已搬到 adapters（`adapters/search_box.rs`，整体，待批 3 拆分） |
| `components/sidebar.rs::render_sidebar` (:71)、`View` 枚举 (:12)、`render_version_row` (:329) | 导航项用 ListRow，版本行用 Badge | shell | 是：`Root` (:7)、`core::updater` (:330)、`platform::is_packaged_install` (:384)、`core::i18n` (:388)。`View` 被 `Root` 与 `topbar.rs:6` 共用 | 已搬到 shell（`shell/sidebar.rs`）；批1第二阶段：导航项已迁移 ListRow（`RowVariant::Soft`；右键不再触发切换视图）；版本行是整行可点，不是 Badge，保留；语言切换（lang-zh-btn/lang-en-btn）独立实现（原因：槽 CARD 底 + 0.5 描边、选中 PRIMARY_FIXED 无阴影的分段外观全仓库仅此一处） |
| `components/topbar.rs::render_top_bar` (:13) | 外壳在 shell；Status 子页签 (:110-200) 用 SegmentedControl；重扫钮用 Button | shell | 是：`Root` (:10)、`state::StatusTab` (:8)、`core::model` (:3) | 已搬到 shell（`shell/topbar.rs`）；已迁移：SegmentedControl（状态子页签，进程数走 `.badge`）、Button（重扫钮）；缺口：Live/暂停胶囊、freed 胶囊、权限状态胶囊仍是手写 |
| `components/dialogs.rs::render_confirm_dialog` (:55) | Dialog（内容）+ ActionBar（底部按钮） | 外壳 components，内容 adapters | 是：`Root` (:12)、`core::apps`/`core::dev_env`/`core::status` (:3,:26,:28,:38) | 已搬到 adapters（`adapters/dialogs.rs`，整体）；批1第二阶段：已迁移 Button |
| `components/dialogs.rs::render_residual_modal` (:245) | Dialog + ListRow + ActionBar + Badge（`render_occupancy_banner` :174） | adapters | 是：`Root`、`core::apps::Residual*` | 已搬到 adapters（`adapters/dialogs.rs`）；批1第二阶段：已迁移 Button、Badge（置信度/来源）；条目行已迁移 ListRow（`RowInset::X3Y2`） |
| `components/dialogs.rs::render_fda_onboarding_modal` (:663) | Dialog | adapters | 是：`Root`；仅 macOS 有用 | 已搬到 adapters（`adapters/dialogs.rs`）；批1第二阶段：已迁移 Button |
| `components/progress.rs::render_scan_line` (:17) | Meter/Progress（不定进度条） | components | 否 | 未迁移 |
| `components/progress.rs::render_progress_bar` (:195) | ActionBar + Meter | adapters | 是：`Root`、`core::model` | 已搬到 adapters（`adapters/progress.rs`）；批1第二阶段：已迁移 Meter、Button |
| `components/progress.rs::render_uninstall_progress` (:54) | Meter/Progress + Card | adapters | 是：`Root`、`UninstallPhase` (:9) | 已搬到 adapters（`adapters/progress.rs`）；批1第二阶段：已迁移 Meter、Spinner |
| `updater_dialog.rs::render_update_dialog` (:12) | Dialog + ActionBar + Button | adapters | 是：`Root`、`core::updater::UpdateStatus` | 已搬到 adapters（`adapters/updater_dialog.rs`）；批1第二阶段：已迁移 Button |

### 1.2 views/ 与外壳

| 现有实现 | 目标组件 | 目标层 | 状态 |
| --- | --- | --- | --- |
| `mod.rs::Root::render` (:855-980)：页面选择、扫描线、底栏、浮层追加顺序 | 浮层栈 | shell | 未迁移 |
| `views/dashboard.rs::render_dashboard_view` (:18)：扫描环按钮 (:96)、四张快捷卡 (:190/:231/:275/:319)、`goto-junk` 主按钮 (:386) | Card、Button、Meter（环） | adapters | 已迁移：Button（goto-junk）；缺口：扫描环按钮与快捷卡为整卡点击，无内部按钮，环装饰/Meter 环不在本轮 |
| `views/apps.rs::render_apps_view` (:17)：桌面/开发环境页签 (:83/:117)、统计卡 (:160 起)、预设筛选 (:313)、列头排序 (:449)、孤儿扫描钮 (:50) | SegmentedControl、Card、Badge、TableShell、Button | adapters | 已迁移：Button（孤儿扫描钮）、SegmentedControl Chip（预设筛选）、Badge（过滤计数）、EmptyState/LoadingState；桌面/开发环境页签（tab-desktop-btn/tab-dev-btn）独立实现（原因：SURF_HIGH 无描边槽、px_3 py5 rounded 6、选中字色 TEXT 的外观全仓库仅此一处）；统计卡图标底板是 icon_badge，保留 |
| `views/apps.rs::render_apps_context_menu` (:645) | Menu（Overlay） | adapters | 未迁移 |
| `views/apps_components.rs::render_app_row` (:15) | ListRow | adapters | 已迁移：ListRow（右键走 on_right_click）、Button::small、IconButton::Tile |
| `views/apps_components.rs::render_apps_list_card` (:282) | VirtualList + ScrollArea + TableShell | adapters | 未迁移 |
| `views/dev_envs.rs::render_dev_environments_view` (:23)、`render_master_panel` (:250)（页签 :287）、`render_master_row` (:485)、`render_detail_panel` (:649)、`render_package_row` (:1145)、`render_dev_bottom_bar` (:1301)、`metric_pill` (:225)、`badge_chip` (:1420)、`detail_metric_box` (:980)、`section_sub_header` (:447) | SegmentedControl、ListRow、ActionBar、Badge、Card、ScrollArea | adapters | 已迁移：Badge（metric_pill、badge_chip、版本徽章）、ListRow（主列表行、包行）、ActionBar capsule Inline、Button、EmptyState/LoadingState；过滤页签（SegmentedControl Chip + `ChipLook`）、主列表行右键选中（`on_right_click`）、右栏空状态（`EmptyState::flush`）；缺口：无；detail_metric_box、section_sub_header 无对应组件，保留 |
| `views/disk.rs::render_disk_view` (:22)、`render_disk_lens_panes` (:216) | Card、LoadingState、EmptyState | adapters | 已迁移：LoadingState、Button（start-mft-scan-btn）；缺口：错误态与卷选择卡仍是手写（无对应 EmptyState 变体/卡片） |
| `views/disk_left.rs::render_left_lens_pane` (:17)、`render_breadcrumbs` (:214)、`render_tab_switch` (:338) | Card、SegmentedControl | adapters | 已迁移：SegmentedControl（`SegmentStyle::Plain`，逐像素还原）、面包屑（Button::Link） |
| `views/disk_right.rs::render_right_browser_pane` (:63)、`render_lens_row` (:465)、`render_cross_folder_badge` (:23)、`render_disk_clean_bar` (:529) | TableShell、ListRow、Badge、ActionBar | adapters | 已迁移：Badge、Button、ListRow、ActionBar；行内「›」/「删除」（Button::Link）；缺口：表头无 TableShell |
| `views/disk_breakdown.rs::render_proportion_bar` (:251)、`views/disk_components.rs::render_breakdown_row` (:18) | Meter、ListRow | adapters | 已迁移：ProportionBar、Badge、ListRow；缺口：无 |
| `views/disk_volume.rs::render_volume_selector_button` (:31)、`render_disk_volume_dropdown` (:87) | Select（触发器 + Overlay/Menu） | adapters | 下拉项已迁移 ListRow（`RowVariant::Soft` + `RowInset::X3Y2`）；触发器未迁移（批 2）：有展开态配色 |
| `views/junk.rs::render_junk_view` (:461)、`render_category_items` (:71)、`item_row` (:183)、`render_selection_toolbar` (:307)、`render_exclusions_panel` (:391)、`render_clean_bar` (:833)、`right_cell` (:18) | Card、VirtualList、ListRow、SegmentedControl（批量动作）、ActionBar | adapters | 部分迁移：已换 ActionBar(capsule)、Button(danger/small)、Badge、LoadingState、条目行 ListRow；条目行「排除」与排除面板「移除」已迁移 Button::Link（Slim）；批量动作用 Button::small 而非 SegmentedControl（动作非互斥选择）；`failed-details-toggle` 纯文字切换保留 |
| `views/search.rs::render_search_view` (:23)、分组/排序钮 (:85/:174/:199/:224)、`render_search_box` (:379)、`render_results_card` (:416)、`render_result_row` (:497) | SegmentedControl、SearchInput、VirtualList、TableShell、ListRow | adapters | 已迁移：LoadingState、EmptyState、Badge（状态签）、ListRow（结果行，双击用 `on_double_press`，保持按下即触发）、分组钮（ToggleChip）；缺口：表头排序钮；打开钮（open-btn-N，PRIMARY@0.08 底、hover 实心主色）与定位钮（reveal-btn-N，CARD 底、hover PRIMARY_FIXED）独立实现（原因：两者各只此一处，配色不与任何现有 Button 变体重合） |
| `views/status.rs`：`status_card` (:79)、`card_rows` (:96)、`card_header` (:128)、`header_chip` (:162)、`meter/meter_colored` (:200/:205)、`card_footer` (:223)、各 `render_*_card` (:371-:945)、`gpu_switch_buttons` (:605)、`fan_mode_buttons` (:820)、`render_ports_badge` (:1091) | Card、Badge、Meter、SegmentedControl | adapters（`status_card` 与 `card_rows` 受 PITFALLS P2/P3 约束，迁移时要保留占位块规则） | 已迁移：Badge（`header_chip`、端口徽章）、Meter（内存/磁盘/电池/进程 CPU 迷你条）、SegmentedControl（GPU 切换、风扇档位）；`status_card`/`card_rows` 原样保留；风扇卡「移除守护进程」（fan-helper-remove）独立实现（原因：全仓库仅此一处，禁用态只淡化字色而非整体透明度，悬停同时变底、描边、字色） |
| `views/status.rs::render_process_table` (:2443)、`render_process_row` (:1173)、`sort_header` (:1137)、过滤页签 (:2547-2600) | TableShell + VirtualList + ListRow、SegmentedControl | adapters | 部分迁移：行内端口徽章、CPU 迷你条已换 Badge/Meter，行结构、uniform_list 不动；过滤页签已迁移 SegmentedControl Chip（`ChipLook`：Md 圆角、px_2p5、0.7 描边、悬停变字色） |
| `views/status.rs::render_proc_detail_drawer` (:1548) | 侧栏面板（Card）；不是浮层 | adapters | 未迁移；抽屉内按钮独立实现（原因：退出 / 强制退出两钮 flex_1 但配色各只一处；打开位置 28x28、关闭 22x22 方钮与复制命令小钮（图标 + 字、随复制态变色）也各只一处） |
| `views/status.rs::render_status_search_popover` (:2276)、`search_candidates` (:2136)、`SearchCandidate` (:2109) | SuggestionList（Overlay） | adapters | 未迁移 |
| `views/declutter/mod.rs::render_declutter_view` (:171) | 页面壳 | adapters | 未迁移 |
| `views/declutter/common.rs::render_unified_nav_header` (:13)、`render_empty_state_card` (:97)、`render_declutter_action_bar` (:136) | SegmentedControl/Tabs、EmptyState、ActionBar | adapters | 部分迁移：EmptyState、ActionBar(strip)；底栏「取消全选」/「清理所选」已迁移 Button（`ButtonSize::Block/BlockWide` + `.hover_style` / `.inactive_opacity`）；导航头页签（unified-subtab-N）独立实现（原因：SURF 槽 + rounded_full 主色实心的页签外观全仓库仅此一处） |
| `views/declutter/overview.rs::render_overview_tab` (:14) | Card | adapters | 部分迁移：状态/扫描/审阅/选择胶囊换 Badge；Card 未动 |
| `views/declutter/downloads.rs::render_downloads_tab` (:17)、`large_files.rs::render_large_files_tab` (:18)、`duplicates.rs::render_duplicates_tab` (:16)、`photos.rs::render_similar_photos_tab` (:17) | ListRow、TableShell、ScrollArea | adapters | 部分迁移：downloads/large_files/duplicates 的定位/打开换 Button::Mini，保留最新/最旧换 Button::Tonal，类型/原件/副本徽章换 Badge；large_files 大小/类型筛选胶囊（ToggleChip）、photos「保留最佳」（Mini + fg/weight）「全部保留」（Mini + hover_style）「智能选择」（Tonal + Pill）；缺口：photos 序号圆形计数；take(N)、overflow_scroll、缩略图未动 |
| `views/declutter/context_menu.rs::render_declutter_context_menu` (:19) | Menu（Overlay） | adapters | 未迁移 |
| `text_input.rs`（`InputTarget` 与 `impl EntityInputHandler for Root`，:513 起） | TextInput 的状态/分发 | adapters | 未迁移 |

「需移到 shell 或 adapters」的 components 文件汇总：

- shell：`sidebar.rs`、`topbar.rs`。
- adapters：`dialogs.rs`、`progress.rs` 中的两个 `Root` 函数、`search_box.rs`（含 `InputTarget` 的绑定部分）、`updater_dialog.rs`。
- `controls.rs::checkbox`：已改收组件自有 `CheckState`，留在 components（批 1 完成）。

## 2. 关闭策略基线

挂载顺序（后挂的在上层）见 `mod.rs:944-970`：confirm、residual modal、FDA modal、apps 右键菜单、declutter 右键菜单、磁盘下拉、更新弹窗。
全仓库没有任何 Esc 关闭菜单或弹窗的代码：`"escape"` 只出现在 `popover.rs:81`、`search_box.rs:194`、`actions/status.rs:334`（grep 结果）。
全局键位只有 Quit (`main.rs:71-83`)。

| 浮层 | 外部点击是否关闭 | 点击是否穿透到下层 | Esc 是否关闭 | 关闭后焦点 | 是否 occlude |
| --- | --- | --- | --- | --- | --- |
| apps 右键菜单 `views/apps.rs:645`（遮罩 :686-703，卡片 :705-722） | 是。遮罩的 Left/Right mouse_down 都调 `close_context_menu` (:688-702)。卡片只拦 Left (:719)，所以在卡片上右键会落到遮罩并关闭 | 代码推断：穿透。遮罩没有 `stop_propagation`，也没有 `occlude`。未实机确认 | 否 | 不处理，焦点原样保留 | 否 |
| declutter 右键菜单 `views/declutter/context_menu.rs:19`（遮罩 :47-62，卡片 :64-84） | 是。遮罩 Left/Right 关闭；卡片 Left/Right 都拦截 (:79-84) | 同上，代码推断穿透 | 否 | 不处理 | 否 |
| disk_volume 下拉 `views/disk_volume.rs:87`（遮罩 :213-233） | 只有 Left 关闭 (:216-222)；Right 不关 | 代码推断：穿透。触发钮的 `on_click` 是 toggle (:81)，点触发钮可能先被遮罩关掉再被 toggle 重新打开，未确认 | 否 | 不处理 | 否 |
| dashboard / status 的「遮罩」 | 没有找到。`dashboard.rs:66/:77` 的 absolute 是扫描环装饰，`status.rs:432` 是环形图文字叠层。状态页的进程详情是行内侧栏 (`status.rs:1548`)，不是浮层 | 不适用 | 不适用（侧栏靠再次点行或 `selected_proc_pid` 置空关闭，`actions/status.rs:227-232`） | 不适用 | 不适用 |
| confirm 弹窗 `components/dialogs.rs:55`（遮罩 :89-93） | 否。只能点 `confirm-cancel` (:147-153，置 `confirm = None`) 或 `confirm-accept` (:156-165) | 否，被 `occlude` 挡住 | 否 | 不处理 | 是 (:91) |
| residual 弹窗 `dialogs.rs:245`（遮罩 :540-544） | 否。出口是 `resid-done-btn` (:420)、`resid-cancel` (:506)、`resid-clean` (:516) | 否 | 否 | 不处理 | 是 (:543) |
| FDA 弹窗 `dialogs.rs:663`（遮罩 :722-726） | 否。出口是 `fda-later-btn` (:841，调 `close_fda_guide`，`mod.rs:378`)、检查/打开设置钮 | 否 | 否 | 不处理 | 是 (:725) |
| updater 弹窗 `updater_dialog.rs:12`（遮罩 :91-95） | 否。`update-later` (:193-198) 调 `dismiss_update_dialog` (`actions/updater.rs:148`) | 否 | 否 | 不处理 | 是 (:93) |
| status 搜索 popover `views/status.rs:2276`；机制在 `components/popover.rs` | 是，除锚点（搜索框）以外的 mouse_down_out 关闭 (`popover.rs:170-176`，`outside_anchor` :141)。锚点点击不关闭，且容器的 Left mouse_down 会在已关闭时重新打开 (`status.rs:2612-2618`) | 外部点击送达目的控件（`docs/UI_POPOVER.md:57-58`）。浮层内部：不向下层透传 hover、按下、抬起、滚轮 (`popover.rs:153,167,169,179`) | 部分。输入框按 Esc：IME 组合中忽略；浮层打开则只关浮层，焦点留在输入框；浮层已关且有文本则清空文本 (`actions/status.rs:329-339`)。清空会调 `reopen()` (:309) | 外部点击关闭时，如果搜索框有焦点则 `window.blur()`，焦点变为无 (`status.rs:2416-2420`)。Esc 关闭时焦点留在输入框 | 是 (`popover.rs:153`)；用 `deferred(...).priority(100)` 渲染 (:226) |

补充观察：

- 四个 `occlude` 弹窗不接管焦点（全仓库没有 `focus_handle` 被弹窗持有）。弹窗出现时，若搜索框已有焦点，键盘输入是否仍进入它：未确认。
- 右键菜单和磁盘下拉的位置是硬编码：菜单 clamp 到 1040×620 (`apps.rs:658-659`、`declutter/context_menu.rs:26-27`)；下拉固定 `top 168 / left 280` (`disk_volume.rs:226-228`)。popover 也不自动翻转或避让窗口边缘（`docs/UI_POPOVER.md:53-54`）。
- 只有 popover 走 `deferred`；菜单和弹窗靠 `Root::render` 里的追加顺序决定层级。
- confirm 在 residual 与 FDA 之下渲染 (`mod.rs:944-952`)。两者是否会同时存在：未确认。

## 3. 输入基线

### 3.1 InputTarget 分发（`text_input.rs`）

`InputTarget { Apps, File, Status }` 定义在 `text_input.rs:513`。三个输入框各有一个 `TextInputState`（`state.rs:318-347`）和一个 `FocusHandle`，在 `mod.rs:127-129` 创建：

- Apps：`root.apps.input`（`mod.rs:188`）。
- File：`root.search.input`（`mod.rs:238`）。
- Status：`root.monitor.input`（`state.rs:1117`）。

开发环境页（`views/dev_envs.rs:36-63`）与桌面应用页共用 `root.apps.input` 和 `InputTarget::Apps`。两页切换时文本是同一份。

分发入口：

- 键盘按键：`search_box.rs:188-258`，`target` 由搜索框规格带入，不看焦点。
- 输入法与普通字符：`impl EntityInputHandler for Root`（`text_input.rs:685` 起）。这里按焦点判断目标：`active_input_target` (:520) 依次检查 File、Status，其余一律当 Apps。没有焦点时也落到 Apps。
- 光标移动、删除：`text_input_*_target` (:597-:675)。
- 变更后收尾：`after_text_edit_target` (:546)。
- 旧的 `is_file_search: bool` 系列函数 (:558-:683) 只能表达 File/Apps，无法选 Status。

各目标差异：

| 项 | Apps | File | Status |
| --- | --- | --- | --- |
| 焦点来源 | `apps_focus_handle`，只在 `search_box` 的 Left mouse_down 里 `focus()` (`search_box.rs:161-163`)，页面进入时不自动聚焦 | 同左，`search.input.focus_handle` | 同左，`monitor.input.focus_handle` |
| 文本变更后的收尾 | `cx.notify()`（`text_input.rs:554`）。应用列表由 `refresh_apps_view` 按文本失效键重算（`mod.rs:809` 起，组词期间不重排） | `search_input_changed`：120ms 防抖加后台检索（`actions/search.rs:190`）；索引未就绪时直接返回 | `search_popover.edited()`（重开并重置选中）、`rebuild_proc_view()`、`cx.notify()`（`text_input.rs:549-553`） |
| 清空按钮 | `search_clear` + notify（`apps.rs:381-384`）；开发环境页是直接 `text.clear(); sel=0..0; notify`（`dev_envs.rs:63-67`），不清 `marked` | `file_search_clear`：取消检索任务、清文本和 `marked`、用空查询重新检索（`actions/search.rs:372`） | `status_search_clear`：清空、`reopen()`、重置选中、重建视图（`actions/status.rs:307`） |
| Backspace | `search_backspace`：只改文本，随后 notify，不走 `after_text_edit`（`text_input.rs:587`、`apps.rs:385-388`）。开发环境页调 `text_input_delete(false)`，即 `delete_forward`（`dev_envs.rs:68-70` 对应 `text_input.rs:597-612`）。代码推断：开发环境页的 Backspace 实际删光标之后的字符，未实机确认 | `file_search_backspace`：自己实现删除并 `search_input_changed`（`actions/search.rs:350`） | `status_search_backspace`：`delete_backward`、`reopen()`、重置选中、重建视图（`actions/status.rs:316`） |
| Esc | 等同清空（`apps.rs:389-392`），不看组词，不失焦 | 等同清空 `file_search_clear` | 分层：组词中忽略；浮层开则关浮层；否则有文本就清空；永不失焦（`actions/status.rs:329`） |
| Delete | 统一走 `text_input_delete_target` → `after_text_edit_target` | 同左 | 同左 |
| 上下键 / Enter | 无 | 无 | 在 `status.rs:2542-2549` 另挂 `on_key_down`，组词中不拦截，交给 `status_search_candidate_key`（`actions/status.rs:275`） |
| 复制/剪切/粘贴/全选 | 统一在 `search_box.rs:210-257` | 同左 | 同左；粘贴和剪切走 `after_text_edit_target` |
| 候选窗口定位字号 | `character_index_for_point` 里非 File 一律 12.0 (`text_input.rs:805-809`) | 13.0（与 `search.rs:386` 一致） | 12.0，而搜索框规格是 11.5（`status.rs:2517-2520`）。字号不一致，是否造成命中偏差：未确认 |

光标闪烁：`Root::render` 只要三个输入框之一有焦点就启动闪烁（`mod.rs:859-865`）。

### 3.2 search_box 回调契约（`components/search_box.rs`）

签名 (:51-58)：`search_box(spec, on_clear, on_backspace, on_escape, on_bounds, cx) -> Stateful<Div>`。

- 三个回调的类型都是 `Fn(&mut Root, &mut Context<Root>) + 'static`。`on_bounds` 是 `Fn(&mut Root, Bounds<Pixels>)`。
- `on_clear`：点击 ✕ 时触发（:140-157），随后组件自己调 `poke_cursor_blink`。
- `on_backspace`、`on_escape`：在组件的 `on_key_down` 里调（:188-194）。Delete、方向键、Home/End、Ctrl+A/C/X/V 由组件直接调 `Root` 的 `text_input_*_target`，调用方无法拦截。
- `on_bounds`：IME canvas 每次 paint 调用，调用方须把 bounds 存回自己的输入状态（`apps.rs:393-395` 等）。popover 的锚点就是这份 bounds（`status.rs:2414`）。
- `SearchBoxSpec` (:18-48)：`id`、`focus_handle`、`text`、`placeholder`、`selection`、`marked`、`width`、`height`、`font_size`、`cursor_h`、`focused`、`cursor_visible`、`target`。焦点、光标可见相位由调用方传入，组件不读焦点。
- 组件自己处理：鼠标按下聚焦、点击定位、双击全选、拖拽选区（窗口级 move/up，:272-318）、在 paint 阶段把文本排版存入 `text_hit`（:90-93）。
- 调用方的差异只有三个回调、`target` 和尺寸。调用点：`apps.rs:362`、`dev_envs.rs:46`、`search.rs:389`、`status.rs:2515`。

## 4. 列表与滚动基线

滚动条与拖拽的统一接法（四处虚拟列表相同）：`scroll_metrics(&base_handle, 估计视口高, 行数 × 行高)`（`scroll.rs:40`）→ `scrollbar(id, metrics, |thumb| thumb.on_mouse_down(...记录起点到 Root 子状态...))`（`scroll.rs:102`）→ 容器里再放 `drag_capture(...)`（`scroll.rs:141`），它借 `canvas` 注册窗口级 move/up。列表右侧有滚动条时 `pr(SCROLLBAR_W)`。

| 列表 | 定高/变高 | 虚拟化 | 行高常量 | 滚动条与拖拽 | 图标加载 |
| --- | --- | --- | --- | --- | --- |
| apps 软件表 `views/apps_components.rs:282` | 定高（行内名称强制单行，:264） | `gpui::uniform_list`（:316），id 含 `apps.gen`；只构造 `range` 内的行 | `APP_ROW_H = 54.0`（:265）；估计视口 340（:296） | `root.apps.scroll`（`state.rs:702`）+ `apps.scroll_drag`（:703）；`drag_capture` 在 :353 | 不是按可见区。扫描后对全部应用的 `icon_cache_key` 一次性后台加载，先 bundle 并行再 AppKit 回退（`actions/apps.rs:573-597`、`app_icons.rs`）；行里用 `try_get_icon` 取缓存，未命中显示首字母（`apps_components.rs:36-60`） |
| junk 分类展开区 `views/junk.rs:71` | 定高 | 每个分类一个 `uniform_list`（:100） | `ITEM_ROW_H = 36.0`（:51），展开区高度 `min(count × 36, LIST_MAX_H = 380)`（:93、:53） | `junk.scroll: HashMap<CategoryId, UniformListScrollHandle>`（`state.rs:36`），`junk.scroll_drag: Option<(CategoryId, f32, f32)>`（`state.rs:38`）；`drag_capture` 在 :809。外层页面是整页 `overflow_scroll`（:795），无自绘滚动条 | 矢量图标，`category_icon`（:28），无加载 |
| search 结果 `views/search.rs:416` | 定高 | `uniform_list`（:442） | `ROW_H = 34.`（:21）；估计视口 340（:425） | `search.scroll`（`state.rs:751`）+ `search.scroll_drag`（:753）；`drag_capture` 在 :475 | 矢量图标，`FileVisualKind::from_name`（:503），无加载 |
| status 进程表 `views/status.rs:2443` | 定高：分组头、子行、单行都用 `PROC_ROW_H`（:1203/:1336/:1453） | `uniform_list`（:2470），数据是已平铺的 `proc_display_items`（`state.rs:1043` 附近） | `PROC_ROW_H = 32.0`（:1128）；视口高 `core::status::STATUS_PROCESS_TABLE_H`（`status.rs:11` 导入，常量值未读） | `monitor.proc_scroll`（`state.rs:1023`）+ `proc_scroll_drag`；`drag_capture` 在 :2718 | 按可见区懒加载：列表回调里收集 `range` 内行的 `icon_source`，调 `load_visible_process_icons`（`status.rs:2480-2499` → `actions/status.rs:398`）；`is_cached` 过滤已尝试路径 |
| status 概览卡 | 卡片网格，非列表 | 否 | `CARD_H = 232.`、`CARD_MIN_W`（:62-73） | 页面整体 `overflow_scroll`（:2859），无自绘滚动条 | 无 |
| disk 右栏 `views/disk_right.rs:63` | 变高（`py_2`，无固定高度，:476-480） | 否。数据在 `Root` 里截断到 `DISK_MAX_ROWS = 200`（`state.rs:919`、`mod.rs:800`），再 `.children(rows)` 全量构造 | 无常量 | `overflow_scroll`（:306），无自绘滚动条 | 矢量图标 |
| disk 卷下拉 `views/disk_volume.rs:178` | 变高 | 否 | 无 | `max_h 380 + overflow_scroll`（:182），无自绘滚动条 | 无 |
| declutter 下载 `views/declutter/downloads.rs:17` | 变高（`py_2p5`，`flex_none`，:51-52） | 否。`.take(100)`（:40）后全量构造 | 无 | 整页内层 `overflow_scroll`（:215），无自绘滚动条 | 固定矢量图标 |
| declutter 大文件 `large_files.rs:18` | 变高（同上模式） | 否。`.take(100)`（:72） | 无 | `overflow_scroll`（:233） | 矢量图标徽章（按 `icon_type`，:75-80） |
| declutter 重复文件 `duplicates.rs:16` | 变高（`py_2`，:62-63） | 否。分组 `.take(50)`（:40），组内全量 | 无 | `overflow_scroll`（:300） | 无 |
| declutter 相似照片 `photos.rs:17` | 变高（缩略图卡 `h 135`/`h 115`，:124/:324） | 否。分组 `.take(30)`（:36），未展开时冗余张数 `.take(4)`（:77），展开才全显 | 缩略图固定高度，非行高 | `overflow_scroll`（:693） | 直接 `img(photo_path)` 交给 GPUI 加载（:135/:335）；没有应用层按可见区加载。GPUI 自身如何调度解码：未确认 |
| declutter 概览 `overview.rs:14` | 卡片布局 | 否 | 无 | `overflow_scroll`（:29） | 无 |
| dev_envs 左栏主列表 `views/dev_envs.rs:331-436` | 变高（混合了分组小标题和行） | 否。条目逐个 `.child(...)`，没有数量上限 | 无 | `overflow_scroll`（:335），无自绘滚动条 | 无 |
| dev_envs 右栏包列表 `dev_envs.rs:915-939` | 变高 | 否。`visible_pkgs` 经过滤后全量构造（:843、:927） | 无 | `overflow_scroll`（:921） | 无 |
| apps 页其余部分 | 页面本身不滚 | — | — | — | — |
| dashboard | 非列表 | — | — | 整页 `overflow_scroll`（`dashboard.rs:362`） | — |
| residual 弹窗列表 `components/dialogs.rs:643` | 变高 | 否 | 无 | `max_h 320 + overflow_scroll`（:646-647） | 无 |

结论：

- 只有 apps、junk、search、status 进程表四处是虚拟化的，且都是定高 `uniform_list`。
- `list`（变高虚拟化）或手写可见区切片：在 `src/ui` 中没有找到使用。
- declutter 各页、disk、dev_envs 靠 `take(N)` 截断或不截断加整体 `overflow_scroll`，没有自绘滚动条。
- 滚动句柄和拖拽状态都散落在 `Root` 子状态：`AppsState`、`SearchState`、`JunkState`、`StatusState`。

## 5. 对后续批次的约束

动手前要对照的既有约束：

- `docs/PITFALLS.md` P2、P3：状态卡片网格的占位块与卡片外壳度量必须一致，徽章不能依赖文字省略。
- P51、P52、P53：popover 的显示由焦点和显式关闭控制，不由查询内容控制；键盘候选与渲染共用同一份来源；必须保留 `occlude()`。
- 本文第 2 节「点击是否穿透」中标注代码推断的几项，迁移前要先在实机验证，再决定新 Menu 组件是否 `occlude`。
- 迁移要补回归测试，并按 `AGENTS.md` 先对要动的路径跑 `verify-notes.py --for-path`。
