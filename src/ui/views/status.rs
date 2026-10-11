//! 状态监控视图（Stitch「Dashboard - System Status with Fan Control」设计：
//! 健康概览 / CPU / GPU / 内存 / 磁盘 / 网络 / 电池 / 风扇温度八卡片
//! + 活动进程表）
//!
//! 卡片数量按硬件有无浮动：读不到 GPU 的机器少一张、没有电池的台式机少
//! 一张。少的那几格由 `card_rows` 补等宽占位，两排卡片的尺寸必须一致——
//! 大小不一的网格看起来就像布局坏了。

use crate::core::i18n::Language;
use crate::core::model::{commas, fmt_mem, fmt_size, truncate};
use crate::core::status::{gpu_labels, FanMode, StatusSnapshot, STATUS_PROCESS_TABLE_H};
use crate::ui::app_icons::try_get_icon;
use crate::ui::components::cards::card;
use crate::ui::components::controls::page_heading;
use crate::ui::components::donut::{render_donut, DonutSegment};
use crate::ui::components::icons::*;
use crate::ui::components::popover::{
    popover_list, popover_overlay, popover_row, popover_surface, PopoverCandidate, PopoverSpec,
};
use crate::ui::components::scroll::{
    drag_capture, drag_to_offset, scroll_metrics, scrollbar, SCROLLBAR_W,
};
use crate::ui::components::text_tooltip;
use crate::ui::i18n::*;
use crate::ui::state::{ProcSort, ProcSortKey, StatusTab, STATUS_HISTORY_LEN};
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::{
    div, img, prelude::*, px, relative, rgb, AnyElement, Context, Div, ImageSource, SharedString,
    Stateful, Window,
};

/// 网络卡片图标：圆形线框 + 十字经纬，不引入新字形依赖。
fn icon_globe(fg: u32, size: f32) -> AnyElement {
    div()
        .w(px(size))
        .h(px(size))
        .rounded_full()
        .border_2()
        .border_color(rgb(fg))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(size * 0.62))
                .h(px(size * 0.62))
                .flex()
                .flex_col()
                .items_center()
                .justify_between()
                .child(div().w_full().h(px(1.5)).rounded_full().bg(rgb(fg)))
                .child(div().w(px(1.5)).h_full().rounded_full().bg(rgb(fg)))
                .child(div().w_full().h(px(1.5)).rounded_full().bg(rgb(fg))),
        )
        .into_any_element()
}

/// 单张卡片的最小宽度。四列布局下窗口一窄，卡片会挤到数字换行、脚注被切；
/// 给个下限让整行改为换行（`flex_wrap`）而不是继续压扁——4×2 变 2×4 仍然
/// 读得下去，压成一条缝就没法看了。
const CARD_MIN_W: f32 = 210.;

/// 一行几张卡。
const CARDS_PER_ROW: usize = 4;

/// 卡片的统一高度下限。行高由该行最高的卡片决定，不给下限的话上排
/// （CPU / GPU 带 64px 柱状历史）218px、下排 173px，两排一大一小。
/// 用 `min_h` 而不是 `h`：真有更高的内容时让卡片长出来，好过被裁掉。
///
/// 上限按**最高的那种配置**定：双显卡机器的 GPU 卡多一排切换按钮，实测
/// 229px。按 220 定的话，那台机器上排 229、下排 220，又不齐了。
const CARD_H: f32 = 232.;

/// 状态页所有卡片的统一外壳：等宽（一行四等分）、等高、内容不外溢。
///
/// `overflow_hidden` 是硬边界：徽章和脚注再长也只能被截断，不能画到卡片
/// 外面去（Windows 的「Windows 11 Home China」实测整块画到了卡片右边界外）。
fn status_card() -> Div {
    card()
        .flex_1()
        .min_w(px(CARD_MIN_W))
        .min_h(px(CARD_H))
        .overflow_hidden()
        .p_5()
        .flex()
        .flex_col()
}

/// 把可用的卡片按每行 [`CARDS_PER_ROW`] 张排成若干行。
///
/// 台式机没有电池、Windows 读不到 GPU，这些卡片会整张缺席（见
/// `render_status_view`），此时最后一行不足四张。用**隐形占位**补齐而不是
/// 让剩下的卡片拉伸：三张卡各占 1/3 会比上一行的 1/4 明显更宽，两行卡片
/// 对不齐，看着像布局坏了。
fn card_rows(cards: Vec<Div>) -> Vec<Div> {
    let mut rows = Vec::new();
    let total = cards.len();
    let mut iter = cards.into_iter();
    let mut placed = 0;
    while placed < total {
        let mut row = div().flex().flex_wrap().gap_3();
        let mut n = 0;
        while n < CARDS_PER_ROW {
            match iter.next() {
                Some(card) => {
                    row = row.child(card);
                    n += 1;
                    placed += 1;
                }
                None => break,
            }
        }
        // 末行补空位，保证每张卡的宽度和满行时一致。占位块的内边距和边框
        // 必须跟真卡片一模一样：flex 的 base size 会被 padding + border 之和
        // 托底（taffy 和 Chrome / Firefox 一致的行为），空 div 少这 42px，
        // 等分的份额就白送给同排的真卡片——实测下排两张卡各 255px、上排
        // 四张各 235px，两排卡片宽度对不上就是这么来的。
        for _ in n..CARDS_PER_ROW {
            row = row.child(div().flex_1().min_w(px(CARD_MIN_W)).p_5().border_1());
        }
        rows.push(row);
    }
    rows
}

/// 卡片小标题行：图标 + 标题 + 右侧徽章。
fn card_header(icon: AnyElement, title: &str, chip: Option<String>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .mb_4()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                // 不给 min_w(0)，标题和徽章都按各自内容宽度占位，两边加起来
                // 超过卡片宽度时谁也不肯让，直接顶出卡片。
                .min_w(px(0.))
                .child(icon)
                .child(
                    div()
                        .min_w(px(0.))
                        .truncate()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT))
                        .child(title.to_string()),
                ),
        )
        .children(chip.map(header_chip))
}

/// 卡片右上角的徽章。
///
/// 卡片只有约 1/4 行宽，徽章是次要信息：位置不够时先缩它、缩不下就省略号，
/// 绝不许把卡片撑破。`truncate` 里的 `overflow_hidden` 同时把自动最小尺寸
/// 归零，否则文字的最小内容宽度会顶着徽章不缩。
fn header_chip(text: String) -> Div {
    div()
        .flex_shrink()
        .min_w(px(0.))
        .truncate()
        .px_2()
        .py(px(2.))
        .rounded_md()
        .bg(rgb(SURF_LOW))
        .text_xs()
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(rgb(MUTED))
        .child(text)
}

/// 大数字 + 单位（各卡片的主读数）。
fn big_number(value: String, unit: String) -> Div {
    div()
        .flex()
        .items_baseline()
        .gap(px(6.))
        .child(
            div()
                .text_xl()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(TEXT))
                .child(value),
        )
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(rgb(MUTED))
                .child(unit),
        )
}

/// 水平进度条（磁盘 / 内存卡片）。
fn meter(ratio: f32) -> Div {
    meter_colored(ratio, PRIMARY)
}

/// 同上，但可以指定填充色——电池要用状态色（低电量变红）表达紧迫程度。
fn meter_colored(ratio: f32, color: u32) -> Div {
    let ratio = ratio.clamp(0.0, 1.0);
    div()
        .w_full()
        .h(px(6.))
        .rounded_full()
        .bg(rgb(SURF_HIGHEST))
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(rgb(color))
                .w(relative(ratio)),
        )
}

/// 卡片底部的一行小字说明。
fn card_footer(cells: Vec<String>) -> Div {
    // 必须能换行：四列布局下卡片只有约 1/4 宽，脚注多一两项就会顶出卡片
    // 右边缘（风扇卡片的「已接管 · 退出后自动恢复」实测溢出）。
    let mut row = div()
        .mt_3()
        .w_full()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .text_xs()
        .text_color(rgb(OUTLINE));
    for (i, cell) in cells.into_iter().enumerate() {
        if i > 0 {
            row = row.child(div().text_color(rgb(OUTLINE_VAR)).child("·"));
        }
        row = row.child(div().child(cell));
    }
    row
}

/// 健康分对应的评级色。分数本身由 `core::status::health_report` 算，
/// 这里只负责挑颜色。
fn health_color(score: u32) -> u32 {
    if score >= 80 {
        PRIMARY
    } else if score >= 60 {
        CAUTION
    } else {
        ERROR
    }
}

fn health_word(score: u32, lang: Language) -> &'static str {
    if score >= 80 {
        tr_status_good(lang)
    } else if score >= 60 {
        tr_status_fair(lang)
    } else {
        tr_status_high(lang)
    }
}

fn uptime_label(secs: u64, lang: Language) -> String {
    let d = secs / 86_400;
    let h = (secs % 86_400) / 3_600;
    let m = (secs % 3_600) / 60;
    match lang {
        Language::Zh => {
            if d > 0 {
                format!("{d} 天 {h} 小时")
            } else if h > 0 {
                format!("{h} 小时 {m} 分")
            } else {
                format!("{m} 分钟")
            }
        }
        Language::En => {
            if d > 0 {
                format!("{d}d {h}h")
            } else if h > 0 {
                format!("{h}h {m}m")
            } else {
                format!("{m}m")
            }
        }
    }
}

/// CPU 占用柱状历史（右侧最新的一根是实心主色，样本不足补空槽占位）。
fn history_bars(history: &[f32]) -> Div {
    let mut bars = div()
        .mt_3()
        .h(px(64.))
        .w_full()
        .flex()
        .items_end()
        .gap(px(2.));
    let n = history.len().min(STATUS_HISTORY_LEN);
    for (i, usage) in history.iter().enumerate() {
        let ratio = (usage / 100.0).clamp(0.03, 1.0);
        let alpha = if i + 1 == n {
            1.0
        } else {
            0.25 + 0.6 * (i as f32 / STATUS_HISTORY_LEN as f32)
        };
        bars = bars.child(
            div()
                .flex_1()
                .h(relative(ratio))
                .rounded_t_sm()
                .bg(rgba(PRIMARY, alpha)),
        );
    }
    for _ in n..STATUS_HISTORY_LEN {
        bars = bars.child(
            div()
                .flex_1()
                .h(px(2.))
                .rounded_t_sm()
                .bg(rgb(SURF_HIGHEST)),
        );
    }
    bars
}

