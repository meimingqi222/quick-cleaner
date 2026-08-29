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
                        if let Some((volume, space)) = volume_space {
                            this.disk.volume_spaces.insert(volume.clone(), space);
                            if this.disk.volume == volume {
                                this.disk.space = Some(space);
                            }
                        }
                        push_history(&mut this.monitor.cpu_history, snap.cpu_usage);
                        if let Some(util) = snap.gpu.utilization {
                            push_history(&mut this.monitor.gpu_history, util);
                        }
                        this.monitor.snapshot = Some(snap);
                        this.monitor.fan_helper_installed = helper_installed;
                        this.rebuild_proc_view();
                        cx.notify();
                        this.view == crate::ui::components::View::Status
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
                cx.background_executor().timer(SAMPLE_INTERVAL).await;
            }
            this.update(cx, |this, _| {
                this.monitor.task = None;
            })
            .ok();
        }));
    }

    /// 按当前排序重建进程表的下标视图。
    ///
    /// 排序结果算一次存下来，而不是塞进虚拟列表的每帧回调——那样九百个
    /// 进程会每帧重排一遍。快照更新和用户点表头时各调一次。
    pub fn rebuild_proc_view(&mut self) {
        let Some(snap) = &self.monitor.snapshot else {
            self.monitor.proc_view.clear();
            return;
        };
        let sort = self.monitor.proc_sort;
        let mut view: Vec<usize> = (0..snap.processes.len()).collect();
        view.sort_by(|&a, &b| {
            let (x, y) = (&snap.processes[a], &snap.processes[b]);
            let ord = match sort.key {
                // partial_cmp 的 None 只可能来自 NaN：真出现了就当相等，
                // 不能 unwrap——排序比较器里 panic 会直接带走整个进程。
                ProcSortKey::Cpu => x
                    .cpu
                    .partial_cmp(&y.cpu)
                    .unwrap_or(std::cmp::Ordering::Equal),
                ProcSortKey::Memory => x.mem_bytes.cmp(&y.mem_bytes),
                ProcSortKey::Name => x.name.to_lowercase().cmp(&y.name.to_lowercase()),
                ProcSortKey::Pid => x.pid.cmp(&y.pid),
            };
            // 主键相等时按 PID 兜底，保证顺序稳定：否则每拍重排都可能把
            // 并列的两行换位置，看起来像在乱跳。
            let ord = if sort.desc { ord.reverse() } else { ord };
            ord.then(x.pid.cmp(&y.pid))
        });
        self.monitor.proc_view = view;
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
        cx.notify();
    }
}
