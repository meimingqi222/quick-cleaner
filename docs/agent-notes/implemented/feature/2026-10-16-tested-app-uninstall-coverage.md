# Agent Note: tested-app inventory extends software-management uninstall residuals

Status: implemented

## Problem

用户要求对照 https://mole.fit/tested-apps 的全部应用，补充软件管理的卸载清理，
不是智能清理。2026-10-16 公开索引有 824 项，其中 822 项有详情链接；vChewing
与 DiskWise 无详情链接。代码片段和文本表格提取出 4663 个去重应用/路径片段；
29 个详情未提供可提取的具体路径。公开指南是审计输入，不是全部目录可删的证明。

原来卸载前扫描只枚举若干 Library 根的直接子项，因而会漏掉共享父目录下的
应用专属条目、异名更新器文件、厂商下面的产品/版本目录和浏览器宿主清单。
用户纠正范围后，本轮误加到智能清理的 macos 规则与对应目录选择器测试全部撤回。

## Decision

扩充 `rules/residual-macos.toml` 与 `src/platform/macos/residuals.rs` 的逐应用
扫描，并补充 `src/platform/macos/apps.rs` 的官方卸载路由，不改变孤儿扫描。
现有软件管理流程在卸载前采集这些候选、卸载成功后复核
仍在的路径，再展示残留确认；不在卸载后重建或扩大授权。

- 沿用 locations 声明，增加 review_id_dirs（WebKit）与 review_dirs（独立更新器
  缓存、SentryCrash、PLCrashReporter、CloudKit、Crashlytics、Bugsnag）。父目录
  只用于查找，精确主/嵌入 Bundle ID 子项才收；后者也允许精确安全应用名。
- app: 加完整主 Bundle ID 的键声明精确异名路径：Notion updater、ImageOptimResults.db、
  Linkly 诊断目录、JetBrains Toolbox 三个产品目录、Chrome/Brave 专属配置和缓存。
  不按显示名触发异名映射，不收 Google、BraveSoftware、JetBrains 等共享父目录。
- versioned: 加完整主 Bundle ID 的键声明产品前缀。Android Studio、IntelliJ IDEA、
  IDEA CE、PyCharm、DataGrip 只接受产品前缀之后非空点分数字版本，不做通配符
  匹配，不吞掉 CE、Beta 或同厂商其它产品。每个父目录最多检查 4096 项。
- 浏览器 NativeMessagingHosts 仅收 JSON 中的 path 确实指向当前 .app 内现存
  文件的清单。文件名或 JSON name 不是归属证明，拒绝链接、路径越界和其它 app。
  同时检查用户及系统 Library 的已声明 Chrome/Chromium/Edge/Brave/Vivaldi/Mozilla 根。
- 新增项均 Possible、不预选，使用共享路径限制、ResidualItem 身份捕获和原有
  占用/清理/成功核验。WebKit 可能有登录态和离线数据，不能当日常缓存自动清掉。
- 全文复核再补 57 条精确/数字版本路径，包括 Fleet、MEGAsync、飞书与 Lark
  分版、腾讯文档、WPS Notes、Mozilla 更新器、ZoomChat/ZoomPhone 等。
- `src/core/cask.rs` 与 `src/platform/macos/cask_uninstall.rs` 读取标准 Homebrew
  Caskroom 的本地 receipt、config 与最新已存定义，按持久化 appdir/target 关联
  所选应用。仅允许一个 app 和该主 ID 的 quit；普通 uninstall 中的脚本、服务、
  pkgutil、额外应用、未知 artifact、flight block、非空 variations 均拒绝自动运行。
  Ruby 定义、第三方 tap、版本/旧 token 歧义也拒绝。可选 zap 不运行。
  执行前重读元数据和对象身份；固定命令清空继承环境并关闭自动更新、autoremove、
  API 安装和 analytics。退出成功后必须确认本体、receipt、定义均 NotFound。
- Zoom 按精确 ID 声明 Frameworks 内的官方卸载器；缺失、重定向或执行失败
  均停止，不回退 Trash。Docker、TeamViewer、Little Snitch Mini、Surge、Synology、
  DisplayLink、Wacom Center、Stream Deck 的指定主 ID 走官方人工流程，不以
  直接删 bundle 冒充服务/扩展卸载；现有输入法规则保持。

每条异名与版本声明的来源链接随 TOML 维护。嵌套通用布局的代表来源为
https://mole.fit/tested-apps/wispr-flow 、https://mole.fit/tested-apps/fork 、
https://mole.fit/tested-apps/opencat 、https://mole.fit/tested-apps/ticktick 。
浏览器清单案例来自 https://mole.fit/tested-apps/notion 。

## Alternatives considered