/// 进程行的 CPU 迷你条 + 数值。>50% 红、>15% 橙、其余灰。
fn cpu_cell(cpu: f32, width: f32) -> Div {
    let (bar_color, text_color) = if cpu > 50.0 {
        (ERROR, ERROR)
    } else if cpu > 15.0 {
        (CAUTION, CAUTION)
    } else {
        (OUTLINE_VAR, MUTED)
    };
    div()
        .w(px(width))
        .flex_none()
        .flex()
        .items_center()
        .justify_end()
        .gap_1p5()
        .child(
            div()
                .w(px(24.))
                .h(px(3.))
                .rounded_full()
                .bg(rgb(SURF_HIGHEST))
                .overflow_hidden()
                .child(
                    div()
                        .h_full()
                        .rounded_full()
                        .bg(rgb(bar_color))
                        .w(relative((cpu / 100.0).clamp(0.0, 1.0))),
                ),
        )
        .child(
            div()
                .w(px(38.))
                .text_right()
                .text_xs()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(rgb(text_color))
                .child(format!("{cpu:.1}%")),
        )
}

fn render_health_card(root: &Root, snap: &StatusSnapshot) -> Div {
    let lang = root.language;
    let (total, free) = root.disk.volume_space(&root.disk.volume).unwrap_or((0, 0));
    // 磁盘、内存、换页、CPU、运行时长五个维度加权扣分，口径见
    // `core::status::health_report`。CPU 用最近这一屏历史的均值而不是瞬时值：
    // 单拍冲到 90% 太常见了，持续 90% 才叫问题。
    let cpu_avg = if root.monitor.cpu_history.is_empty() {
        snap.cpu_usage
    } else {
        root.monitor.cpu_history.iter().sum::<f32>() / root.monitor.cpu_history.len() as f32
    };
    let health = crate::core::status::health_report(crate::core::status::HealthInputs {
        disk_free_ratio: (total > 0).then(|| free as f32 / total as f32),
        mem_used: snap.mem_used,
        mem_total: snap.mem_total,
        swap_used: snap.swap_used,
        cpu_avg,
        uptime_secs: snap.uptime_secs,
    });
    let score = health.score;
    let score_color = health_color(score);

    let mut header = div()
        .flex()
        .items_center()
        .gap_2()
        .child(icon_shield(PRIMARY, 16.))
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(TEXT))
                .child(tr_status_card_health(lang).to_string()),
        )
        .child(div().flex_1().min_w(px(0.)));
    // 四列布局下这张卡只有约 1/4 宽，塞两个徽章会把第二个裁掉半截。
    // 内存总量在同排的内存卡片里已经有「共 xx GB」，这里只留系统版本。
    //
    // 24 字的上限是按 macOS 的「macOS 15.6」定的，Windows 的
    // 「Windows 11 Home China」一个都截不掉——上限只是粗筛，真正兜底的是
    // `header_chip` 的截断和卡片的 `overflow_hidden`。
    if !snap.os_name.is_empty() {
        header = header.child(header_chip(truncate(&snap.os_name, 24).to_string()));
    }

    // 得分环 + 中央文字
    let ring = div()
        .relative()
        .w(px(72.))
        .h(px(72.))
        .flex_none()
        .child(render_donut(
            vec![DonutSegment {
                ratio: score as f32 / 100.0,
                color: PRIMARY,
            }],
            72.,
            7.,
        ))
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(score_color))
                        .child(format!("{score}")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(tr_status_health_short(lang)),
                ),
        );

    status_card()
        .child(header)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .mt_2()
                .child(
                    div()
                        // 四列布局下这张卡很窄，「主要扣分项：…」那行比卡片还长。
                        // 不给 flex_1 + min_w(0) 的话这一列按内容撑开，把得分环
                        // 顶出卡片右边缘（真机上环被裁掉半个）。
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap_2()
                                .child(
                                    div()
                                        .text_xl()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(score_color))
                                        .child(health_word(score, lang).to_string()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(OUTLINE))
                                        .child(format!("{score}/100")),
                                ),
                        )
                        .child(div().text_xs().text_color(rgb(MUTED)).child(format!(
                            "{} {}",
                            fmt_size(free),
                            tr_status_available(lang)
                        )))
                        // 光给个数字没用，得说清楚分是从哪掉的。
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(if health.worst.is_some() {
                                    CAUTION
                                } else {
                                    OUTLINE
                                }))
                                .child(match health.worst {
                                    Some(factor) => tr_health_factor(lang, factor),
                                    None => tr_health_all_clear(lang).to_string(),
                                }),
                        ),
                )
                .child(ring),
        )
        .child(card_footer(vec![format!(
            "{} {}",
            tr_status_uptime_label(lang),
            uptime_label(snap.uptime_secs, lang)
        )]))
}

fn render_cpu_card(snap: &StatusSnapshot, history: &[f32], root: &Root) -> Div {
    let lang = root.language;
    let temp_chip = snap.thermal.cpu_temp.map(|t| format!("{t:.0} °C"));
    status_card()
        .child(card_header(
            icon_pulse(MUTED, 16.),
            tr_status_card_cpu(lang),
            temp_chip,
        ))
        .child(big_number(
            format!("{:.0}", snap.cpu_usage),
            "%".to_string(),
        ))
        .child(history_bars(history))
        .child(card_footer(vec![format!(
            "{} × {}",
            tr_status_cores(lang),
            snap.core_count
        )]))
}

/// GPU 卡片。刻意和 CPU 卡片同构（大数字 + 柱状历史 + 脚注），因为两者
/// 是同一类东西——用户扫一眼就该知道该看哪个数字，不需要重新学一种排版。
///
/// 笔记本普遍是核显 + 独显：多于一张时底部换成切换按钮，卡片显示选中那张
/// 的读数。按钮顶掉的是脚注（型号名 + 渲染器占用）——按钮上已经有厂商名，
/// 同一行位置再写一遍型号是重复信息。
fn render_gpu_card(
    gpus: &[crate::core::status::GpuReading],
    selected: usize,
    history: &[f32],
    root: &Root,
    cx: &mut Context<Root>,
) -> Div {
    let lang = root.language;
    let gpu = &gpus[selected];
    // 徽章位只有一个：温度比显存更值得放在那儿（和 CPU 卡片同一套语言，
    // 一眼扫过去两张卡的右上角都是温度）。拿不到温度才退回显存。
    let chip = match gpu.temp_c {
        Some(t) => Some(format!("{t:.0} °C")),
        None => gpu
            .vram_in_use
            .map(|bytes| format!("{} {}", tr_status_vram(lang), fmt_mem(bytes))),
    };

    let mut footer = Vec::new();
    if let Some(name) = &gpu.name {
        footer.push(truncate(name, 18).to_string());
    }
    if let Some(r) = gpu.renderer_utilization {
        footer.push(format!("{} {r:.0}%", tr_status_renderer(lang)));
    }
    // 温度占了徽章位时，显存挪到脚注，别丢信息。
    if gpu.temp_c.is_some() {
        if let Some(bytes) = gpu.vram_in_use {
            footer.push(format!("{} {}", tr_status_vram(lang), fmt_mem(bytes)));
        }
    }
    if footer.is_empty() {
        footer.push(tr_status_no_gpu(lang).to_string());
    }

    status_card()
        .child(card_header(
            icon_gpu(MUTED, 16.),
            tr_status_card_gpu(lang),
            chip,
        ))
        // 读不到利用率的卡不会进这张表（见平台层），unwrap_or 只是兜底。
        .child(big_number(
            format!("{:.0}", gpu.utilization.unwrap_or(0.0)),
            "%".to_string(),
        ))
        .child(history_bars(history))
        .map(|card| {
            if gpus.len() > 1 {
                card.child(gpu_switch_buttons(gpus, selected, cx))
            } else {
                card.child(card_footer(footer))
            }
        })
}

/// 多显卡时的切换按钮，样式与风扇档位按钮一致——两者都是「同一张卡上的
/// 互斥选择」，长得一样用户就不用重新学。
fn gpu_switch_buttons(
    gpus: &[crate::core::status::GpuReading],
    selected: usize,
    cx: &mut Context<Root>,
) -> Div {
    let mut row = div().mt_3().flex().gap_2();
    for ((index, gpu), label) in gpus.iter().enumerate().zip(gpu_labels(gpus)) {
        let is_active = index == selected;
        let id = gpu.id.clone();
        row = row.child(
            div()
                .id(SharedString::from(format!("gpu-{}", gpu.id)))
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .py(px(6.))
                .rounded_lg()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_center()
                .cursor_pointer()
                .when(is_active, |d| {
                    d.bg(rgb(PRIMARY)).text_color(rgb(ON_PRIMARY))
                })
                .when(!is_active, |d| {
                    d.bg(rgb(SURF_LOW))
                        .text_color(rgb(MUTED))
                        .hover(|h| h.bg(rgb(SURF_HIGH)))
                })
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_gpu(id.clone(), cx);
                })),
        );
    }
    row
}

/// 电池电量对应的强调色：低电量要显眼，充电中回到主色。
fn battery_color(percent: f32, charging: bool) -> u32 {
    if charging || percent >= 40.0 {
        PRIMARY
    } else if percent >= 20.0 {
        CAUTION
    } else {
        ERROR
    }
}

/// 电池卡片：电量 + 充放电状态 + 循环次数 + 健康度。
///
/// 排版沿用内存卡片那一套（大数字 + 进度条 + 脚注），差别只在进度条用状态色，
/// 以及中间多一行状态说明——「已接通电源但没在充」这种情况不写清楚，用户
/// 会以为充电坏了（实际是 macOS 的优化充电策略）。
fn render_battery_card(bat: &crate::core::status::BatteryReading, root: &Root) -> Div {
    let lang = root.language;
    let accent = battery_color(bat.percent, bat.charging);
    let mut state_line =
        vec![tr_battery_state(lang, bat.charging, bat.external, bat.fully_charged).to_string()];
    if let Some(min) = bat.minutes_remaining {
        if !bat.fully_charged {
            state_line.push(tr_battery_time(lang, min, bat.charging));
        }
    }

    let mut footer = Vec::new();
    if let Some(cycles) = bat.cycle_count {
        footer.push(match bat.design_cycle_count {
            Some(max) => format!("{} {cycles}/{max}", tr_status_cycles(lang)),
            None => format!("{} {cycles}", tr_status_cycles(lang)),
        });
    }
    if let Some(h) = bat.health_percent {
        footer.push(format!("{} {h:.0}%", tr_status_battery_health(lang)));
    }

    status_card()
        .child(card_header(
            icon_battery(accent, bat.percent / 100.0, 16.),
            tr_status_card_battery(lang),
            bat.temp_c.map(|t| format!("{t:.0} °C")),
        ))
        .child(big_number(format!("{:.0}", bat.percent), "%".to_string()))
        .child(
            div()
                .mt_3()
                .child(meter_colored(bat.percent / 100.0, accent)),
        )
        .child(
            div()
                .mt_2()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(state_line.join(" · ")),
        )
        .child(card_footer(footer))
}

