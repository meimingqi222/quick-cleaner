//! 状态监控动作：后台轮询采样与结束进程

use crate::core::i18n::bilingual;
use crate::core::status::{FanError, FanMode, StatusSampler};
use crate::ui::components::{ConfirmKind, ConfirmRequest};
use crate::ui::i18n::*;
use crate::ui::state::{ProcSort, ProcSortKey, STATUS_HISTORY_LEN};
use gpui::Context;
use std::time::Duration;

/// 采样周期。CPU / 进程 / 网络都是差值型指标，2 秒一拍足够顺滑，
/// 也让 SMC 读数不至于高频打扰硬件。
const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
const SAMPLE_SLICE: Duration = Duration::from_millis(100);

/// 默认选中的 GPU：当前最忙的那张。双显卡机器上核显常年在跑桌面合成，
/// 独显只在真干活时才醒，一上来就把用户带到有数据的那张。
fn busiest_gpu(gpus: &[crate::core::status::GpuReading]) -> Option<String> {
    gpus.iter()
        .max_by(|a, b| {
            a.utilization
                .unwrap_or(0.0)
                .total_cmp(&b.utilization.unwrap_or(0.0))
        })
        .map(|gpu| gpu.id.clone())
}

/// 往柱状历史里推一拍，并把超出一屏的旧数据裁掉。
///
/// saturating_sub：历史不足一屏时多退少补都按 0 算，debug 模式下普通减法
/// 会直接下溢 panic（真机踩过）。
fn push_history(history: &mut Vec<f32>, value: f32) {
    history.push(value);
    let excess = history.len().saturating_sub(STATUS_HISTORY_LEN);
    if excess > 0 {
        history.drain(0..excess);
    }
}

