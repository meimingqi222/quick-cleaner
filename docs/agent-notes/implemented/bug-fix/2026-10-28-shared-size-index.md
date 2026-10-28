# Agent Note: 应用和残留的大小估算复用已有索引

Status: implemented

## Problem

软件管理对每个应用执行 `du` 或目录遍历，残留发现也对每个目录分别遍历；即使智能清理已有完整的磁盘索引，这些占用估算仍重复读取同一批文件。现场 143 款应用和 浏览器缓存扫描在同机测试中有明显等待。

## Decision

`internal/core/size_indexes.go` 只为扫描上下文携带不可变索引，用于大小估算。现存普通目录且索引覆盖完整才可使用缓存；不存在、符号链接、索引缺失或被标记为不完整时，沿用原有实时遍历。macOS 软件列表可加载已有磁盘索引并只对未覆盖的 app 执行 `du`；Windows 已持有 MFT 时复用其逻辑大小，访问时间仍实时检查浅层文件。残留目录仅对去重后幸存的项目进行有界并发统计。

`internal/core/disk_index_macos.go` 的 `Cached` 从已持有或已有磁盘快照取数，本次调用不发起全盘扫描或事件回放，加载后启动原有定期后台回放；残留检测的 `Peek` 只取已加载快照，不因一个残留扫描等待冷索引加载。快照可能滞后，展示值是估算。应用发现、归属、冻结身份、用户选择、占用和删除前安全检查均不以大小缓存授权；卸载后的候选复核显式屏蔽快照并重新读取文件系统。

## Alternatives considered

- 软件列表没索引时主动重建整盘：首次进入可能需要数分钟，不适合等待软件列表。
- 对未覆盖的子树使用不完整聚合：会把未知数据误算成零。
- 用索引推断应用已卸载、路径没有被替换或目标未被占用：旧快照不能提供这些实时证据。
- 所有残留扫描都先冷读整个索引：小范围扫描可能比原来的实时遍历更慢。

## Consequences

已有快照被多个页面复用，冷缓存缺失、忙碌或覆盖不完整时依旧可工作；首次打开软件列表加载快照有额外开销。显示大小可能与当前磁盘状态略有差异。无扫描授权范围扩大，无变更清理操作。

## Verification

- `internal/core/size_indexes_test.go::TestResidualAndAppSizesUseOnlyCoveredIndexes`
- `internal/core/size_indexes_test.go::TestResidualVerificationUsesLiveSize`
- `internal/core/disk_index_macos_test.go::TestMacDiskIndexCachedNeverStartsOrReplaysScan`
- `internal/platform/apps_inventory_darwin_test.go::TestMacAppSizesUseIndexAndOnlyRunDUForMissingPaths`

Proved: 原实现的同一测试分别以 4096 和 12 字节的实时统计替代索引哨兵 424242，见 `docs/agent-notes-evidence/2026-10-28-shared-sizes-red.log`；接入后同 focused 绿跑见 `docs/agent-notes-evidence/2026-10-28-shared-sizes-green.log`。本轮最初接入时卸载后复核误用哨兵 424242，自然红跑见 `docs/agent-notes-evidence/2026-10-28-size-verification-red.log`；屏蔽旧快照后同 focused 绿跑见 `docs/agent-notes-evidence/2026-10-28-size-verification-green.log`。

同一台 Mac 的只读单次对比：应用盘点 12.75 秒（实时）与 1.00 秒（已有索引），均 143 款；浏览器残留 4.70 秒与 0.20 秒，均 17 项。索引冷读 2.24 秒另计。实时测试先运行、随后索引测试，因此 OS 文件缓存也影响结果；快照时间不同使大小略有变化，不能将数字当成普遍加速保证。记录见 `docs/agent-notes-evidence/2026-10-28-shared-scan-performance.log`。