fn render_memory_card(snap: &StatusSnapshot, root: &Root) -> Div {
    let lang = root.language;
    let ratio = if snap.mem_total > 0 {
        snap.mem_used as f32 / snap.mem_total as f32
    } else {
        0.0
    };
    let mut footer = vec![
        format!("{} {:.0}%", tr_status_used(lang), ratio * 100.0),
        format!("{} {}", tr_status_total(lang), fmt_mem(snap.mem_total)),
    ];
    if snap.swap_total > 0 {
        footer.push(format!(
            "{} {}",
            tr_status_swap(lang),
            fmt_mem(snap.swap_used)
        ));
    }
    status_card()
        .child(card_header(
            icon_ram(MUTED, 16.),
            tr_status_card_memory(lang),
            None,
        ))
        .child(big_number(
            fmt_mem(snap.mem_used),
            tr_status_used(lang).to_string(),
        ))
        .child(div().mt_3().child(meter(ratio)))
        .child(card_footer(footer))
}

fn render_disk_card(root: &Root) -> Div {
    let lang = root.language;
    let (total, free) = root.disk.volume_space(&root.disk.volume).unwrap_or((0, 0));
    let used = total.saturating_sub(free);
    let ratio = if total > 0 {
        used as f32 / total as f32
    } else {
        0.0
    };
    status_card()
        .child(card_header(
            icon_disk(MUTED, 16.),
            tr_status_card_disk(lang),
            (total > 0).then(|| format!("{} {}", tr_status_total(lang), fmt_size(total))),
        ))
        .child(big_number(
            fmt_size(free),
            tr_status_available(lang).to_string(),
        ))
        .child(div().mt_3().child(meter(ratio)))
        .child(card_footer(vec![
            format!("{} {}", tr_status_used(lang), fmt_size(used)),
            format!("{:.0}%", ratio * 100.0),
        ]))
}

fn render_network_card(snap: &StatusSnapshot, root: &Root) -> Div {
    let lang = root.language;
    let rate = |bps: f64| format!("{}/s", fmt_size(bps as u64));
    status_card()
        .child(card_header(
            icon_globe(MUTED, 16.),
            tr_status_card_network(lang),
            None,
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .mt_1()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(PRIMARY))
                                .child("↓"),
                        )
                        .child(
                            div()
                                .text_base()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(TEXT))
                                .child(rate(snap.rx_bps)),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(OUTLINE))
                                .child("↑"),
                        )
                        .child(
                            div()
                                .text_base()
                                .text_color(rgb(MUTED))
                                .child(rate(snap.tx_bps)),
                        ),
                ),
        )
        .child(card_footer(vec![tr_status_net_live(lang).to_string()]))
}

/// 风扇档位按钮（自动 / 降温 / 全速），当前档位高亮，切换中禁用。
fn fan_mode_buttons(root: &Root, has_temp: bool, cx: &mut Context<Root>) -> Div {
    let lang = root.language;
    let active = root.monitor.fan_mode;
    let applying = root.monitor.fan_applying;
    // 「降温」档靠温度联动升档来保证不会低于系统所需（见
    // `platform::macos::status::effective_duty`），所以读不到温度的机型上
    // 干脆不提供这一档，而不是给一个没有安全保证的固定转速。
    let mut modes = vec![(FanMode::Auto, tr_fan_mode_auto(lang))];
    if has_temp {
        modes.push((FanMode::Percent(60), tr_fan_mode_cool(lang)));
    }
    modes.push((FanMode::Percent(100), tr_fan_mode_full(lang)));
    let mut row = div().mt_3().flex().gap_2();
    for (mode, label) in modes {
        let is_active = active == mode;
        row = row.child(
            div()
                .id(SharedString::from(format!(
                    "fan-{}",
                    match mode {
                        FanMode::Auto => "auto".to_string(),
                        FanMode::Percent(p) => p.to_string(),
                    }
                )))
                .flex_1()
                .py(px(6.))
                .rounded_lg()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_center()
                .cursor_pointer()
                .when(is_active, |d| {
                    d.bg(rgb(PRIMARY)).text_color(rgb(ON_PRIMARY))
                })
                .when(!is_active && !applying, |d| {
                    d.bg(rgb(SURF_LOW))
                        .text_color(rgb(MUTED))
                        .hover(|h| h.bg(rgb(SURF_HIGH)))
                })
                .when(applying && !is_active, |d| {
                    d.bg(rgb(SURF_LOW)).text_color(rgba(MUTED, 0.5))
                })
                .child(label.to_string())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.apply_fan_mode(mode, cx);
                })),
        );
    }
    row
}

fn render_fan_card(snap: &StatusSnapshot, root: &Root, cx: &mut Context<Root>) -> Div {
    let lang = root.language;
    let fan = snap.thermal.fans.first();
    let temp_chip = snap.thermal.cpu_temp.map(|t| format!("{t:.0} °C"));
    status_card()
        .child(card_header(
            icon_fan(MUTED, 16.),
            tr_status_card_fan(lang),
            temp_chip,
        ))
        .child(if let Some(fan) = fan {
            big_number(commas(fan.rpm.round() as u64), "RPM".to_string())
        } else {
            div()
                .text_base()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(rgb(OUTLINE))
                .child(tr_status_no_fan(lang).to_string())
        })
        // Windows 只能读转速、改不了档位（见 `platform::fan_control_supported`）。
        // 画出来再点一次报错，不如根本不画。
        .when(
            !snap.thermal.fans.is_empty() && crate::platform::fan_control_supported(),
            |d| d.child(fan_mode_buttons(root, snap.thermal.cpu_temp.is_some(), cx)),
        )
        // 装了特权守护进程才给移除入口：没装的机器上这一行毫无意义。
        .when(root.monitor.fan_helper_installed, |d| {
            let disabled = root.monitor.fan_applying;
            d.child(
                div().mt_2().flex().child(
                    div()
                        .id("fan-helper-remove")
                        .px_2()
                        .py(px(3.))
                        .rounded_md()
                        .border_1()
                        .border_color(rgba(OUTLINE_VAR, 0.7))
                        .bg(rgb(SURF_LOW))
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(tr_fan_helper_remove(lang).to_string())
                        .when(!disabled, |d| {
                            d.cursor_pointer()
                                .hover(|h| {
                                    h.bg(rgb(ERROR_CONTAINER))
                                        .border_color(rgba(ERROR, 0.5))
                                        .text_color(rgb(ERROR))
                                })
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.uninstall_fan_helper(cx)),
                                )
                        })
                        .when(disabled, |d| d.text_color(rgba(MUTED, 0.5))),
                ),
            )
        })
        .child(card_footer(
            [
                format!("{} {}", tr_status_fan_count(lang), snap.thermal.fans.len()),
                if root.monitor.fan_mode == FanMode::Auto {
                    tr_status_fan_managed(lang).to_string()
                } else {
                    tr_status_fan_taken_over(lang).to_string()
                },
            ]
            .into_iter()
            .collect(),
        ))
}

/// 进程行的图标：优先用进程所属应用的真实图标，缓存未命中（还没提取完、
/// 或者这个进程压根不属于任何 .app）时回退到首字母方块。
///
/// 图标是异步提取的，第一拍多半还没就绪——回退块和图标同尺寸同位置，
/// 换上真图标时行高不跳。
/// 进程图标渲染：优先用进程真实图标，若提取中或无图标则回退到首字母色块。
fn process_icon_from_source(
    icon_source: Option<&std::path::Path>,
    name: &str,
    pid: u32,
    size: f32,
) -> AnyElement {
    if let Some(image) = icon_source.and_then(crate::ui::app_icons::try_get_icon) {
        return img(ImageSource::from(image))
            .id(SharedString::from(format!("proc-icon-{pid}")))
            .w(px(size))
            .h(px(size))
            .rounded(px(3.))
            .flex_none()
            .into_any_element();
    }
    div()
        .w(px(size))
        .h(px(size))
        .flex_none()
        .rounded(px(3.))
        .bg(rgb(SURF_LOW))
        .border_1()
        .border_color(rgba(OUTLINE_VAR, 0.5))
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(rgb(MUTED))
        .child(name.chars().take(2).collect::<String>().to_uppercase())
        .into_any_element()
}

/// 格式化进程启动时间（本地时间 + 相对时长）
fn fmt_proc_start_time(start_time: u64, lang: Language) -> String {
    if start_time == 0 {
        return "-".into();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let ago_secs = now.saturating_sub(start_time);
    let ago_str = if ago_secs < 60 {
        match lang {
            Language::Zh => "刚刚".into(),
            Language::En => "just now".into(),
        }
    } else if ago_secs < 3600 {
        match lang {
            Language::Zh => format!("{}分钟前", ago_secs / 60),
            Language::En => format!("{}m ago", ago_secs / 60),
        }
    } else if ago_secs < 86400 {
        match lang {
            Language::Zh => format!("{}小时前", ago_secs / 3600),
            Language::En => format!("{}h ago", ago_secs / 3600),
        }
    } else {
        match lang {
            Language::Zh => format!("{}天前", ago_secs / 86400),
            Language::En => format!("{}d ago", ago_secs / 86400),
        }
    };
    if let Some(dt) = chrono::DateTime::from_timestamp(start_time as i64, 0) {
        let local = dt.with_timezone(&chrono::Local);
        match lang {
            Language::Zh => format!("{} · {}", local.format("%m月%d日 %H:%M"), ago_str),
            Language::En => format!("{} · {}", local.format("%b %d, %H:%M"), ago_str),
        }
    } else {
        ago_str
    }
}

/// 本地化进程状态文本
fn localize_proc_status(status_text: &str, lang: Language) -> &'static str {
    match status_text {
        "Run" | "Running" => match lang {
            Language::Zh => "运行中",
            Language::En => "Running",
        },
        "Sleep" | "Sleeping" => match lang {
            Language::Zh => "休眠",
            Language::En => "Sleeping",
        },
        "Idle" => match lang {
            Language::Zh => "空闲",
            Language::En => "Idle",
        },
        "Stop" | "Stopped" => match lang {
            Language::Zh => "已暂停",
            Language::En => "Stopped",
        },
        "Zombie" => match lang {
            Language::Zh => "僵尸",
            Language::En => "Zombie",
        },
        _ => match lang {
            Language::Zh => "正常",
            Language::En => "Normal",
        },
    }
}

