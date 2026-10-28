# Agent Note: 状态卡片占位块保留相同盒子度量

Status: implemented

## Problem

状态页不满一行时，空占位块缺少 padding 和 border 会改变 flex 基础尺寸，使下排真实卡片比上排宽。P2 记录了历史实机现象。

## Decision

`src/ui/views/status/cards.rs` 的 `status_card` 通过共享卡片外壳设置边框并设置内边距；`card_rows` 的占位块保持相同的 padding、border、flex 和最小宽度。改动这些度量时必须一起调整。

## Alternatives considered

只给占位块设置 flex 和最小宽度会让其基础尺寸与真实卡片不同。

## Consequences

不满行和满行都应检查边缘对齐。

## Verification

本次核对了两处源码；没有找到直接验证实际布局宽度的自动测试，也没有重新进行 GUI 实机验证。P2 的历史测量不能当作本次测试结果。
