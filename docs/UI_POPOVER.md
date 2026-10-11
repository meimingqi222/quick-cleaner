# 可复用的搜索候选浮层

实现位于 `src/ui/components/popover.rs`，状态页的使用入口是
`src/ui/views/status.rs::render_status_search_popover`。组件不依赖 `Root`、进程数据
或查询语法；候选文案与业务操作由调用方提供，文案继续使用 `tr_*`。

## 组件分工

| API | 职责 |
| --- | --- |
| `PopoverCandidate` | 提供唯一、稳定的 `id()`，不要使用会随排序改变的行号 |
| `PopoverState` | 保存关闭状态、选中身份、滚动位置；处理键盘和 IME 边界 |
| `popover_surface` | 外观、定位、鼠标命中隔离、锚点以外点击关闭 |
| `popover_list` | 有高度限制的滚动候选列表，绑定状态的滚动句柄 |
| `popover_row` | 统一行样式、选中高亮、鼠标悬停同步选中项 |
| `popover_overlay` | deferred 渲染层级 |

## 接入步骤

1. 在视图状态中保存一个 `PopoverState`。候选实现 `PopoverCandidate`，身份在刷新
   与重排后仍应对应同一个逻辑项目。
2. 用同一份候选列表渲染行并处理键盘，避免显示内容和 Enter 接受内容不同。
3. 输入改动调用 `edited()`；需要重新展开时调用 `reopen()`；明确关闭调用
   `dismiss()`。是否渲染由 `is_open(anchor_focused)` 决定，查询文本只负责筛选。
4. 搜索框保留焦点，按键监听调用 `handle_key(key, composing, &items)`。
   `Changed` 时重绘，`Confirm(index)` 时执行该候选业务动作，`Dismissed` 时重绘。
   `Unhandled` 留给输入框。IME 组合输入时设置 `composing = true`。
5. 使用下面的结构，给行添加自己的选择回调，给 surface 提供关闭回调。

```rust,ignore
let selected = state.selection(&items);
let list = popover_list("my-candidates".into(), state, 240.)
    .children(items.iter().enumerate().map(|(index, item)| {
        popover_row(item.id().to_owned().into(), selected == Some(index),
            |view: &mut MyView| &mut view.popover, cx)
            .child(item.label.clone())
            // 添加调用方的选择处理。
    }));
popover_overlay(
    popover_surface(spec, |view: &mut MyView, _, cx| {
        view.popover.dismiss();
        cx.notify();
    }, cx).child(list)
)
```

## 交互约定与边界

默认高亮第一项，上下循环选择；关闭后方向键重新展开。Enter 在关闭时无操作，
空列表无候选动作。候选身份消失时回到第一项。悬停改变选中项；方向键选中时
会滚动到对应行。每个滚动列表的直接子项应对应一个候选，标题与提示放在列表外。

`PopoverSpec` 的 `anchor_bounds` 使用窗口坐标，`top` 相对于调用方的定位容器；
调用方提供宽度、最大高度及锚点几何。组件目前不自动翻转方向或避让窗口边缘。

浮层内部不会向后层透传 hover、鼠标按下/松开或滚轮；内部列表仍可滚动。
锚点点击不关闭；其他位置的点击关闭后继续送达目的控件。不要移除 `occlude()`
或只保留 `stop_propagation()`，后者不能隔离 GPUI hover 命中。

回归决策见 `docs/agent-notes/implemented/bug-fix/2026-10-11-popover-mouse-isolation.md`。