/// 迷你 CPU 趋势条形波形图（24 点）
fn render_mini_sparkline(history: &[f32], max_val: f32, color: u32, height: f32) -> AnyElement {
    const COUNT: usize = 24;
    let mut bars = div().flex().items_end().gap(px(2.)).h(px(height)).w_full();
    for i in 0..COUNT {
        let val = if i + history.len() >= COUNT {
            history[i + history.len() - COUNT]
        } else {
            0.0
        };
        let ratio = if max_val > 0.0 {
            (val / max_val).clamp(0.08, 1.0)
        } else {
            0.08
        };
        let h = (height * ratio).max(2.5);
        bars = bars.child(
            div()
                .flex_1()
                .h(px(h))
                .rounded(px(1.5))
                .bg(rgba(color, 0.4 + 0.6 * ratio)),
        );
    }
    bars.into_any_element()
}

/// 迷你内存趋势条形波形图（24 点）
fn render_mini_mem_sparkline(history: &[u64], color: u32, height: f32) -> AnyElement {
    const COUNT: usize = 24;
    let max_bytes = history.iter().copied().max().unwrap_or(1).max(1) as f32;
    let mut bars = div().flex().items_end().gap(px(2.)).h(px(height)).w_full();
    for i in 0..COUNT {
        let val = if i + history.len() >= COUNT {
            history[i + history.len() - COUNT] as f32
        } else {
            0.0
        };
        let ratio = (val / max_bytes).clamp(0.08, 1.0);
        let h = (height * ratio).max(2.5);
        bars = bars.child(
            div()
                .flex_1()
                .h(px(h))
                .rounded(px(1.5))
                .bg(rgba(color, 0.4 + 0.6 * ratio)),
        );
    }
    bars.into_any_element()
}

/// 详细信息键值对单行
fn detail_kv(label: &str, value: &str) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .text_xs()
        .py(px(2.))
        .child(div().text_color(rgb(OUTLINE)).child(label.to_string()))
        .child(
            div()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(rgb(TEXT))
                .child(value.to_string()),
        )
}

/// 渲染端口胶囊徽章
fn render_ports_badge(ports: &[u16]) -> Option<Div> {
    if ports.is_empty() {
        return None;
    }
    let mut badges = div().flex().items_center().gap_1();
    for &p in ports.iter().take(2) {
        badges = badges.child(
            div()
                .px(px(4.))
                .py(px(0.5))
                .rounded(px(3.))
                .bg(rgba(PRIMARY, 0.12))
                .text_color(rgb(PRIMARY))
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(format!(":{p}")),
        );
    }
    if ports.len() > 2 {
        badges = badges.child(
            div()
                .px(px(3.))
                .py(px(0.5))
                .rounded(px(3.))
                .bg(rgb(SURF_LOW))
                .text_color(rgb(MUTED))
                .text_xs()
                .child(format!("+{}", ports.len() - 2)),
        );
    }
    Some(badges)
}

/// 进程表左右内边距。表头要按它对齐，所以取成常量。
const TABLE_PAD_X: f32 = 16.0;

/// 进程表的行高。`uniform_list` 要求等高，紧凑排布（32px）避免大字老年机感。
const PROC_ROW_H: f32 = 32.0;
const PROC_CPU_COL_W: f32 = 72.0;
const PROC_MEM_COL_W: f32 = 80.0;
const PROC_THREADS_COL_W: f32 = 54.0;
const PROC_CPU_TIME_COL_W: f32 = 74.0;
const PROC_PID_COL_W: f32 = 58.0;
const PROC_USER_COL_W: f32 = 82.0;

/// 可排序的列表头。当前排序列显示方向箭头，点击切换。
fn sort_header(
    label: String,
    key: ProcSortKey,
    width: Option<f32>,
    sort: ProcSort,
    cx: &mut Context<Root>,
) -> Stateful<Div> {
    let active = sort.key == key;
    let numeric = width.is_some();
    let base = div().id(SharedString::from(format!("proc-sort-{key:?}")));
    let base = match width {
        Some(w) => base.w(px(w)).flex_none().justify_end(),
        None => base.flex_1().min_w(px(0.)).justify_start(),
    };
    base.flex()
        .items_center()
        .gap(px(2.))
        .text_xs()
        .font_weight(if active {
            gpui::FontWeight::BOLD
        } else {
            gpui::FontWeight::MEDIUM
        })
        .text_color(rgb(match (active, numeric) {
            (true, _) => PRIMARY,
            (false, true) => OUTLINE,
            (false, false) => TEXT,
        }))
        .cursor_pointer()
        .hover(|h| h.text_color(rgb(if active { PRIMARY } else { MUTED })))
        .child(label)
        .when(active, |d| d.child(if sort.desc { " ↓" } else { " ↑" }))
        .on_click(cx.listener(move |this, _, _, cx| this.sort_processes(key, cx)))
}

