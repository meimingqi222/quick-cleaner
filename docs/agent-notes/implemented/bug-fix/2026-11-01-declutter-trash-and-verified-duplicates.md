# Agent Note: 冗余整理固定移入废纸篓，大文件重复先做全文校验

Status: implemented

## Problem

功能审阅（`docs/temp/functional-review-2026-11-01.md`）确认两处数据风险：

1. 冗余整理的确认和完成文案都说「移入废纸篓，可恢复」，执行却读全局「删除到回收站」设置。
   该设置默认关闭，于是下载项、大文件、重复文件、相似照片全部被**永久删除**。Rust 参照
   `src/ui/actions/declutter.rs`（2026-11-01 前的历史）固定用 `Disposal::RecycleBin`，
   这是 Go 移植引入的回归。复现：14 字节文件经确认后 `freed=14`，状态仍是「已把 1 个项目移入废纸篓」。
2. 超过 2 MiB 的重复识别只采样五个 16 KiB 区域。两个 4 MiB 文件在 128 KiB 处差一个字节，
   仍被分成一组并默认勾选 4 MiB 待清理。

## Decision

- `requestArbitraryClean`：`action == "declutter"` 时处置固定为 `RecycleBin`，与文案和 Rust 一致；
  磁盘透镜仍跟随全局设置。新增 `Root.cleanArbitrary` 注入点，测试只记录处置，不往真实回收站放文件。
- 重复文件：两个参照哈希仍作候选过滤，组 ID 格式不变；大小超过 2 MiB 的候选组按全文 SHA-256
  拆分，只保留两份以上完全相同的集合。一个采样匹配拆成多组时 ID 加 `-N` 后缀，保证列表 key 唯一。
  读不完整的文件不进组（无法证明是重复）。

## Alternatives considered

- **冗余整理也跟随全局设置，改文案。** 「整理」承诺可恢复是该功能的前提，且与 Rust 不符。
- **大文件疑似重复只标注、不预选。** 用户仍要自己判断哪些是真重复；全文校验只读已碰撞的候选，代价可控。
- **全部文件都全文哈希。** 不碰撞的文件不需要读全文，白白增加扫描时间。

## Consequences

- 冗余整理永远移入废纸篓，不释放磁盘空间（文案本来就这么说）。
- 大文件重复扫描会完整读取发生采样碰撞的候选文件；大型视频副本较多时扫描会变慢。
- 与 Rust 的有意偏离已写进 `GO_MIGRATION.md`。

## Verification

- `internal/app/declutter_safety_test.go::TestDeclutterCleanAlwaysUsesTrashWhileDiskLensFollowsSetting`
- `internal/core/declutter_test.go::TestDuplicateScanVerifiesFullContentAboveTwoMiB`
- `internal/core/declutter_test.go::TestDuplicateScanTwoMiBBoundaryMatchesRust`

Proved: TDD 先红。修复前处置测试报 `disposals = [permanent permanent], want [RecycleBin Permanent]`，
见 `docs/agent-notes-evidence/2026-11-01-declutter-trash-red.log`；修复后绿跑见
`docs/agent-notes-evidence/2026-11-01-declutter-trash-green.log`。
重复测试修复前报 `files differing outside the samples were grouped: 1 groups, 4194304 bytes pre-selected`；
制造型红跑去掉拆分 ID 后缀时报 `split sets` 冲突，均见
`docs/agent-notes-evidence/2026-11-01-duplicate-full-verify-red.log`；绿跑见
`docs/agent-notes-evidence/2026-11-01-duplicate-full-verify-green.log`（Rust 2 MiB 边界与组 ID 测试仍绿）。

Pitfalls checked: P130 的整组拒绝/警告不变；删除仍只用扫描时冻结的身份。