impl crate::ui::Root {
    /// 启动状态监控的轮询任务（幂等）。任务在用户切出状态页时自退出，
    /// 与 `start_tick` 的自停模式一致；重新进入页面时由侧边栏再拉起。
    pub fn start_status_monitor(&mut self, cx: &mut Context<Self>) {
        if self.monitor.task.is_some() {
            return;
        }
        let refresh_trigger = self.monitor.refresh_trigger.clone();
        self.monitor.task = Some(cx.spawn(async move |this, cx| {
            // 采样器必须整个任务期间存活：CPU% / 网络速率都是对上一次
            // 采样的差值，换了采样器第一拍全是失真读数。后台 spawn 要求
            // 'static，所以每拍把采样器 move 进去、连同快照一起拿回来。
            let mut sampler = Some(StatusSampler::new());
            loop {
                // 守护进程的安装状态跟着采样一起在后台取：渲染侧要靠它决定
                // 是否显示「移除组件」，不能每帧去 stat 文件系统。
                let sampled_volume = this.update(cx, |this, _| this.disk.volume.clone()).ok();
                let (snap, helper_installed, volume_space, returned) = cx
                    .background_executor()
                    .spawn(async move {
                        let mut inner = sampler.take().expect("sampler 不该在两拍之间丢失");
                        let snap = inner.sample();
                        let volume_space = sampled_volume.and_then(|volume| {
                            crate::platform::get_volume_space(&volume).map(|space| (volume, space))
                        });
                        (
                            snap,
                            crate::platform::fan_helper_installed(),
                            volume_space,
                            inner,
                        )
                    })
                    .await;
                sampler = Some(returned);
                let keep = this
                    .update(cx, |this, cx| {
                        let manual = refresh_trigger.load(std::sync::atomic::Ordering::Relaxed);
                        if !this.monitor.paused || manual {
                            if let Some((volume, space)) = volume_space {
                                this.disk.volume_spaces.insert(volume.clone(), space);
                                if this.disk.volume == volume {
                                    this.disk.space = Some(space);
                                }
                            }
                            push_history(&mut this.monitor.cpu_history, snap.cpu_usage);
                            for gpu in &snap.gpus {
                                if let Some(util) = gpu.utilization {
                                    push_history(
                                        this.monitor.gpu_history.entry(gpu.id.clone()).or_default(),
                                        util,
                                    );
                                }
                            }
                            // 没选过、或者选中的那张卡不见了（驱动重载、外接显卡
                            // 拔掉），就落到当前最忙的那张上。
                            let selected_gone = this
                                .monitor
                                .gpu_selected
                                .as_ref()
                                .is_none_or(|id| !snap.gpus.iter().any(|g| &g.id == id));
                            if selected_gone {
                                this.monitor.gpu_selected = busiest_gpu(&snap.gpus);
                            }
                            this.monitor.snapshot = Some(snap);
                            this.monitor.fan_helper_installed = helper_installed;
                            this.rebuild_proc_view();
                            cx.notify();
                        }
                        this.view == crate::ui::components::View::Status
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
                // 等待下一拍（2000ms），每 100ms 检查一次是否有立即刷新打断
                let slices = (SAMPLE_INTERVAL.as_millis() / SAMPLE_SLICE.as_millis()).max(1);
                for _ in 0..slices {
                    if refresh_trigger.swap(false, std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    cx.background_executor().timer(SAMPLE_SLICE).await;
                }
            }
            this.update(cx, |this, _| {
                this.monitor.task = None;
            })
            .ok();
        }));
    }

    /// 切换状态监控的子标签页（硬件概览 / 进程管理）
    pub fn set_status_tab(&mut self, tab: crate::ui::state::StatusTab, cx: &mut Context<Self>) {
        if self.monitor.tab != tab {
            self.monitor.tab = tab;
            cx.notify();
        }
    }

    /// 切换监控的暂停/恢复状态
    pub fn toggle_status_pause(&mut self, cx: &mut Context<Self>) {
        self.monitor.paused = !self.monitor.paused;
        if !self.monitor.paused {
            self.monitor
                .refresh_trigger
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        cx.notify();
    }

    /// 立即触发一拍状态采样
    pub fn refresh_status_now(&mut self, cx: &mut Context<Self>) {
        self.monitor
            .refresh_trigger
            .store(true, std::sync::atomic::Ordering::Relaxed);
        cx.notify();
    }

    /// 切换 GPU 卡片显示的那张卡。
    pub fn select_gpu(&mut self, id: String, cx: &mut Context<Self>) {
        if self.monitor.gpu_selected.as_deref() != Some(id.as_str()) {
            self.monitor.gpu_selected = Some(id);
            cx.notify();
        }
    }

    /// 按当前排序与过滤重建进程表的下标视图与分组平铺项。
    pub fn rebuild_proc_view(&mut self) {
        let Some(snap) = &self.monitor.snapshot else {
            self.monitor.proc_view.clear();
            self.monitor.proc_display_items.clear();
            return;
        };
        let sort = self.monitor.proc_sort;
        let query = self.monitor.input.text.trim();
        let tab = self.monitor.filter_tab;
        let current_user = std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .ok();
        let current_user_ref = current_user.as_deref();

        let (view, display_items) = compute_proc_display_items(
            snap,
            sort,
            query,
            tab,
            &self.monitor.expanded_groups,
            current_user_ref,
        );

        self.monitor.proc_view = view;
        self.monitor.proc_display_items = display_items;

        // 6. 更新选中进程的历史读数
        if let Some(selected_pid) = self.monitor.selected_proc_pid {
            if let Some(p) = snap.processes.iter().find(|p| p.pid == selected_pid) {
                push_history(&mut self.monitor.selected_proc_cpu_history, p.cpu);
                self.monitor.selected_proc_mem_history.push(p.mem_bytes);
                let excess = self
                    .monitor
                    .selected_proc_mem_history
                    .len()
                    .saturating_sub(STATUS_HISTORY_LEN);
                if excess > 0 {
                    self.monitor.selected_proc_mem_history.drain(0..excess);
                }
            } else {
                // 进程已退出，取消选中
                self.monitor.selected_proc_pid = None;
                self.monitor.selected_proc_cpu_history.clear();
                self.monitor.selected_proc_mem_history.clear();
            }
        }
    }

    /// 切换某组多进程的折叠/展开状态
    pub fn toggle_proc_group(&mut self, group_key: String, cx: &mut Context<Self>) {
        if self.monitor.expanded_groups.contains(&group_key) {
            self.monitor.expanded_groups.remove(&group_key);
        } else {
            self.monitor.expanded_groups.insert(group_key);
        }
        self.rebuild_proc_view();
        cx.notify();
    }

    /// 选中/取消选中进程，开启或切换右侧侧滑抽屉
    pub fn select_proc_item(&mut self, pid: u32, cx: &mut Context<Self>) {
        if self.monitor.selected_proc_pid == Some(pid) {
            self.monitor.selected_proc_pid = None;
            self.monitor.selected_proc_cpu_history.clear();
            self.monitor.selected_proc_mem_history.clear();
        } else {
            self.monitor.selected_proc_pid = Some(pid);
            self.monitor.selected_proc_cpu_history.clear();
            self.monitor.selected_proc_mem_history.clear();
            if let Some(snap) = &self.monitor.snapshot {
                if let Some(p) = snap.processes.iter().find(|p| p.pid == pid) {
                    self.monitor.selected_proc_cpu_history.push(p.cpu);
                    self.monitor.selected_proc_mem_history.push(p.mem_bytes);
                }
            }
        }
        cx.notify();
    }

    /// 关闭右侧侧滑抽屉
    pub fn close_proc_detail(&mut self, cx: &mut Context<Self>) {
        self.monitor.selected_proc_pid = None;
        self.monitor.selected_proc_cpu_history.clear();
        self.monitor.selected_proc_mem_history.clear();
        cx.notify();
    }

    /// 复制进程命令行到剪贴板并提示
    pub fn copy_proc_cmd(&mut self, cmd: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(cmd));
        self.monitor.cmd_copied_feedback = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            this.update(cx, |this, cx| {
                this.monitor.cmd_copied_feedback = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn reset_status_search_selection(&mut self) {
        self.monitor.search_popover.reset_selection();
    }

    pub fn status_search_candidate_key(&mut self, key: &str, cx: &mut Context<Self>) {
        use crate::ui::components::popover::PopoverKey;
        let candidates = crate::ui::views::status::status_search_candidates(self);
        match self.monitor.search_popover.handle_key(
            key,
            self.monitor.input.marked.is_some(),
            &candidates,
        ) {
            PopoverKey::Confirm(index) => {
                let item = &candidates[index];
                self.status_choose_search_candidate(&item.query, item.keep_open, cx);
            }
            PopoverKey::Changed | PopoverKey::Dismissed => cx.notify(),
            PopoverKey::Unhandled => {}
        }
    }

    pub fn status_choose_search_candidate(
        &mut self,
        query: &str,
        keep_open: bool,
        cx: &mut Context<Self>,
    ) {
        self.status_apply_search_query(query, cx);
        if keep_open {
            self.monitor.search_popover.reopen();
        } else {
            self.monitor.search_popover.dismiss();
        }
    }

    /// 清空进程搜索框
    pub fn status_search_clear(&mut self, cx: &mut Context<Self>) {
        self.monitor.input.clear();
        self.monitor.search_popover.reopen();
        self.reset_status_search_selection();
        self.rebuild_proc_view();
        cx.notify();
    }

    /// 进程搜索框退格
    pub fn status_search_backspace(&mut self, cx: &mut Context<Self>) {
        let input = &mut self.monitor.input;
        input.sel = crate::ui::text_input::delete_backward(&mut input.text, input.sel.clone());
        input.marked = None;
        self.monitor.search_popover.reopen();
        self.reset_status_search_selection();
        self.rebuild_proc_view();
        cx.notify();
    }

    /// 进程搜索框按 Esc 处理：
    /// 若 Popover 浮层当前正在显示，优先收起浮层；
    /// 若浮层已收起或未满足显示条件，且搜索框有内容，则清空搜索框。
    pub fn status_search_escape(&mut self, cx: &mut Context<Self>) {
        if self.monitor.input.marked.is_some() {
            return;
        }
        if self.monitor.search_popover.is_open(true) {
            self.status_search_candidate_key("escape", cx);
        } else if !self.monitor.input.text.is_empty() {
            self.status_search_clear(cx);
        }
    }

    /// 切换进程分类 Tab
    pub fn status_set_filter_tab(
        &mut self,
        tab: crate::core::status::ProcFilterTab,
        cx: &mut Context<Self>,
    ) {
        if self.monitor.filter_tab != tab {
            self.monitor.filter_tab = tab;
            self.rebuild_proc_view();
            self.monitor
                .proc_scroll
                .0
                .borrow()
                .base_handle
                .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
            cx.notify();
        }
    }

    /// 应用搜索建议或关键字填入
    pub fn status_apply_search_query(&mut self, query: &str, cx: &mut Context<Self>) {
        self.monitor.input.set_text(query);
        self.reset_status_search_selection();
        // 若填入的是端口搜索前缀 ":" 则保持/开启浮层，否则收起浮层露出表格过滤结果
        if query == ":" {
            self.monitor.search_popover.reopen();
        } else {
            self.monitor.search_popover.dismiss();
        }
        self.rebuild_proc_view();
        self.monitor
            .proc_scroll
            .0
            .borrow()
            .base_handle
            .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
        cx.notify();
    }

    /// 点击进程表表头：切换排序列 / 方向。
    pub fn sort_processes(&mut self, key: ProcSortKey, cx: &mut Context<Self>) {
        self.monitor.proc_sort = ProcSort::toggled(self.monitor.proc_sort, key);
        self.rebuild_proc_view();
        // 换了排序还停在原来的滚动位置没有意义，回到第一行。
        self.monitor
            .proc_scroll
            .0
            .borrow()
            .base_handle
            .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
        cx.notify();
    }

    /// 给进程表当前**可见区间**里的行补图标。
    ///
    /// 不能像应用页那样一次性全捞：这里有九百来个进程，滚到哪儿才加载哪儿。
    /// `is_cached` 把「已加载」和「已确认没有图标」都挡掉，所以这个函数虽然
    /// 由渲染回调驱动，稳态下不会重复触发。
    pub fn load_visible_process_icons(
        &mut self,
        paths: Vec<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let mut paths: Vec<std::path::PathBuf> = paths
            .into_iter()
            .filter(|p| !crate::ui::app_icons::is_cached(p))
            .collect();
        paths.sort();
        paths.dedup();
        if paths.is_empty() {
            return;
        }
        self.load_process_icons(paths, cx);
    }

    /// 后台提取进程图标，取完重绘一次。
    ///
    /// `load_icons` 会把「没有图标」也记进缓存，配合 `is_cached` 过滤，同一个
    /// 进程不会每帧都去读一次磁盘。
    fn load_process_icons(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move { crate::ui::app_icons::load_icons(paths) })
                .await;
            if loaded > 0 {
                this.update(cx, |_, cx| cx.notify()).ok();
            }
        })
        .detach();
    }

    /// 切换风扇档位（自动 / 温度联动降温 / 全速 100%）。
    ///
    /// 三级通道，逐级降级：
    ///
    /// 1. **进程内直写**：应用本身以 root 跑、或固件放行的机型上直接成功。
    /// 2. **常驻特权守护进程**：直写被固件拒绝（[`FanError::NeedsRoot`]）时走
    ///    unix socket 交给 root 守护进程写。装过一次之后这条路**不弹任何框**。
    /// 3. **安装守护进程**：从没装过（[`FanError::NotInstalled`]）时先弹应用内
    ///    确认框；已经装过但对不上（[`FanError::NeedsUpgrade`]，例如重新打包）
    ///    则直接覆盖，不再先卸、也不再确认——用户已经同意过一次。覆盖仍会
    ///    弹一次系统密码框。
    ///
    /// 强制档位的生命周期绑在 socket 连接上：应用退出/崩溃，守护进程立刻把
    /// 风扇交还系统调速，不会出现「App 没了风扇还锁着」。
    ///
    /// 失败路径必须把 `fan_applying` 放掉：它是按钮的互斥闸，留在 true 上
    /// 会让三个档位按钮直到重启应用都点不动（取消一次密码框就能踩到）。
    pub fn apply_fan_mode(&mut self, mode: FanMode, cx: &mut Context<Self>) {
        self.apply_fan_mode_inner(mode, false, cx);
    }

    /// `allow_install = true` 表示用户已经在确认框里同意安装守护进程，
    /// 这一轮撞上 [`FanError::NotInstalled`] 时直接装，不再问第二遍。
    fn apply_fan_mode_inner(&mut self, mode: FanMode, allow_install: bool, cx: &mut Context<Self>) {
        // 同档位重复点击通常是空操作，但上一次失败回退后（`fan_stale`）高亮的
        // 那一档背后已经没有保活循环，必须允许重新点它把控制权拿回来。
        if self.monitor.fan_applying
            || (self.monitor.fan_mode == mode && !self.monitor.fan_stale && !allow_install)
        {
            return;
        }
        let previous = self.monitor.fan_mode;
        self.monitor.fan_task = None; // 顶掉旧任务：旧循环醒来发现代次易主即自退
        self.monitor.fan_generation = self.monitor.fan_generation.wrapping_add(1);
        let generation = self.monitor.fan_generation;
        self.monitor.fan_mode = mode;
        self.monitor.fan_applying = true;
        // 授权框正文要按界面语言给，而后台任务里没有 `self`，先取出来搬进去。
        let install_prompt = tr_fan_helper_auth_prompt(self.language).to_string();
        cx.notify();
        self.monitor.fan_task = Some(cx.spawn(async move |this, cx| {
            // 只在首轮播报成功文案。`status` 是全局状态栏，保活循环每 3 秒
            // 重播一次会把用户此刻的清理进度、扫描结果反复顶掉。
            let mut announced = false;
            // 首轮直写确认普通 GUI 进程没有 SMC 权限后，后续直接走 helper，
            // 避免每 3 秒重复做一次注定失败的非特权写入。
            let mut use_helper = false;
            loop {
                // 代次易主 = 已有新一轮切换接管，`fan_applying` / `status` 都归它，
                // 这里必须原样退出，不能顺手清闸（那会解开新一轮的按钮互斥）。
                let current = this
                    .update(cx, |this, _| this.monitor.fan_generation == generation)
                    .unwrap_or(false);
                if !current {
                    break;
                }

                let direct_outcome = if use_helper {
                    None
                } else {
                    let direct = cx
                        .background_executor()
                        .spawn(async move { crate::platform::set_fan_mode(mode) })
                        .await;
                    match direct {
                        Err(FanError::NeedsRoot(_)) => {
                            use_helper = true;
                            None
                        }
                        other => Some(other),
                    }
                };

                let outcome = match direct_outcome {
                    Some(result) => result,
                    None => {
                        let via_helper = cx
                            .background_executor()
                            .spawn(async move { crate::platform::elevated_fan_control(mode) })
                            .await;
                        let should_install = match &via_helper {
                            // 重新打包后旧 helper 还在：直接覆盖，不要走到
                            // 「未安装」确认框，更不要让用户先点移除。
                            Err(FanError::NeedsUpgrade) if !announced => true,
                            Err(FanError::NotInstalled) if allow_install => true,
                            _ => false,
                        };
                        if should_install {
                            match cx
                                .background_executor()
                                .spawn({
                                    let prompt = install_prompt.clone();
                                    async move { crate::platform::install_fan_helper(&prompt) }
                                })
                                .await
                            {
                                Ok(()) => {
                                    cx.background_executor()
                                        .spawn(async move {
                                            crate::platform::elevated_fan_control(mode)
                                        })
                                        .await
                                }
                                Err(err) => Err(err),
                            }
                        } else {
                            via_helper
                        }
                    }
                };

                if let Err(err) = outcome {
                    let needs_consent = err == FanError::NotInstalled;
                    // 写新档可能在若干个 SMC 键之间失败。旧保活任务已经被顶掉，
                    // 此时不能继续把 previous 显示成仍受控：先主动交还系统；若
                    // 复位也失败，才保留旧高亮并标 stale 表示硬件状态未知。
                    let direct_restore = cx
                        .background_executor()
                        .spawn(async move { crate::platform::set_fan_mode(FanMode::Auto) })
                        .await;
                    let restored_auto = match direct_restore {
                        Ok(()) => true,
                        Err(FanError::NeedsRoot(_)) => cx
                            .background_executor()
                            .spawn(
                                async move { crate::platform::elevated_fan_control(FanMode::Auto) },
                            )
                            .await
                            .is_ok(),
                        Err(_) => false,
                    };
                    this.update(cx, |this, cx| {
                        if this.monitor.fan_generation != generation {
                            return;
                        }
                        this.monitor.fan_mode = if restored_auto {
                            FanMode::Auto
                        } else {
                            previous
                        };
                        this.monitor.fan_applying = false;
                        this.monitor.fan_stale =
                            !restored_auto && matches!(previous, FanMode::Percent(_));
                        if needs_consent {
                            // 装系统组件是持久化改动，不能靠一句 osascript 密码框
                            // 当知情同意，先在应用内说清楚装什么、怎么卸。
                            this.request_install_fan_helper(mode);
                        } else {
                            this.status = bilingual(|l| match &err {
                                FanError::Canceled => tr_fan_elevate_canceled(l).to_string(),
                                other => tr_status_fan_failed(l, &other.to_string()),
                            });
                        }
                        cx.notify();
                    })
                    .ok();
                    break;
                }

                if !announced {
                    announced = true;
                    this.update(cx, |this, cx| {
                        if this.monitor.fan_generation != generation {
                            return;
                        }
                        this.monitor.fan_applying = false;
                        this.monitor.fan_stale = false;
                        this.monitor.fan_helper_installed = crate::platform::fan_helper_installed();
                        this.status = bilingual(|l| tr_status_fan_ok(l, mode).to_string());
                        cx.notify();
                    })
                    .ok();
                }

                // Percent 档每 3 秒重申。helper 路径的这次请求也兼作健康检查：
                // daemon 因热压力/传感器异常回退 Auto 并断开后，下一拍会收到错误，
                // 从而把 UI 高亮同步回自动，而不是只在硬件层悄悄回退。
                if mode == FanMode::Auto {
                    break;
                }
                cx.background_executor().timer(Duration::from_secs(3)).await;
            }
        }));
    }

    /// 弹确认框征求安装特权守护进程的同意。用户点确认后
    /// `confirm_accept` 会带 `allow_install = true` 重跑一次切换。
    fn request_install_fan_helper(&mut self, mode: FanMode) {
        let lang = self.language;
        self.confirm = Some(ConfirmRequest {
            title: tr_fan_helper_install_title(lang).to_string(),
            body: tr_fan_helper_install_body(lang).to_string(),
            detail: tr_fan_helper_install_detail(lang).to_string(),
            kind: ConfirmKind::InstallFanHelper(mode),
            app_data: false,
        });
    }

    /// 用户在确认框里点了「安装」：带上放行标记重跑切换。
    pub fn install_fan_helper_and_apply(&mut self, mode: FanMode, cx: &mut Context<Self>) {
        self.apply_fan_mode_inner(mode, true, cx);
    }

    /// 卸载特权守护进程（风扇卡片上的「移除系统组件」）。守护进程收到
    /// SIGTERM 会先把风扇交还系统再退出，所以不需要额外先切回自动。
    pub fn uninstall_fan_helper(&mut self, cx: &mut Context<Self>) {
        // 卸载要弹一次授权框，那段时间**不能**再让用户切档：切档会把 helper
        // 重新用起来（甚至重装一遍），而卸载一旦完成又把它删掉，两边互相拆台。
        // 复用切档那把互斥闸，档位按钮在授权期间自动禁用。
        if self.monitor.fan_applying {
            return;
        }
        self.monitor.fan_task = None;
        self.monitor.fan_generation = self.monitor.fan_generation.wrapping_add(1);
        let generation = self.monitor.fan_generation;
        self.monitor.fan_applying = true;
        let remove_prompt = tr_fan_helper_remove_prompt(self.language).to_string();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    // root 直写路径没有 socket 断开兜底，停掉保活任务后也要显式
                    // 复位；普通用户这里失败无妨，uninstall 内 drop_client 会让
                    // 守护进程在授权框出现前恢复 Auto。
                    let _ = crate::platform::set_fan_mode(FanMode::Auto);
                    crate::platform::uninstall_fan_helper(&remove_prompt)
                })
                .await;
            this.update(cx, |this, cx| {
                // 代次易主 = 卸载期间已有别的操作接管，状态归它，这里不能写。
                if this.monitor.fan_generation != generation {
                    return;
                }
                this.monitor.fan_applying = false;
                this.monitor.fan_helper_installed = crate::platform::fan_helper_installed();
                // 即使用户取消卸载，uninstall 也已先断开客户端，daemon 会复位；
                // root 直写路径则由上面的显式 Auto 覆盖。
                this.monitor.fan_mode = FanMode::Auto;
                this.monitor.fan_stale = false;
                this.status = bilingual(|l| match &result {
                    Ok(()) => tr_fan_helper_removed(l).to_string(),
                    Err(FanError::Canceled) => tr_fan_elevate_canceled(l).to_string(),
                    Err(err) => tr_status_fan_failed(l, &err.to_string()),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// 请求结束一个进程：弹二次确认，真正的终止在用户确认后执行。
    pub fn request_kill_process(
        &mut self,
        pid: u32,
        start_time: u64,
        unique_id: Option<u64>,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let lang = self.language;
        self.confirm = Some(ConfirmRequest {
            title: tr_confirm_kill_title(lang).to_string(),
            body: tr_confirm_kill_body(lang, &name, pid),
            detail: tr_confirm_kill_detail(lang).to_string(),
            kind: ConfirmKind::KillProcess {
                pid,
                start_time,
                unique_id,
                name,
            },
            app_data: false,
        });
        cx.notify();
    }

    /// 确认弹窗点下「结束进程」之后走这里。kill/TerminateProcess 是微秒级
    /// 系统调用，直接在主线程做；结果走状态栏，进程列表等下一拍采样刷新。
    /// 确认弹窗的分发在 `actions::clean::confirm_accept`，所以必须是 pub。
    pub fn kill_process(
        &mut self,
        pid: u32,
        start_time: u64,
        unique_id: Option<u64>,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let result = crate::platform::terminate_process(pid, start_time, unique_id);
        self.status = bilingual(|l| match &result {
            Ok(()) => tr_status_kill_ok(l, &name).to_string(),
            Err(err) => tr_status_kill_failed(l, &name, err).to_string(),
        });
        if self.monitor.selected_proc_pid == Some(pid) {
            self.close_proc_detail(cx);
        }
        self.refresh_status_now(cx);
        cx.notify();
    }

    /// 强制退出进程（立即强制终止）
    pub fn force_kill_process(
        &mut self,
        pid: u32,
        start_time: u64,
        unique_id: Option<u64>,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let result = crate::platform::terminate_process(pid, start_time, unique_id);
        self.status = bilingual(|l| match &result {
            Ok(()) => tr_status_kill_ok(l, &name).to_string(),
            Err(err) => tr_status_kill_failed(l, &name, err).to_string(),
        });
        if self.monitor.selected_proc_pid == Some(pid) {
            self.close_proc_detail(cx);
        }
        self.refresh_status_now(cx);
        cx.notify();
    }
}

/// 纯函数：根据排序、搜索关键字、Tab 过滤和当前用户展开的分组集合，
/// 将采样快照中的进程聚合分组，并展平成供虚拟滚动列表渲染的平铺条目。
pub fn compute_proc_display_items(
    snap: &crate::core::status::StatusSnapshot,
    sort: ProcSort,
    query: &str,
    tab: crate::core::status::ProcFilterTab,
    expanded_groups: &std::collections::HashSet<String>,
    current_user_ref: Option<&str>,
) -> (Vec<usize>, Vec<crate::ui::state::ProcDisplayItem>) {
    // 1. 过滤匹配项
    let view: Vec<usize> = (0..snap.processes.len())
        .filter(|&idx| {
            let proc = &snap.processes[idx];
            if !crate::core::status::matches_proc_tab(proc, tab, current_user_ref) {
                return false;
            }
            if !query.is_empty() && !crate::core::status::match_proc_query(proc, query) {
                return false;
            }
            true
        })
        .collect();

    // 2. 将匹配项按进程名称聚合分组
    let mut groups: std::collections::BTreeMap<String, Vec<usize>> =
        std::collections::BTreeMap::new();
    for &idx in &view {
        let proc = &snap.processes[idx];
        let key = proc.name.to_lowercase();
        groups.entry(key).or_default().push(idx);
    }

    // 3. 构建顶层项（单进程或多进程组）
    enum TopItem {
        Group {
            group_key: String,
            name: String,
            icon_source: Option<std::path::PathBuf>,
            count: usize,
            total_cpu: f32,
            total_mem: u64,
            total_threads: u32,
            total_cpu_time_ms: u64,
            representative_pid: u32,
            user: Option<String>,
            child_indices: Vec<usize>,
        },
        Single {
            process_index: usize,
        },
    }

    let mut top_items: Vec<TopItem> = Vec::new();
    for (group_key, mut indices) in groups {
        if indices.len() == 1 {
            top_items.push(TopItem::Single {
                process_index: indices[0],
            });
        } else {
            // 组内子项按当前列排序
            indices.sort_by(|&a, &b| {
                let (x, y) = (&snap.processes[a], &snap.processes[b]);
                let ord = match sort.key {
                    ProcSortKey::Cpu => x
                        .cpu
                        .partial_cmp(&y.cpu)
                        .unwrap_or(std::cmp::Ordering::Equal),
                    ProcSortKey::Memory => x.mem_bytes.cmp(&y.mem_bytes),
                    ProcSortKey::Name => x.name.to_lowercase().cmp(&y.name.to_lowercase()),
                    ProcSortKey::Pid => x.pid.cmp(&y.pid),
                    ProcSortKey::Threads => x.threads.cmp(&y.threads),
                    ProcSortKey::CpuTime => x.cpu_time_ms.cmp(&y.cpu_time_ms),
                    ProcSortKey::User => x
                        .user
                        .as_deref()
                        .unwrap_or("")
                        .cmp(y.user.as_deref().unwrap_or("")),
                };
                let ord = if sort.desc { ord.reverse() } else { ord };
                ord.then(x.pid.cmp(&y.pid))
            });

            let first = &snap.processes[indices[0]];
            let name = first.name.clone();
            let icon_source = first.icon_source.clone();
            let mut total_cpu = 0.0;
            let mut total_mem = 0;
            let mut total_threads = 0;
            let mut total_cpu_time_ms = 0;
            let mut representative_pid = first.pid;
            let user = first.user.clone();

            for &idx in &indices {
                let p = &snap.processes[idx];
                total_cpu += p.cpu;
                total_mem += p.mem_bytes;
                total_threads += p.threads;
                total_cpu_time_ms += p.cpu_time_ms;
            }

            // 挑选 representative_pid：如果存在某个进程是组内其它进程的 parent，选它
            let pids: std::collections::HashSet<u32> =
                indices.iter().map(|&i| snap.processes[i].pid).collect();
            for &idx in &indices {
                let p = &snap.processes[idx];
                if let Some(ppid) = p.parent_pid {
                    if pids.contains(&ppid) {
                        representative_pid = ppid;
                        break;
                    }
                }
            }

            top_items.push(TopItem::Group {
                group_key,
                name,
                icon_source,
                count: indices.len(),
                total_cpu,
                total_mem,
                total_threads,
                total_cpu_time_ms,
                representative_pid,
                user,
                child_indices: indices,
            });
        }
    }

    // 4. 对顶层项按当前排序列与方向排序
    top_items.sort_by(|a, b| {
        let (cpu_a, mem_a, name_a, pid_a, threads_a, cpu_time_a, user_a) = match a {
            TopItem::Group {
                total_cpu,
                total_mem,
                name,
                representative_pid,
                total_threads,
                total_cpu_time_ms,
                user,
                ..
            } => (
                *total_cpu,
                *total_mem,
                name.as_str(),
                *representative_pid,
                *total_threads,
                *total_cpu_time_ms,
                user.as_deref().unwrap_or(""),
            ),
            TopItem::Single { process_index } => {
                let p = &snap.processes[*process_index];
                (
                    p.cpu,
                    p.mem_bytes,
                    p.name.as_str(),
                    p.pid,
                    p.threads,
                    p.cpu_time_ms,
                    p.user.as_deref().unwrap_or(""),
                )
            }
        };
        let (cpu_b, mem_b, name_b, pid_b, threads_b, cpu_time_b, user_b) = match b {
            TopItem::Group {
                total_cpu,
                total_mem,
                name,
                representative_pid,
                total_threads,
                total_cpu_time_ms,
                user,
                ..
            } => (
                *total_cpu,
                *total_mem,
                name.as_str(),
                *representative_pid,
                *total_threads,
                *total_cpu_time_ms,
                user.as_deref().unwrap_or(""),
            ),
            TopItem::Single { process_index } => {
                let p = &snap.processes[*process_index];
                (
                    p.cpu,
                    p.mem_bytes,
                    p.name.as_str(),
                    p.pid,
                    p.threads,
                    p.cpu_time_ms,
                    p.user.as_deref().unwrap_or(""),
                )
            }
        };

        let ord = match sort.key {
            ProcSortKey::Cpu => cpu_a
                .partial_cmp(&cpu_b)
                .unwrap_or(std::cmp::Ordering::Equal),
            ProcSortKey::Memory => mem_a.cmp(&mem_b),
            ProcSortKey::Name => name_a.to_lowercase().cmp(&name_b.to_lowercase()),
            ProcSortKey::Pid => pid_a.cmp(&pid_b),
            ProcSortKey::Threads => threads_a.cmp(&threads_b),
            ProcSortKey::CpuTime => cpu_time_a.cmp(&cpu_time_b),
            ProcSortKey::User => user_a.to_lowercase().cmp(&user_b.to_lowercase()),
        };
        let ord = if sort.desc { ord.reverse() } else { ord };
        ord.then(pid_a.cmp(&pid_b))
    });

    // 5. 展平成平铺渲染列表 proc_display_items
    let mut display_items = Vec::new();
    for item in top_items {
        match item {
            TopItem::Group {
                group_key,
                name,
                icon_source,
                count,
                total_cpu,
                total_mem,
                total_threads,
                total_cpu_time_ms,
                representative_pid,
                user,
                child_indices,
            } => {
                let expanded = expanded_groups.contains(&group_key);
                display_items.push(crate::ui::state::ProcDisplayItem::GroupHeader {
                    group_key: group_key.clone(),
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
                    child_indices: child_indices.clone(),
                });
                if expanded {
                    for child_idx in child_indices {
                        display_items.push(crate::ui::state::ProcDisplayItem::GroupChild {
                            group_key: group_key.clone(),
                            process_index: child_idx,
                        });
                    }
                }
            }
            TopItem::Single { process_index } => {
                display_items.push(crate::ui::state::ProcDisplayItem::Single { process_index });
            }
        }
    }

    (view, display_items)
}

/// 输入只筛选建议；浮层的生命周期由焦点和显式关闭控制。
#[cfg(test)]
fn status_popover_should_show(search_focused: bool, dismissed: bool, _search_text: &str) -> bool {
    let mut state = crate::ui::components::popover::PopoverState::default();
    if dismissed {
        state.dismiss();
    }
    state.is_open(search_focused)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_popover_visibility_rules() {
        // 1. 未聚焦时绝不展示
        assert!(!status_popover_should_show(false, false, ""));
        assert!(!status_popover_should_show(false, false, ":80"));
        assert!(!status_popover_should_show(false, false, "chrome"));

        // 2. 聚焦且未关闭：输入为空展示语法帮助
        assert!(status_popover_should_show(true, false, ""));
        assert!(status_popover_should_show(true, false, "   "));

        // 3. 聚焦且未关闭：输入含冒号展示端口匹配
        assert!(status_popover_should_show(true, false, ":"));
        assert!(status_popover_should_show(true, false, ":80"));
        assert!(status_popover_should_show(true, false, "port:443"));

        // 输入只改变建议内容，不改变浮层可见性。
        assert!(status_popover_should_show(true, false, "chrome"));
        assert!(status_popover_should_show(true, false, "mem > 1 GB"));
        assert!(status_popover_should_show(true, false, "-helper"));

        assert!(status_popover_should_show(true, false, "mem"));
        assert!(status_popover_should_show(true, false, "ms"));

        // 5. 用户主动按 Esc 或点击收起（dismissed = true）：无论内容为何均不展示
        assert!(!status_popover_should_show(true, true, ""));
        assert!(!status_popover_should_show(true, true, ":80"));
        assert!(!status_popover_should_show(true, true, "chrome"));
    }

    fn make_test_proc(
        pid: u32,
        name: &str,
        cpu: f32,
        mem_mb: u64,
        parent_pid: Option<u32>,
    ) -> crate::core::status::ProcInfo {
        crate::core::status::ProcInfo {
            pid,
            name: name.to_string(),
            cpu,
            mem_bytes: mem_mb * 1024 * 1024,
            parent_pid,
            threads: 4,
            cpu_time_ms: 1000,
            ..Default::default()
        }
    }

    fn make_test_snapshot(
        procs: Vec<crate::core::status::ProcInfo>,
    ) -> crate::core::status::StatusSnapshot {
        let process_count = procs.len();
        crate::core::status::StatusSnapshot {
            processes: procs,
            process_count,
            ..Default::default()
        }
    }

    #[test]
    fn proc_aggregation_and_expansion_logic() {
        let procs = vec![
            make_test_proc(100, "msedge.exe", 1.5, 200, None),
            make_test_proc(101, "msedge.exe", 2.0, 300, Some(100)),
            make_test_proc(102, "msedge.exe", 0.5, 100, Some(100)),
            make_test_proc(200, "notepad.exe", 0.1, 50, None),
        ];
        let snap = make_test_snapshot(procs);
        let mut expanded_groups = std::collections::HashSet::new();

        // 1. 折叠状态（默认未展开 msedge.exe）
        let (view, items) = compute_proc_display_items(
            &snap,
            ProcSort::default(),
            "",
            crate::core::status::ProcFilterTab::All,
            &expanded_groups,
            None,
        );

        assert_eq!(view.len(), 4, "4 个进程全部匹配");
        assert_eq!(
            items.len(),
            2,
            "折叠后只有 2 行：1 个 msedge 聚合头 + 1 个 notepad 单进程"
        );

        match &items[0] {
            crate::ui::state::ProcDisplayItem::GroupHeader {
                group_key,
                count,
                expanded,
                total_cpu,
                total_mem,
                representative_pid,
                child_indices,
                ..
            } => {
                assert_eq!(group_key, "msedge.exe");
                assert_eq!(*count, 3);
                assert!(!expanded);
                assert!((total_cpu - 4.0).abs() < 0.001);
                assert_eq!(*total_mem, 600 * 1024 * 1024);
                assert_eq!(*representative_pid, 100, "识别 PID 100 为父进程代表 PID");
                assert_eq!(child_indices.len(), 3);
            }
            _ => panic!("第一项应当是 GroupHeader"),
        }

        match &items[1] {
            crate::ui::state::ProcDisplayItem::Single { process_index } => {
                assert_eq!(snap.processes[*process_index].name, "notepad.exe");
            }
            _ => panic!("第二项应当是 Single"),
        }

        // 2. 展开 msedge.exe 分组
        expanded_groups.insert("msedge.exe".to_string());
        let (_, expanded_items) = compute_proc_display_items(
            &snap,
            ProcSort::default(),
            "",
            crate::core::status::ProcFilterTab::All,
            &expanded_groups,
            None,
        );

        assert_eq!(
            expanded_items.len(),
            5,
            "展开后为 1 (GroupHeader) + 3 (GroupChild) + 1 (Single) = 5 行"
        );
        match &expanded_items[0] {
            crate::ui::state::ProcDisplayItem::GroupHeader { expanded, .. } => {
                assert!(expanded);
            }
            _ => panic!("第一项应当是已展开的 GroupHeader"),
        }
        for (i, item) in expanded_items[1..4].iter().enumerate() {
            match item {
                crate::ui::state::ProcDisplayItem::GroupChild {
                    group_key,
                    process_index,
                } => {
                    assert_eq!(group_key, "msedge.exe");
                    assert_eq!(snap.processes[*process_index].name, "msedge.exe");
                }
                _ => panic!("第 {} 项应当是 GroupChild", i + 1),
            }
        }
    }

    #[test]
    fn proc_sorting_by_aggregated_metrics() {
        let procs = vec![
            make_test_proc(10, "worker.exe", 3.0, 100, None),
            make_test_proc(11, "worker.exe", 4.0, 100, None),
            make_test_proc(20, "single.exe", 5.0, 100, None),
        ];
        let snap = make_test_snapshot(procs);
        let expanded = std::collections::HashSet::new();

        // worker 总 CPU 是 7.0，single 是 5.0
        // CPU 降序 (默认) -> worker 应该在第一项
        let (_, items_desc) = compute_proc_display_items(
            &snap,
            ProcSort {
                key: ProcSortKey::Cpu,
                desc: true,
            },
            "",
            crate::core::status::ProcFilterTab::All,
            &expanded,
            None,
        );
        // worker 组内 PID 11 CPU (4.0) 高于 PID 10 (3.0)，降序时选为代表 PID
        assert_eq!(items_desc[0].pid(&snap), 11);

        // CPU 升序 -> single (5.0) 应该在第一项
        let (_, items_asc) = compute_proc_display_items(
            &snap,
            ProcSort {
                key: ProcSortKey::Cpu,
                desc: false,
            },
            "",
            crate::core::status::ProcFilterTab::All,
            &expanded,
            None,
        );
        assert_eq!(items_asc[0].pid(&snap), 20);
    }

    #[test]
    fn proc_display_item_pid_lookup() {
        let procs = vec![
            make_test_proc(100, "app.exe", 1.0, 100, None),
            make_test_proc(101, "app.exe", 1.0, 100, None),
            make_test_proc(200, "other.exe", 1.0, 100, None),
        ];
        let snap = make_test_snapshot(procs);

        let header = crate::ui::state::ProcDisplayItem::GroupHeader {
            group_key: "app.exe".into(),
            name: "app.exe".into(),
            icon_source: None,
            count: 2,
            expanded: false,
            total_cpu: 2.0,
            total_mem: 200,
            total_threads: 8,
            total_cpu_time_ms: 2000,
            representative_pid: 100,
            user: None,
            child_indices: vec![0, 1],
        };
        let child = crate::ui::state::ProcDisplayItem::GroupChild {
            group_key: "app.exe".into(),
            process_index: 1,
        };
        let single = crate::ui::state::ProcDisplayItem::Single { process_index: 2 };

        assert_eq!(header.pid(&snap), 100);
        assert_eq!(child.pid(&snap), 101);
        assert_eq!(single.pid(&snap), 200);
    }
}