/// 进程行渲染：支持 GroupHeader (折叠父组)、GroupChild (展开子项)、Single (单进程)。
fn render_process_row(
    root: &Root,
    item: &crate::ui::state::ProcDisplayItem,
    snap: &StatusSnapshot,
    cx: &mut Context<Root>,
) -> AnyElement {
    let selected_pid = root.monitor.selected_proc_pid;

    match item {
        crate::ui::state::ProcDisplayItem::GroupHeader {
            group_key,
            name,
            icon_source,
            count,
            expanded,
            total_cpu,
            total_mem,
            total_threads,
            total_cpu_time_ms,
            representative_pid,
            user,
            child_indices: _,
        } => {
            let pid = *representative_pid;
            let is_selected = selected_pid == Some(pid);
            let gkey = group_key.clone();
            let expanded_val = *expanded;

            div()
                .id(SharedString::from(format!("grp-row-{gkey}")))
                .h(px(PROC_ROW_H))
                .w_full()
                .flex()
                .items_center()
                .px(px(TABLE_PAD_X))
                .border_b_1()
                .border_color(rgba(OUTLINE_VAR, 0.35))
                .cursor_pointer()
                .when(is_selected, |d| d.bg(rgba(PRIMARY, 0.15)))
                .when(!is_selected, |d| d.hover(|h| h.bg(rgb(SURF_LOW))))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_proc_item(pid, cx);
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .min_w(px(0.))
                        .flex_1()
                        .child(
                            // 展开/折叠箭头
                            div()
                                .id(SharedString::from(format!("toggle-grp-{gkey}")))
                                .w(px(16.))
                                .h(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_sm()
                                .cursor_pointer()
                                .hover(|h| h.bg(rgb(SURF_HIGH)))
                                .child(if expanded_val {
                                    icon_chevron_down(MUTED, 10.)
                                } else {
                                    icon_chevron_right(MUTED, 10.)
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_proc_group(gkey.clone(), cx);
                                    cx.stop_propagation();
                                })),
                        )
                        .child(process_icon_from_source(
                            icon_source.as_deref(),
                            name,
                            pid,
                            18.,
                        ))
                        .child(
                            div()
                                .min_w(px(0.))
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(rgb(TEXT))
                                        .child(truncate(name, 28).to_string()),
                                )
                                .child(
                                    div()
                                        .px_1p5()
                                        .py(px(0.5))
                                        .rounded_md()
                                        .bg(rgba(OUTLINE_VAR, 0.5))
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(rgb(MUTED))
                                        .child(count.to_string()),
                                ),
                        ),
                )
                .child(cpu_cell(*total_cpu, PROC_CPU_COL_W))
                .child(
                    div()
                        .w(px(PROC_MEM_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(TEXT))
                        .child(fmt_mem(*total_mem)),
                )
                .child(
                    div()
                        .w(px(PROC_THREADS_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(total_threads.to_string()),
                )
                .child(
                    div()
                        .w(px(PROC_CPU_TIME_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(crate::core::status::fmt_cpu_time(*total_cpu_time_ms)),
                )
                .child(
                    div()
                        .w(px(PROC_PID_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child("-"),
                )
                .child(
                    div()
                        .w(px(PROC_USER_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(truncate(user.as_deref().unwrap_or("-"), 12).to_string()),
                )
                .into_any_element()
        }
        crate::ui::state::ProcDisplayItem::GroupChild { process_index, .. } => {
            let Some(proc) = snap.processes.get(*process_index) else {
                return div().into_any_element();
            };
            let pid = proc.pid;
            let is_selected = selected_pid == Some(pid);

            let ports_badge = render_ports_badge(&proc.ports);

            div()
                .id(SharedString::from(format!("child-row-{pid}")))
                .h(px(PROC_ROW_H))
                .w_full()
                .flex()
                .items_center()
                .px(px(TABLE_PAD_X))
                .border_b_1()
                .border_color(rgba(OUTLINE_VAR, 0.35))
                .cursor_pointer()
                .when(is_selected, |d| d.bg(rgba(PRIMARY, 0.15)))
                .when(!is_selected, |d| d.hover(|h| h.bg(rgb(SURF_LOW))))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_proc_item(pid, cx);
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .min_w(px(0.))
                        .flex_1()
                        .child(
                            // 缩进短线指示
                            div()
                                .w(px(16.))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .w(px(2.))
                                        .h(px(8.))
                                        .rounded_full()
                                        .bg(rgba(OUTLINE_VAR, 0.6)),
                                ),
                        )
                        .child(process_icon_from_source(
                            proc.icon_source.as_deref(),
                            &proc.name,
                            pid,
                            16.,
                        ))
                        .child(
                            div()
                                .min_w(px(0.))
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::NORMAL)
                                        .text_color(rgb(TEXT))
                                        .child(truncate(&proc.name, 30).to_string()),
                                )
                                .children(ports_badge),
                        ),
                )
                .child(cpu_cell(proc.cpu, PROC_CPU_COL_W))
                .child(
                    div()
                        .w(px(PROC_MEM_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(TEXT))
                        .child(fmt_mem(proc.mem_bytes)),
                )
                .child(
                    div()
                        .w(px(PROC_THREADS_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(proc.threads.to_string()),
                )
                .child(
                    div()
                        .w(px(PROC_CPU_TIME_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(crate::core::status::fmt_cpu_time(proc.cpu_time_ms)),
                )
                .child(
                    div()
                        .w(px(PROC_PID_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(proc.pid.to_string()),
                )
                .child(
                    div()
                        .w(px(PROC_USER_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(truncate(proc.user.as_deref().unwrap_or("-"), 12).to_string()),
                )
                .into_any_element()
        }
        crate::ui::state::ProcDisplayItem::Single { process_index } => {
            let Some(proc) = snap.processes.get(*process_index) else {
                return div().into_any_element();
            };
            let pid = proc.pid;
            let is_selected = selected_pid == Some(pid);

            let ports_badge = render_ports_badge(&proc.ports);

            div()
                .id(SharedString::from(format!("single-row-{pid}")))
                .h(px(PROC_ROW_H))
                .w_full()
                .flex()
                .items_center()
                .px(px(TABLE_PAD_X))
                .border_b_1()
                .border_color(rgba(OUTLINE_VAR, 0.35))
                .cursor_pointer()
                .when(is_selected, |d| d.bg(rgba(PRIMARY, 0.15)))
                .when(!is_selected, |d| d.hover(|h| h.bg(rgb(SURF_LOW))))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_proc_item(pid, cx);
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .min_w(px(0.))
                        .flex_1()
                        .child(div().w(px(16.)).flex_none()) // 对齐占位
                        .child(process_icon_from_source(
                            proc.icon_source.as_deref(),
                            &proc.name,
                            pid,
                            18.,
                        ))
                        .child(
                            div()
                                .min_w(px(0.))
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(rgb(TEXT))
                                        .child(truncate(&proc.name, 32).to_string()),
                                )
                                .children(ports_badge),
                        ),
                )
                .child(cpu_cell(proc.cpu, PROC_CPU_COL_W))
                .child(
                    div()
                        .w(px(PROC_MEM_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(TEXT))
                        .child(fmt_mem(proc.mem_bytes)),
                )
                .child(
                    div()
                        .w(px(PROC_THREADS_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(proc.threads.to_string()),
                )
                .child(
                    div()
                        .w(px(PROC_CPU_TIME_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(crate::core::status::fmt_cpu_time(proc.cpu_time_ms)),
                )
                .child(
                    div()
                        .w(px(PROC_PID_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(proc.pid.to_string()),
                )
                .child(
                    div()
                        .w(px(PROC_USER_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(truncate(proc.user.as_deref().unwrap_or("-"), 12).to_string()),
                )
                .into_any_element()
        }
    }
}

/// 侧滑进程详情抽屉面板（对标 Pulse 设计）
fn render_proc_detail_drawer(
    root: &Root,
    proc: &crate::core::status::ProcInfo,
    cx: &mut Context<Root>,
) -> Div {
    let lang = root.language;
    let pid = proc.pid;
    let start_time = proc.start_time;
    let unique_id = proc.unique_id;
    let name = proc.name.clone();

    div()
        .w(px(330.))
        .flex_none()
        .h_full()
        .bg(rgb(CARD))
        .border_l_1()
        .border_color(rgba(OUTLINE_VAR, 0.4))
        .flex()
        .flex_col()
        .overflow_hidden()
        .child(
            div()
                .flex_1()
                .min_h(px(0.))
                .overflow_hidden()
                .p_4()
                .flex()
                .flex_col()
                .gap_3()
                // 顶部 Header：大图标 + 标题 + 副标题 + 关闭按钮
                .child(
                    div()
                        .flex()
                        .items_start()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2p5()
                                .min_w(px(0.))
                                .child(process_icon_from_source(
                                    proc.icon_source.as_deref(),
                                    &proc.name,
                                    pid,
                                    36.,
                                ))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .min_w(px(0.))
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .text_color(rgb(TEXT))
                                                .truncate()
                                                .child(proc.name.clone()),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(rgb(MUTED))
                                                .truncate()
                                                .child(format!(
                                                    "PID {} · {}",
                                                    proc.pid,
                                                    proc.user.as_deref().unwrap_or("-")
                                                )),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .id("btn-close-detail")
                                .w(px(22.))
                                .h(px(22.))
                                .rounded_md()
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .hover(|h| h.bg(rgb(SURF_HIGH)))
                                .child(icon_close(MUTED, 14.))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.close_proc_detail(cx);
                                })),
                        ),
                )
                // 快捷操作栏：退出 / 强制退出 / 打开位置
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child({
                            let n = name.clone();
                            div()
                                .id("btn-detail-quit")
                                .flex_1()
                                .py_1()
                                .rounded_md()
                                .bg(rgb(SURF_LOW))
                                .border_1()
                                .border_color(rgba(OUTLINE_VAR, 0.6))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_xs()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgb(TEXT))
                                .cursor_pointer()
                                .hover(|h| h.bg(rgb(SURF_HIGH)))
                                .child(tr_status_proc_exit(lang))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.request_kill_process(
                                        pid,
                                        start_time,
                                        unique_id,
                                        n.clone(),
                                        cx,
                                    );
                                }))
                        })
                        .child({
                            let n = name.clone();
                            div()
                                .id("btn-detail-force-quit")
                                .flex_1()
                                .py_1()
                                .rounded_md()
                                .bg(rgba(ERROR, 0.1))
                                .border_1()
                                .border_color(rgba(ERROR, 0.35))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_xs()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgb(ERROR))
                                .cursor_pointer()
                                .hover(|h| h.bg(rgba(ERROR, 0.2)))
                                .child(tr_status_proc_force_exit(lang))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.force_kill_process(
                                        pid,
                                        start_time,
                                        unique_id,
                                        n.clone(),
                                        cx,
                                    );
                                }))
                        })
                        .when(proc.exe_path.is_some(), |d| {
                            let path_buf =
                                std::path::PathBuf::from(proc.exe_path.as_ref().unwrap());
                            d.child(
                                div()
                                    .id("btn-detail-reveal")
                                    .w(px(28.))
                                    .h(px(28.))
                                    .rounded_md()
                                    .bg(rgb(SURF_LOW))
                                    .border_1()
                                    .border_color(rgba(OUTLINE_VAR, 0.6))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .hover(|h| h.bg(rgb(SURF_HIGH)))
                                    .child(icon_folder(MUTED, 13.))
                                    .tooltip(text_tooltip(tr_status_proc_reveal(lang)))
                                    .on_click(cx.listener(move |_, _, _, _| {
                                        crate::platform::reveal_in_explorer(&path_buf);
                                    })),
                            )
                        }),
                )
                // CPU 性能卡片与走势
                .child(
                    div()
                        .rounded_lg()
                        .bg(rgba(PRIMARY, 0.08))
                        .border_1()
                        .border_color(rgba(PRIMARY, 0.18))
                        .p_2p5()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(PRIMARY))
                                        .child("CPU"),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(TEXT))
                                        .child(format!("{:.1}%", proc.cpu)),
                                ),
                        )
                        .child(render_mini_sparkline(
                            &root.monitor.selected_proc_cpu_history,
                            100.0,
                            PRIMARY,
                            24.,
                        )),
                )
                // 内存性能卡片与走势
                .child(
                    div()
                        .rounded_lg()
                        .bg(rgba(0x10B981, 0.08))
                        .border_1()
                        .border_color(rgba(0x10B981, 0.2))
                        .p_2p5()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(0x059669))
                                        .child(tr_status_card_memory(lang)),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(TEXT))
                                        .child(fmt_mem(proc.mem_bytes)),
                                ),
                        )
                        .child(render_mini_mem_sparkline(
                            &root.monitor.selected_proc_mem_history,
                            0x10B981,
                            24.,
                        )),
                )
                // 详细信息列表
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .pt_1()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(MUTED))
                                .mb_1()
                                .child(tr_status_proc_detail_title(lang)),
                        )
                        .child(detail_kv(
                            tr_th_cpu_time(lang),
                            &crate::core::status::fmt_cpu_time(proc.cpu_time_ms),
                        ))
                        .child(detail_kv(tr_th_threads(lang), &proc.threads.to_string()))
                        .child(detail_kv(
                            tr_status_proc_rss(lang),
                            &fmt_mem(proc.mem_bytes),
                        ))
                        .child(detail_kv(
                            tr_status_proc_vms(lang),
                            &fmt_mem(proc.virtual_mem_bytes),
                        ))
                        .child(detail_kv(
                            tr_status_proc_disk_read(lang),
                            &format!(
                                "{} · {}",
                                fmt_size(proc.disk_read_bps as u64) + "/s",
                                fmt_mem(proc.disk_read_bytes)
                            ),
                        ))
                        .child(detail_kv(
                            tr_status_proc_disk_write(lang),
                            &format!(
                                "{} · {}",
                                fmt_size(proc.disk_written_bps as u64) + "/s",
                                fmt_mem(proc.disk_written_bytes)
                            ),
                        ))
                        .child(detail_kv(
                            tr_status_proc_start_time(lang),
                            &fmt_proc_start_time(proc.start_time, lang),
                        ))
                        .child(detail_kv(
                            tr_status_proc_status(lang),
                            localize_proc_status(&proc.status_text, lang),
                        ))
                        .when(proc.parent_pid.is_some(), |d| {
                            let ppid = proc.parent_pid.unwrap();
                            let pname = proc.parent_name.as_deref().unwrap_or("process");
                            d.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .text_xs()
                                    .py(px(2.))
                                    .child(
                                        div()
                                            .text_color(rgb(OUTLINE))
                                            .child(tr_status_proc_parent(lang)),
                                    )
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("parent-proc-{ppid}")))
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .text_color(rgb(PRIMARY))
                                            .cursor_pointer()
                                            .hover(|h| h.underline())
                                            .child(icon_folder(PRIMARY, 11.))
                                            .child(format!("{pname} {ppid}"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.select_proc_item(ppid, cx);
                                            })),
                                    ),
                            )
                        }),
                )
                // 命令行卡片
                .when(proc.cmd_line.is_some(), |d| {
                    let cmd = proc.cmd_line.as_ref().unwrap().clone();
                    let cmd_clone = cmd.clone();
                    let copied = root.monitor.cmd_copied_feedback;
                    d.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .pt_1()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(MUTED))
                                            .child(tr_status_proc_cmd(lang)),
                                    )
                                    .child(
                                        div()
                                            .id("btn-copy-cmd")
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .px_1p5()
                                            .py(px(1.))
                                            .rounded_md()
                                            .bg(rgb(SURF_LOW))
                                            .hover(|h| h.bg(rgb(SURF_HIGH)))
                                            .cursor_pointer()
                                            .child(icon_copy(
                                                if copied { PRIMARY } else { MUTED },
                                                11.,
                                            ))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(if copied {
                                                        PRIMARY
                                                    } else {
                                                        MUTED
                                                    }))
                                                    .child(if copied {
                                                        tr_status_proc_copied(lang)
                                                    } else {
                                                        tr_status_proc_copy_cmd(lang)
                                                    }),
                                            )
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.copy_proc_cmd(cmd_clone.clone(), cx);
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .p_2()
                                    .rounded_md()
                                    .bg(rgb(SURF_LOW))
                                    .border_1()
                                    .border_color(rgba(OUTLINE_VAR, 0.4))
                                    .text_xs()
                                    .text_color(rgb(TEXT))
                                    .child(truncate(&cmd, 160).to_string()),
                            ),
                    )
                }),
        )
}

