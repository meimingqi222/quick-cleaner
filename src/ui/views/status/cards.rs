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
/// 卡片只有约 1/4 行宽，徽章是次要信息：位置不够时先缩它、缩不下就裁剪，
/// 绝不许把卡片撑破（P3，`clip` 负责）。
fn header_chip(text: String) -> Badge {
    Badge::new(text).tone(BadgeTone::NeutralSoft).clip()
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
            Meter::new(cpu / 100.0, bar_color)
                .width(px(24.))
                .height(MeterHeight::Hairline),
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
    let current = gpus.get(selected).map(|g| g.id.clone()).unwrap_or_default();
    let mut control = SegmentedControl::new(
        "gpu-switch",
        current,
        cx.listener(|this, id: &String, _, cx| {
            this.select_gpu(id.clone(), cx);
        }),
    )
    .style(SegmentStyle::Solid);
    for (gpu, label) in gpus.iter().zip(gpu_labels(gpus)) {
        control = control.item(SegmentItem::new(gpu.id.clone(), label));
    }
    div().mt_3().child(control)
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
        .child(div().mt_3().child(Meter::new(bat.percent / 100.0, accent)))
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
        .child(div().mt_3().child(Meter::new(ratio, PRIMARY)))
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
        .child(div().mt_3().child(Meter::new(ratio, PRIMARY)))
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
    let mut control = SegmentedControl::new(
        "fan-mode",
        active,
        cx.listener(|this, mode: &FanMode, _, cx| {
            this.apply_fan_mode(*mode, cx);
        }),
    )
    .style(SegmentStyle::Solid);
    for (mode, label) in modes {
        control = control.item(SegmentItem::new(mode, label.to_string()).disabled(applying));
    }
    div().mt_3().child(control)
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