把路径加到智能清理：拒绝，用户明确要求卸载流程。把 .android、.gradle、SDK、
模型和素材全部归到某款 GUI app：拒绝，它们可能供其它 IDE/CLI/项目使用。
把卸载指南中的每个路径或通配符直接变成规则：拒绝，路径本身不证明归属，
尤其是共享父目录、登录凭据、用户项目及自定义存储。

按 NativeMessagingHosts 文件名认领：拒绝，同名清单可以指向别的应用或用户脚本。
将本轮补充扩展到孤儿扫描：拒绝，未安装应用不能再读取真实组件证据，P31 保持不变。

## Consequences

索引 824 项中 822 篇完整指南均已复核，694 篇含 Homebrew 命令；逐项 CSV/JSON
保留完整命令及条件、服务步骤、保留数据、来源验证局限和原文证据。校验为 822 个
唯一 URL、无缺项、无引文错误；按 slug 纠正 164 个批次序号并去重一次重复审查。
vChewing 与 DiskWise 没有详情页，不能称其卸载方法已核实。

不能承诺全部零残留，也没有安装它们逐一运行。共享 SDK/模型/项目、系统扩展、
权限/SIP、需要用户选择的官方卸载步骤保持保护或人工处理。产品改动已提交推送，
v0.0.16 tag 指向通过原生 CI 的提交。Homebrew CLI 不提供不可变 receipt 执行接口：执行前复读只能缩小
竞态窗口，不能消除 Homebrew 自身加载元数据的竞态；NO_INSTALL_FROM_API 不等于
完全离线。任一 Caskroom 损坏/不可读当前会保守阻止卸载，包括无关应用，这是
已知可用性限制；不支持自定义 Homebrew prefix。智能清理保持原状。

## Verification

- `src/platform/macos/residuals.rs::webkit_residuals_require_exact_owner_and_manual_selection`
- `src/platform/macos/residuals.rs::uninstall_review_finds_nested_and_aliased_residuals_without_shared_parents`
- `src/platform/macos/residuals.rs::uninstall_browser_manifests_require_an_executable_inside_the_app_bundle`
- `src/core/cask.rs::installed_cask_scope_uses_persisted_appdir_and_exact_target`
- `src/core/cask.rs::installed_cask_scope_refuses_companions_scripts_and_unknown_scope`
- `src/platform/macos/cask_uninstall.rs::cask_inventory_uses_installed_scope_and_rejects_corrupt_or_redirected_records`
- `src/platform/macos/apps.rs::zoom_discovers_only_the_declared_confined_uninstaller`
- `src/core/rules/uninstall.rs::vendor_component_removal_never_degrades_to_bundle_trash`

Proved: Linux 临时隔离 Cargo harness 直接引用生产 cask 策略与 inventory 模块，
三项 cask 测试通过，真实文件夹覆盖重命名目标、持久化 appdir、最新定义中的伴随
应用、损坏和重定向记录；focused clippy -D warnings 通过。这不是原生 brew 执行。
13 份 TOML 可解析，初始 locations 50/64；Zoom 改用路径声明后为 51/64。
格式、空白和 UI 守卫通过。
最终同一隔离 harness 全跑为 32 passed、0 failed（没有忽略测试），clippy 通过；
证据 `docs/agent-notes-evidence/2026-10-16-uninstall-portable-check.log`。

后续原生 CI run 38907489652：macOS 编译、严格 clippy 和 811 项测试通过；
Windows 严格 clippy、提权 ACL 检查和 778 项测试通过，回归记录与真实测试绑定
校验通过。上述新增残留扫描、Zoom 和人工路由测试均参与原生执行：
https://github.com/meimingqi222/quick-cleaner/actions/runs/38907489652 。
既有忽略项为 macOS 12 项、Windows 9 项；本轮没有新增忽略测试。

Limitation: Linux orb 的完整 cargo test 在 gdk-sys 缺少 gdk-3.0 处停止，
产品也没有 Linux 平台实现；以上原生结论来自 GitHub CI，不来自 orb。
隔离 harness 的全跑曾为 31 通过、1 失败，失败是既有模型测试的 Linux inode
立即复用，不隐藏、不改该测试。界面文字布局、真实 Homebrew/官方卸载器及全部
822 款软件安装后逐一卸载仍未验收；不能将 CI 单元回归当成全部软件零残留证明。

Pitfalls checked: 全表已读；P19 保留原有残留去重，P25/P28/P37 保留冻结身份及
删除前复核，P30 不改变固定 GPUI Git patch，P31 不扩张孤儿扫描，P32 不把名称
当可重建缓存证据，P49/P50 扩展同样的官方卸载/数据删除保护；P20/P46 保持
命令失败无回退及进程树超时，P36 检查 TOML 表头；P58 文案走 i18n。
其余 PITFALLS 未直接涉及；Windows 卸载执行路径未改。