/// 端口搜索匹配条目
struct PortMatchItem<'a> {
    port: u16,
    proc_name: &'a str,
    icon_source: Option<&'a std::path::Path>,
    pid: u32,
    is_system: bool,
}

/// 渲染微型进程图标（用于 Popover 条目内）
fn render_popover_icon(
    _proc_name: &str,
    icon_source: Option<&std::path::Path>,
    pid: u32,
    is_system: bool,
) -> AnyElement {
    const SIZE: f32 = 16.0;
    if let Some(image) = icon_source.and_then(try_get_icon) {
        let src = ImageSource::from(image);
        return img(src)
            .id(SharedString::from(format!("pop-icon-{pid}")))
            .w(px(SIZE))
            .h(px(SIZE))
            .rounded(px(3.))
            .flex_none()
            .into_any_element();
    }
    if is_system {
        div()
            .w(px(SIZE))
            .h(px(SIZE))
            .rounded(px(3.))
            .bg(rgb(SURF_HIGH))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .child(icon_gear(MUTED, 10.))
            .into_any_element()
    } else {
        div()
            .w(px(SIZE))
            .h(px(SIZE))
            .rounded(px(3.))
            .bg(rgba(PRIMARY, 0.12))
            .border_1()
            .border_color(rgba(PRIMARY, 0.25))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .child(icon_apps(PRIMARY, 10.))
            .into_any_element()
    }
}

/// 搜索框正下方的端口智能匹配浮层（Popover）
fn matching_ports<'a>(snap: &'a StatusSnapshot, search_text: &str) -> Vec<PortMatchItem<'a>> {
    // 1. 搜集所有正在监听/打开网络端口的进程条目
    let mut entries: Vec<PortMatchItem> = Vec::new();
    for proc in &snap.processes {
        let is_system = proc.pid <= 4
            || proc.name.eq_ignore_ascii_case("system")
            || proc.name.eq_ignore_ascii_case("registry")
            || proc.name.eq_ignore_ascii_case("smss.exe")
            || proc.name.eq_ignore_ascii_case("csrss.exe")
            || proc.name.eq_ignore_ascii_case("services.exe")
            || proc.name.eq_ignore_ascii_case("svchost.exe")
            || proc.name.eq_ignore_ascii_case("lsass.exe");

        for &port in &proc.ports {
            entries.push(PortMatchItem {
                port,
                proc_name: &proc.name,
                icon_source: proc.icon_source.as_deref(),
                pid: proc.pid,
                is_system,
            });
        }
    }
    // 按端口号升序排列
    entries.sort_by_key(|e| e.port);

    // 2. 判断输入是否含有端口前缀 ':'
    let (is_port_query, sub_query) = if let Some(idx) = search_text.find(':') {
        let sub = search_text[idx + 1..].trim();
        (true, sub)
    } else {
        (false, "")
    };

    // 3. 动态过滤
    if is_port_query && !sub_query.is_empty() {
        entries
            .into_iter()
            .filter(|e| {
                e.port.to_string().starts_with(sub_query)
                    || e.port.to_string().contains(sub_query)
                    || e.proc_name
                        .to_lowercase()
                        .contains(&sub_query.to_lowercase())
            })
            .collect()
    } else {
        entries
    }
}

fn search_syntax_items(lang: Language) -> [(&'static str, &'static str, &'static str); 6] {
    [
        (
            "msedge, \"NVIDIA Overlay\"",
            tr_status_search_hint_multi_desc(lang),
            "msedge, \"NVIDIA Overlay\"",
        ),
        ("cpu >", tr_status_search_hint_cpu_desc(lang), "cpu > "),
        ("mem >", tr_status_search_hint_mem_desc(lang), "mem > "),
        ("user:", tr_status_search_hint_user_desc(lang), "user:"),
        (":3000", tr_status_search_hint_port_desc(lang), ":"),
        (
            "-helper",
            tr_status_search_hint_exclude_desc(lang),
            "-helper",
        ),
    ]
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum CandidateContent {
    #[default]
    Query,
    Syntax,
    Process {
        source: Option<std::path::PathBuf>,
        pid: u32,
        system: bool,
    },
    Port {
        source: Option<std::path::PathBuf>,
        pid: u32,
        system: bool,
        port: u16,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SearchCandidate {
    pub id: String,
    pub query: String,
    pub keep_open: bool,
    label: String,
    detail: String,
    content: CandidateContent,
}

impl PopoverCandidate for SearchCandidate {
    fn id(&self) -> &str {
        &self.id
    }
}

pub(crate) fn status_search_candidates(root: &Root) -> Vec<SearchCandidate> {
    let Some(snap) = root.monitor.snapshot.as_ref() else {
        return Vec::new();
    };
    search_candidates(
        snap,
        &root.monitor.proc_view,
        &root.monitor.input.text,
        root.language,
    )
}

fn search_candidates(
    snap: &StatusSnapshot,
    view: &[usize],
    text: &str,
    lang: Language,
) -> Vec<SearchCandidate> {
    if text.trim().starts_with(':') || text.trim().to_lowercase().starts_with("port:") {
        return matching_ports(snap, text)
            .into_iter()
            .take(80)
            .map(|p| SearchCandidate {
                id: format!("pop-port-{}-{}", p.port, p.pid),
                query: format!(":{}", p.port),
                keep_open: false,
                label: p.proc_name.into(),
                detail: format!("#{}", p.pid),
                content: CandidateContent::Port {
                    source: p.icon_source.map(std::path::Path::to_path_buf),
                    pid: p.pid,
                    system: p.is_system,
                    port: p.port,
                },
            })
            .collect();
    }
    let mut candidates = Vec::new();
    if !text.trim().is_empty() {
        candidates.push(SearchCandidate {
            id: "status-search-current-query".into(),
            query: text.into(),
            keep_open: false,
            label: tr_status_search_query(lang, text),
            detail: String::new(),
            content: CandidateContent::Query,
        });
        candidates.extend(
            process_search_groups(&snap.processes, view)
                .into_iter()
                .take(8)
                .map(|(p, count)| SearchCandidate {
                    id: format!("status-search-process-{}", p.name.to_lowercase()),
                    query: format!("\"{}\"", p.name),
                    keep_open: false,
                    label: p.name.clone(),
                    detail: tr_status_search_process_count(lang, count),
                    content: CandidateContent::Process {
                        source: p.icon_source.clone(),
                        pid: p.pid,
                        system: p.pid <= 4,
                    },
                }),
        );
    }
    candidates.extend(
        search_syntax_items(lang)
            .into_iter()
            .enumerate()
            .filter_map(|(i, (example, desc, query))| {
                syntax_completion(query, text).map(|completion| (i, example, desc, completion))
            })
            .map(|(i, example, desc, query)| SearchCandidate {
                id: format!("syntax-hint-{i}"),
                label: if example.starts_with("mem") || example.starts_with("cpu") {
                    query.trim_end().to_owned()
                } else {
                    example.into()
                },
                query,
                keep_open: true,
                detail: desc.into(),
                content: CandidateContent::Syntax,
            }),
    );
    let lower = text.trim().to_ascii_lowercase();
    if ["mem", "cpu", "user"]
        .iter()
        .any(|field| lower.starts_with(field))
    {
        candidates.sort_by_key(|item| item.content != CandidateContent::Syntax);
    }
    candidates
}

#[cfg(test)]
fn candidate_index(selected: Option<&str>, candidates: &[SearchCandidate]) -> Option<usize> {
    crate::ui::components::popover::candidate_index(selected, candidates)
}

#[cfg(test)]
fn next_search_candidate(
    selected: Option<&str>,
    candidates: &[SearchCandidate],
    backwards: bool,
) -> Option<usize> {
    crate::ui::components::popover::next_candidate(selected, candidates, backwards)
}

fn syntax_completion(template: &str, query: &str) -> Option<String> {
    let compact = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase();
    for field in ["mem", "cpu"] {
        if template.starts_with(field) {
            if let Some(rest) = compact.strip_prefix(field) {
                return match rest {
                    "" => Some(format!("{field} > ")),
                    ">" | "<" | ">=" | "<=" => Some(format!("{field} {rest} ")),
                    _ => None,
                };
            }
        }
    }
    template
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
        .contains(&compact)
        .then(|| template.to_owned())
}

#[cfg(test)]
fn syntax_hint_matches(example: &str, query: &str) -> bool {
    syntax_completion(example, query).is_some()
}

fn process_search_groups<'a>(
    processes: &'a [crate::core::status::ProcInfo],
    view: &[usize],
) -> Vec<(&'a crate::core::status::ProcInfo, usize)> {
    let mut groups = std::collections::BTreeMap::new();
    for proc in view.iter().filter_map(|&i| processes.get(i)) {
        let group = groups.entry(proc.name.to_lowercase()).or_insert((proc, 0));
        group.1 += 1;
    }
    groups.into_values().collect()
}

fn render_status_search_popover(
    root: &Root,
    snap: &StatusSnapshot,
    cx: &mut Context<Root>,
) -> AnyElement {
    let lang = root.language;
    let text = &root.monitor.input.text;
    let port_query =
        text.trim().starts_with(':') || text.trim().to_lowercase().starts_with("port:");
    let candidates = status_search_candidates(root);
    let selected = root.monitor.search_popover.selection(&candidates);
    let rows = candidates
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let mut row = popover_row(
                item.id.clone().into(),
                selected == Some(index),
                |this: &mut Root| &mut this.monitor.search_popover,
                cx,
            );
            match &item.content {
                CandidateContent::Query => {
                    row = row.child(icon_search(PRIMARY, 12.));
                }
                CandidateContent::Syntax => {}
                CandidateContent::Process {
                    source,
                    pid,
                    system,
                } => {
                    row = row.child(render_popover_icon(
                        &item.label,
                        source.as_deref(),
                        *pid,
                        *system,
                    ));
                }
                CandidateContent::Port {
                    source,
                    pid,
                    system,
                    port,
                } => {
                    row = row
                        .child(
                            div()
                                .flex_none()
                                .px_1p5()
                                .rounded_md()
                                .text_xs()
                                .text_color(rgb(PRIMARY))
                                .child(format!(":{port}")),
                        )
                        .child(render_popover_icon(
                            &item.label,
                            source.as_deref(),
                            *pid,
                            *system,
                        ));
                }
            }
            row = row
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_xs()
                        .text_color(rgb(TEXT))
                        .child(item.label.clone()),
                )
                .when(!item.detail.is_empty(), |d| {
                    d.child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(rgb(OUTLINE))
                            .child(item.detail.clone()),
                    )
                });
            row.on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.status_choose_search_candidate(&item.query, item.keep_open, cx);
                    cx.stop_propagation();
                }),
            )
            .into_any_element()
        })
        .collect::<Vec<_>>();
    let empty = rows.is_empty();
    let sub_query = text.split_once(':').map_or("", |(_, q)| q.trim());
    let list = popover_list(
        "status-search-candidates".into(),
        &root.monitor.search_popover,
        240.,
    )
    .children(rows)
    .when(empty, |d| {
        d.child(
            div()
                .px_2()
                .py_4()
                .text_xs()
                .text_color(rgb(OUTLINE))
                .child(if sub_query.is_empty() {
                    tr_status_popover_no_ports(lang).to_string()
                } else {
                    tr_status_popover_no_matching_ports(lang, sub_query)
                }),
        )
    });
    let header = div()
        .flex_none()
        .px_3()
        .py_2()
        .flex()
        .items_center()
        .justify_between()
        .text_xs()
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(rgb(MUTED))
        .child(if port_query {
            tr_status_popover_ports(lang)
        } else {
            tr_status_search_hint_title(lang)
        })
        .when(port_query, |d| {
            d.child(matching_ports(snap, text).len().to_string())
        });
    popover_overlay(
        popover_surface(
            PopoverSpec {
                id: "status-proc-search-popover".into(),
                width: 320.,
                top: 34.,
                max_height: 320.,
                anchor_bounds: root.monitor.input.bounds,
            },
            |this: &mut Root, window, cx| {
                this.monitor.search_popover.dismiss();
                if this.monitor.input.focus_handle.is_focused(window) {
                    window.blur();
                }
                cx.notify();
            },
            cx,
        )
        .child(header)
        .child(list)
        .when(port_query, |d| {
            d.child(
                div()
                    .flex_none()
                    .px_3()
                    .py_2()
                    .text_xs()
                    .text_color(rgb(OUTLINE))
                    .child(tr_status_popover_tip(lang)),
            )
        }),
    )
}

/// 进程表。九百来个进程全量铺开会让整页掉到个位数帧率，所以和软件管理表
/// 一样走 `uniform_list`：只构造视口内的十几行，配一条自绘滚动条。
fn render_process_table(
    root: &Root,
    snap: &StatusSnapshot,
    window: &mut Window,
    cx: &mut Context<Root>,
) -> Div {
    let lang = root.language;
    let sort = root.monitor.proc_sort;
    let row_count = root.monitor.proc_display_items.len();

    let base = root.monitor.proc_scroll.0.borrow().base_handle.clone();
    let metrics = scroll_metrics(&base, STATUS_PROCESS_TABLE_H, row_count as f32 * PROC_ROW_H);
    let scrollbar_el = metrics.map(|m| {
        scrollbar("proc-scroll-thumb", m, |thumb| {
            thumb.on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                    let mouse_y: f32 = event.position.y.into();
                    let start_top: f32 =
                        (-this.monitor.proc_scroll.0.borrow().base_handle.offset().y).into();
                    this.monitor.proc_scroll_drag = Some((mouse_y, start_top.max(0.0)));
                    cx.notify();
                }),
            )
        })
    });

    let list = gpui::uniform_list(
        SharedString::from("status-proc-rows"),
        row_count,
        cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
            let Some(snap) = this.monitor.snapshot.clone() else {
                return Vec::new();
            };
            let picked_items: Vec<crate::ui::state::ProcDisplayItem> = range
                .filter_map(|i| this.monitor.proc_display_items.get(i).cloned())
                .collect();
            // 图标按可见区间懒加载
            let mut icon_paths = Vec::new();
            for item in &picked_items {
                match item {
                    crate::ui::state::ProcDisplayItem::GroupHeader { icon_source, .. } => {
                        if let Some(src) = icon_source {
                            icon_paths.push(src.clone());
                        }
                    }
                    crate::ui::state::ProcDisplayItem::GroupChild { process_index, .. }
                    | crate::ui::state::ProcDisplayItem::Single { process_index } => {
                        if let Some(p) = snap.processes.get(*process_index) {
                            if let Some(src) = &p.icon_source {
                                icon_paths.push(src.clone());
                            }
                        }
                    }
                }
            }
            this.load_visible_process_icons(icon_paths, cx);
            picked_items
                .iter()
                .map(|item| render_process_row(this, item, &snap, cx))
                .collect()
        }),
    )
    .track_scroll(root.monitor.proc_scroll.clone())
    .size_full()
    .when(metrics.is_some(), |l| l.pr(px(SCROLLBAR_W)));

    // 搜索框状态
    let search_focused = root.monitor.input.focus_handle.is_focused(window);
    let search_text = root.monitor.input.text.clone();
    let status_focus_handle = root.monitor.input.focus_handle.clone();
    let search_sel = root.monitor.input.sel.clone();
    let search_marked = root.monitor.input.marked.clone();
    let sel = crate::ui::text_input::clamp_to_boundary(&search_text, search_sel);

    let search_box_el = crate::ui::components::search_box::search_box(
        crate::ui::components::search_box::SearchBoxSpec {
            id: SharedString::from("status-proc-search-box"),
            focus_handle: &status_focus_handle,
            text: &search_text,
            placeholder: SharedString::from(tr_status_proc_search_placeholder(lang)),
            selection: sel,
            marked: search_marked,
            width: 210.,
            height: 28.,
            font_size: 11.5,
            cursor_h: 12.,
            focused: search_focused,
            cursor_visible: root.cursor_blink_visible,
            target: crate::ui::text_input::InputTarget::Status,
        },
        |this, cx| this.status_search_clear(cx),
        |this, cx| this.status_search_backspace(cx),
        |this, cx| this.status_search_escape(cx),
        |this, bounds| {
            this.monitor.input.bounds = Some(bounds);
        },
        cx,
    )
    .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
        if matches!(event.keystroke.key.as_str(), "up" | "down" | "enter")
            && this.monitor.input.marked.is_none()
        {
            this.status_search_candidate_key(&event.keystroke.key, cx);
            cx.stop_propagation();
        }
    }));

    // 分类胶囊按钮组
    let current_tab = root.monitor.filter_tab;
    let filter_tabs = [
        (
            crate::core::status::ProcFilterTab::All,
            tr_status_tab_all(lang),
        ),
        (
            crate::core::status::ProcFilterTab::App,
            tr_status_tab_app(lang),
        ),
        (
            crate::core::status::ProcFilterTab::User,
            tr_status_tab_user(lang),
        ),
        (
            crate::core::status::ProcFilterTab::System,
            tr_status_tab_system(lang),
        ),
    ];
    let tab_buttons: Vec<AnyElement> = filter_tabs
        .into_iter()
        .map(|(tab, label)| {
            let is_active = current_tab == tab;
            div()
                .id(SharedString::from(format!("proc-tab-{tab:?}")))
                .px_2p5()
                .py(px(3.))
                .rounded_md()
                .cursor_pointer()
                .text_xs()
                .font_weight(if is_active {
                    gpui::FontWeight::SEMIBOLD
                } else {
                    gpui::FontWeight::MEDIUM
                })
                .when(is_active, |d| {
                    d.bg(rgb(PRIMARY_FIXED))
                        .border_1()
                        .border_color(rgb(PRIMARY))
                        .text_color(rgb(PRIMARY))
                })
                .when(!is_active, |d| {
                    d.bg(rgb(CARD))
                        .border_1()
                        .border_color(rgba(OUTLINE_VAR, 0.7))
                        .text_color(rgb(MUTED))
                        .hover(|h| h.bg(rgb(SURF_LOW)).text_color(rgb(TEXT)))
                })
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.status_set_filter_tab(tab, cx);
                }))
                .into_any_element()
        })
        .collect();

    let popover_el = root
        .monitor
        .search_popover
        .is_open(search_focused)
        .then(|| render_status_search_popover(root, snap, cx));

    let search_box_container = div()
        .relative()
        .child(search_box_el)
        .children(popover_el)
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                if this.monitor.search_popover.dismissed() {
                    this.monitor.search_popover.reopen();
                    cx.notify();
                }
            }),
        );

    let selected_proc = root
        .monitor
        .selected_proc_pid
        .and_then(|pid| snap.processes.iter().find(|p| p.pid == pid));

    let (filtered_cpu, filtered_mem): (f32, u64) = root
        .monitor
        .proc_view
        .iter()
        .filter_map(|&i| snap.processes.get(i))
        .fold((0.0, 0), |(cpu_acc, mem_acc), p| {
            (cpu_acc + p.cpu, mem_acc + p.mem_bytes)
        });

    let table_pane = div()
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .flex()
        .flex_col()
        .child(
            div()
                .pl(px(TABLE_PAD_X))
                .pr(px(
                    TABLE_PAD_X + if metrics.is_some() { SCROLLBAR_W } else { 0. }
                ))
                .py_2()
                .bg(rgb(SURF_LOW))
                .border_b_1()
                .border_color(rgba(OUTLINE_VAR, 0.4))
                .flex()
                .items_center()
                .child(sort_header(
                    tr_th_proc_name(lang).to_string(),
                    ProcSortKey::Name,
                    None,
                    sort,
                    cx,
                ))
                .child(sort_header(
                    tr_th_cpu_short(lang).to_string(),
                    ProcSortKey::Cpu,
                    Some(PROC_CPU_COL_W),
                    sort,
                    cx,
                ))
                .child(sort_header(
                    tr_th_mem_short(lang).to_string(),
                    ProcSortKey::Memory,
                    Some(PROC_MEM_COL_W),
                    sort,
                    cx,
                ))
                .child(sort_header(
                    tr_th_threads(lang).to_string(),
                    ProcSortKey::Threads,
                    Some(PROC_THREADS_COL_W),
                    sort,
                    cx,
                ))
                .child(sort_header(
                    tr_th_cpu_time(lang).to_string(),
                    ProcSortKey::CpuTime,
                    Some(PROC_CPU_TIME_COL_W),
                    sort,
                    cx,
                ))
                .child(sort_header(
                    tr_th_pid(lang).to_string(),
                    ProcSortKey::Pid,
                    Some(PROC_PID_COL_W),
                    sort,
                    cx,
                ))
                .child(sort_header(
                    tr_th_user(lang).to_string(),
                    ProcSortKey::User,
                    Some(PROC_USER_COL_W),
                    sort,
                    cx,
                )),
        )
        .child(
            div()
                .flex_1()
                .min_h(px(0.))
                .relative()
                .child(list)
                .children(scrollbar_el)
                // 拖拽期间的 move/up 走窗口级监听，鼠标滑出卡片也不会断流
                .child(drag_capture(
                    cx.entity(),
                    |this, mouse_y, cx| {
                        let Some(start) = this.monitor.proc_scroll_drag else {
                            return;
                        };
                        let base = this.monitor.proc_scroll.0.borrow().base_handle.clone();
                        if let Some(new_top) = drag_to_offset(&base, start, mouse_y) {
                            base.set_offset(gpui::point(px(0.0), px(-new_top)));
                            cx.notify();
                        }
                    },
                    |this, cx| {
                        if this.monitor.proc_scroll_drag.take().is_some() {
                            cx.notify();
                        }
                    },
                )),
        )
        // 底部状态栏：统计显示进程数及已筛选进程的总消耗
        .child(
            div()
                .px(px(TABLE_PAD_X))
                .py_1p5()
                .bg(rgb(SURF_LOW))
                .border_t_1()
                .border_color(rgba(OUTLINE_VAR, 0.4))
                .flex()
                .items_center()
                .justify_between()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(tr_status_proc_count_detail(
                    lang,
                    root.monitor.proc_view.len(),
                    snap.process_count,
                ))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(format!("CPU: {:.1}%", filtered_cpu))
                        .child(format!(
                            "{}: {}",
                            tr_status_card_memory(lang),
                            fmt_mem(filtered_mem)
                        )),
                ),
        );

    card()
        .w_full()
        .flex_1()
        .min_h(px(0.))
        .overflow_hidden()
        .flex()
        .flex_col()
        // 顶部操作工具栏：标题与统计在左，搜索框与分类药丸在右
        .child(
            div()
                .relative()
                .px(px(TABLE_PAD_X))
                .py_2p5()
                .bg(rgb(CARD))
                .border_b_1()
                .border_color(rgba(OUTLINE_VAR, 0.3))
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2p5()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(TEXT))
                                .child(tr_status_processes(lang)),
                        )
                        .child(div().text_xs().text_color(rgb(MUTED)).child(
                            tr_status_proc_count_summary(
                                lang,
                                root.monitor.proc_view.len(),
                                snap.process_count,
                            ),
                        )),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(search_box_container)
                        .child(div().flex().items_center().gap_1().children(tab_buttons)),
                ),
        )
        .child(
            div()
                .w_full()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .flex_row()
                .child(table_pane)
                .when_some(selected_proc, |d, p| {
                    d.child(render_proc_detail_drawer(root, p, cx))
                }),
        )
}

/// 状态监控页入口。
pub fn render_status_view(root: &Root, window: &mut Window, cx: &mut Context<Root>) -> AnyElement {
    let lang = root.language;

    let Some(snap) = &root.monitor.snapshot else {
        return div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .child(icon_pulse(PRIMARY, 28.))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(MUTED))
                    .child(tr_status_loading(lang).to_string()),
            )
            .into_any_element();
    };

    match root.monitor.tab {
        StatusTab::Overview => div()
            .id("status-overview-scroll")
            .size_full()
            .min_w(px(0.))
            .overflow_scroll()
            .flex()
            .flex_col()
            .p_5()
            .gap_4()
            .child(page_heading(
                tr_status_overview_heading(lang),
                tr_status_overview_subheading(lang),
            ))
            .child(
                div().flex().flex_col().children(card_rows(
                    [
                        Some(render_health_card(root, snap)),
                        Some(render_cpu_card(snap, &root.monitor.cpu_history, root)),
                        (!snap.gpus.is_empty()).then(|| {
                            let selected = root
                                .monitor
                                .gpu_selected
                                .as_ref()
                                .and_then(|id| snap.gpus.iter().position(|g| &g.id == id))
                                .unwrap_or(0);
                            let history = root
                                .monitor
                                .gpu_history
                                .get(&snap.gpus[selected].id)
                                .map(Vec::as_slice)
                                .unwrap_or(&[]);
                            render_gpu_card(&snap.gpus, selected, history, root, cx)
                        }),
                        Some(render_memory_card(snap, root)),
                        Some(render_disk_card(root)),
                        Some(render_network_card(snap, root)),
                        snap.battery
                            .as_ref()
                            .map(|bat| render_battery_card(bat, root)),
                        Some(render_fan_card(snap, root, cx)),
                    ]
                    .into_iter()
                    .flatten()
                    .collect(),
                )),
            )
            .into_any_element(),
        StatusTab::Processes => {
            let total_threads: u64 = snap.processes.iter().map(|p| p.threads as u64).sum();
            div()
                .id("status-processes-page")
                .size_full()
                .min_w(px(0.))
                .p_5()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div().flex().items_baseline().justify_between().child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(rgb(TEXT))
                                    .child(tr_status_proc_heading(lang)),
                            )
                            .child(div().text_xs().text_color(rgb(MUTED)).child(format!(
                                "{} · {}",
                                tr_status_proc_subheading(lang, snap.process_count),
                                tr_status_thread_count(lang, total_threads),
                            ))),
                    ),
                )
                .child(render_process_table(root, snap, window, cx))
                .into_any_element()
        }
    }
}

#[cfg(test)]
mod search_popover_tests {
    use super::*;

    #[test]
    fn process_suggestions_group_only_filtered_processes() {
        let proc = crate::core::status::ProcInfo {
            pid: 100,
            start_time: 0,
            unique_id: None,
            name: "msedge".into(),
            cpu: 0.,
            mem_bytes: 0,
            icon_source: None,
            ports: vec![],
            threads: 0,
            cpu_time_ms: 0,
            user: None,
            exe_path: None,
            ..Default::default()
        };
        let mut same_name = proc.clone();
        same_name.pid = 101;
        same_name.name = "MSEDGE".into();
        let mut other = proc.clone();
        other.pid = 102;
        other.name = "Memory Compression".into();
        let processes = vec![proc, same_name, other];
        let groups = process_search_groups(&processes, &[0, 1]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].1, 2);
        assert_eq!(groups[0].0.name, "msedge");
        let groups = process_search_groups(&processes, &[2]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0.name, "Memory Compression");
        assert!(process_search_groups(&processes, &[]).is_empty());
    }

    #[test]
    fn popover_keyboard_navigation_wraps_and_survives_filter_changes() {
        let candidates = vec![
            SearchCandidate {
                id: "query".into(),
                query: "mem".into(),
                keep_open: false,
                ..Default::default()
            },
            SearchCandidate {
                id: "process".into(),
                query: "\"Memory Compression\"".into(),
                keep_open: false,
                ..Default::default()
            },
            SearchCandidate {
                id: "syntax".into(),
                query: "mem > 1 GB".into(),
                keep_open: true,
                ..Default::default()
            },
        ];
        assert_eq!(candidate_index(None, &candidates), Some(0));
        assert_eq!(next_search_candidate(None, &candidates, false), Some(1));
        assert_eq!(
            next_search_candidate(Some("query"), &candidates, true),
            Some(2)
        );
        assert_eq!(
            next_search_candidate(Some("syntax"), &candidates, false),
            Some(0)
        );
        assert_eq!(candidate_index(Some("process"), &candidates[1..]), Some(0));
        assert_eq!(
            candidate_index(Some("vanished-process"), &candidates),
            Some(0)
        );
        assert_eq!(next_search_candidate(None, &[], true), None);
        assert_eq!(
            next_search_candidate(None, &candidates[..1], false),
            Some(0)
        );
        let chosen = &candidates[candidate_index(Some("syntax"), &candidates).unwrap()];
        assert_eq!(chosen.query, "mem > 1 GB");
        assert!(chosen.keep_open);
        assert!(!candidates[1].keep_open);
    }

    #[test]
    fn typing_filters_syntax_suggestions() {
        assert!(syntax_hint_matches("mem > 1 GB", "mem"));
        assert!(!syntax_hint_matches("cpu > 10", "mem"));
        assert!(syntax_hint_matches("msedge, \"NVIDIA Overlay\"", "MS"));
        assert!(syntax_hint_matches("cpu > 10", ""));
        assert!(!syntax_hint_matches("mem > 1 GB", "no-such-process"));
    }

    #[test]
    fn search_completion_leaves_threshold_values_to_the_user() {
        let snap = StatusSnapshot::default();
        for (text, expected) in [
            ("mem", "mem > "),
            ("mem>", "mem > "),
            ("mem<=", "mem <= "),
            ("cpu", "cpu > "),
            ("user", "user:"),
        ] {
            let items = search_candidates(&snap, &[], text, Language::En);
            let item = items
                .iter()
                .find(|item| item.content == CandidateContent::Syntax)
                .expect(text);
            assert_eq!(items[0].content, CandidateContent::Syntax, "{text}");
            assert_eq!(item.query, expected, "{text}");
            assert!(item.keep_open);
        }
        for text in ["mem>1", "mem > 512 MB", "cpu>20", "user:SYSTEM"] {
            let items = search_candidates(&snap, &[], text, Language::En);
            assert!(
                !items
                    .iter()
                    .any(|item| item.content == CandidateContent::Syntax),
                "completed input must not be replaced: {text}"
            );
        }
    }
}
